//! ATTRIB, XCOPY, CHKDSK, EJECT.

use super::files::{ask_overwrite, confirm_overwrite, same_file, Answer};
use super::Builtin;
use crate::fmt::thousands;
use crate::parse::Args;
use crate::shell::{err, Io, Shell};
use crate::{sysinfo, term};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub const ATTRIB: Builtin = Builtin {
    names: &["ATTRIB"],
    summary: "Displays or changes file attributes.",
    usage: "ATTRIB [+R | -R] [[drive:][path]filename] [/S]\n\n  +   Sets an attribute.\n  -   Clears an attribute.\n  R   Read-only file attribute.\n  /S  Processes matching files in all subdirectories.\n\nA file is Hidden (H) when its name starts with a dot, and a Program (X)\nwhen it can be run.",
    run: attrib,
};

fn attrib(sh: &mut Shell, a: &Args, io: &mut Io) -> i32 {
    let mut set: Option<bool> = None;
    let mut pattern = None;
    for w in &a.words {
        match w.to_ascii_uppercase().as_str() {
            "+R" => set = Some(true),
            "-R" => set = Some(false),
            "+H" | "-H" | "+S" | "-S" | "+A" | "-A" => {
                return err(
                    io,
                    "Only +R and -R can be changed (hidden files are those named .something)",
                );
            }
            _ => pattern = Some(w.clone()),
        }
    }
    let pattern = pattern.unwrap_or_else(|| "*.*".into());
    let mut files = Vec::new();
    collect(sh, &pattern, a.has("S"), &mut files);
    if files.is_empty() {
        return err(io, &format!("File not found - {pattern}"));
    }
    let mut code = 0;
    for p in files {
        if let Some(ro) = set {
            if let Ok(m) = std::fs::metadata(&p) {
                let mut perm = m.permissions();
                let mode = perm.mode();
                perm.set_mode(if ro { mode & !0o222 } else { mode | 0o200 });
                if std::fs::set_permissions(&p, perm).is_err() {
                    let _ = writeln!(io.out, "Access denied - {}", sh.dos.display(&p));
                    code = 1;
                }
            }
            continue;
        }
        let Ok(m) = std::fs::metadata(&p) else {
            continue;
        };
        let mode = m.permissions().mode();
        let name = p.file_name().unwrap_or_default().to_string_lossy();
        let flag = |on: bool, c: char| if on { c } else { ' ' };
        let _ = writeln!(
            io.out,
            "  {}  {}  {}    {}",
            flag(mode & 0o200 == 0, 'R'),
            flag(name.starts_with('.'), 'H'),
            flag(m.is_file() && mode & 0o111 != 0, 'X'),
            sh.dos.display(&p)
        );
    }
    code
}

/// Files matching `pattern` (with its directory), optionally in subdirectories.
fn collect(sh: &Shell, pattern: &str, recurse: bool, out: &mut Vec<PathBuf>) {
    let full = match sh.dos.resolve_new(pattern) {
        Ok(p) => p,
        Err(_) => return,
    };
    let (dir, mask) = if full.is_dir() {
        (full, "*.*".to_string())
    } else {
        (
            full.parent().map(Path::to_path_buf).unwrap_or_default(),
            full.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        )
    };
    walk(&dir, &mask, recurse, out);
}

fn walk(dir: &Path, mask: &str, recurse: bool, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in &entries {
        let name = p.file_name().unwrap_or_default().to_string_lossy();
        if p.is_file() && retro_dosfs::dos_match(mask, &name) {
            out.push(p.clone());
        }
    }
    if recurse {
        for p in entries.iter().filter(|p| p.is_dir() && !p.is_symlink()) {
            walk(p, mask, recurse, out);
        }
    }
}

pub const XCOPY: Builtin = Builtin {
    names: &["XCOPY"],
    summary: "Copies files and directory trees.",
    usage: "XCOPY source [destination] [/S [/E]] [/Y | /-Y] [/P] [/Q]\n\n  source       Specifies the file(s) to copy.\n  destination  Specifies the location and/or name of new files.\n  /S           Copies directories and subdirectories except empty ones.\n  /E           Copies any subdirectories, even if empty (with /S).\n  /P           Prompts you before creating each destination file.\n  /Q           Does not display file names while copying.\n  /Y           Suppresses prompting to confirm overwriting a file.\n  /-Y          Prompts to confirm overwriting a file.",
    run: xcopy,
};

fn xcopy(sh: &mut Shell, a: &Args, io: &mut Io) -> i32 {
    let Some(src) = a.words.first() else {
        return err(io, "Required parameter missing");
    };
    let dest = a.words.get(1).cloned().unwrap_or_else(|| ".".into());
    let Ok(src_full) = sh.dos.resolve_new(src) else {
        return err(io, "Invalid path");
    };
    let (base, mask) = if src_full.is_dir() {
        (src_full.clone(), "*.*".to_string())
    } else {
        (
            src_full.parent().map(Path::to_path_buf).unwrap_or_default(),
            src_full
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        )
    };
    let Ok(dest_full) = sh.dos.resolve_new(&dest) else {
        return err(io, "Invalid path");
    };
    let recurse = a.has("S");
    let mut files = Vec::new();
    walk(&base, &mask, recurse, &mut files);
    if files.is_empty() {
        return err(io, &format!("File not found - {src}"));
    }
    // One file onto a name that is not a directory: copy to that name.
    let single = files.len() == 1 && !recurse && !dest_full.is_dir();
    if !single && !dest_full.exists() {
        if let Err(e) = std::fs::create_dir_all(&dest_full) {
            return err(io, &crate::shell::os_error(&e));
        }
    }
    let mut ask = confirm_overwrite(a, io);
    let mut n = 0;
    for f in &files {
        if term::interrupted() {
            break;
        }
        let rel = f.strip_prefix(&base).unwrap_or(f);
        let target = if single {
            dest_full.clone()
        } else {
            dest_full.join(rel)
        };
        if same_file(f, &target) {
            return err(io, "File cannot be copied onto itself");
        }
        if a.has("P") && term::ask(&format!("{} (Y/N)? ", sh.dos.display(f)), "YN") != 'Y' {
            continue;
        }
        if target.exists() && ask {
            match ask_overwrite(&sh.dos.display(&target)) {
                Answer::No => continue,
                Answer::All => ask = false,
                Answer::Yes => {}
            }
        }
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if !a.has("Q") {
            let _ = writeln!(io.out, "{}", sh.dos.display(f));
        }
        if let Err(e) = std::fs::copy(f, &target) {
            let _ = writeln!(io.out, "{}", crate::shell::os_error(&e));
            break;
        }
        n += 1;
    }
    if recurse && a.has("E") {
        copy_empty_dirs(&base, &dest_full);
    }
    let _ = writeln!(io.out, "{n} File(s) copied");
    i32::from(n == 0)
}

fn copy_empty_dirs(src: &Path, dest: &Path) {
    let Ok(rd) = std::fs::read_dir(src) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() && !p.is_symlink() {
            let t = dest.join(e.file_name());
            let _ = std::fs::create_dir_all(&t);
            copy_empty_dirs(&p, &t);
        }
    }
}

pub const CHKDSK: Builtin = Builtin {
    names: &["CHKDSK"],
    summary: "Displays a status report for a disk.",
    usage: "CHKDSK [drive:]\n\nShows the size of the disk, the space used by directories and files, and the\nfree space, followed by a memory summary.",
    run: chkdsk,
};

fn chkdsk(sh: &mut Shell, a: &Args, io: &mut Io) -> i32 {
    let spec = a
        .words
        .first()
        .cloned()
        .unwrap_or_else(|| format!("{}:", sh.dos.current_drive()));
    let Some(letter) = spec.chars().next().filter(|c| c.is_ascii_alphabetic()) else {
        return err(io, "Invalid drive specification");
    };
    let Some(drive) = sh.dos.drive(letter.to_ascii_uppercase()).cloned() else {
        return err(io, "Invalid drive specification");
    };
    let Some(fs) = sysinfo::statvfs(&drive.root) else {
        return err(io, "Drive not ready");
    };
    // Count what lives on the drive (bounded, so a huge disk stays quick).
    let (mut files, mut dirs, mut bytes) = (0u64, 0u64, 0u64);
    let mut stack = vec![drive.root.clone()];
    let dev = std::fs::metadata(&drive.root)
        .map(|m| std::os::unix::fs::MetadataExt::dev(&m))
        .ok();
    let mut budget = 200_000;
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            budget -= 1;
            if budget == 0 || term::interrupted() {
                stack.clear();
                break;
            }
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() {
                if dev == Some(std::os::unix::fs::MetadataExt::dev(&m)) {
                    dirs += 1;
                    stack.push(e.path());
                }
            } else {
                files += 1;
                bytes += m.len();
            }
        }
    }
    let label = if drive.label.is_empty() {
        "has no label".to_string()
    } else {
        format!("is {}", drive.label)
    };
    let mi = sysinfo::meminfo();
    let w = 14;
    let _ = writeln!(io.out, "\nVolume in drive {} {label}", drive.letter);
    let _ = writeln!(io.out, "Volume Serial Number is {}\n", fs.serial);
    let _ = writeln!(io.out, "{:>w$} bytes total disk space", thousands(fs.total));
    let _ = writeln!(
        io.out,
        "{:>w$} bytes in {} directories",
        thousands(dirs * 4096),
        thousands(dirs)
    );
    let _ = writeln!(
        io.out,
        "{:>w$} bytes in {} user files",
        thousands(bytes),
        thousands(files)
    );
    let _ = writeln!(
        io.out,
        "{:>w$} bytes available on disk\n",
        thousands(fs.free)
    );
    if budget == 0 {
        let _ = writeln!(io.out, "(Counted the first 200,000 entries only.)\n");
    }
    let total = mi.get("MemTotal").copied().unwrap_or(0);
    let avail = mi.get("MemAvailable").copied().unwrap_or(0);
    let _ = writeln!(io.out, "{:>w$} total bytes memory", thousands(total));
    let _ = writeln!(io.out, "{:>w$} bytes free", thousands(avail));
    0
}

pub const EJECT: Builtin = Builtin {
    names: &["EJECT"],
    summary: "Prepares a removable drive to be unplugged.",
    usage: "EJECT drive:\n\nWrites everything still waiting to the drive and unmounts it. When it says\nso, the drive can be removed safely.",
    run: |sh, a, io| {
        let Some(letter) = a.words.first().and_then(|w| w.chars().next()) else {
            return err(io, "Required parameter missing");
        };
        let letter = letter.to_ascii_uppercase();
        let Some(drive) = sh.dos.drive(letter).cloned() else {
            return err(io, "Invalid drive specification");
        };
        if drive.kind != retro_dosfs::DriveKind::Removable {
            return err(io, &format!("Drive {letter}: is not a removable drive"));
        }
        if sh.dos.cwd_path().starts_with(&drive.root) {
            let _ = sh.dos.set_drive('C');
        }
        // Which device is mounted there?
        let mountinfo = std::fs::read_to_string("/proc/self/mountinfo").unwrap_or_default();
        let root = drive.root.to_string_lossy().into_owned();
        let source = mountinfo.lines().find_map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            let sep = f.iter().position(|x| *x == "-")?;
            (f.get(4) == Some(&root.as_str())).then(|| f.get(sep + 2).map(|s| s.to_string()))?
        });
        let Some(source) = source else {
            return err(io, &format!("Drive {letter}: is not mounted"));
        };
        let name = source.rsplit('/').next().unwrap_or(&source).to_string();
        unsafe { libc::sync() };
        let _ = io.out.flush();
        let ok = std::process::Command::new("systemctl")
            .args(["stop", "--no-ask-password", &format!("retro-automount@{name}.service")])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
            && !drive.root.exists();
        if ok {
            let user = sh.user.clone();
            sh.dos.refresh_removable(&user);
            let _ = writeln!(io.out, "Drive {letter}: can now be removed safely.");
            0
        } else {
            err(io, &format!("Drive {letter}: is in use and cannot be ejected"))
        }
    },
};

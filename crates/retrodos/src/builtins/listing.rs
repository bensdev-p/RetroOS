//! DIR and TREE.

use super::{Builtin, Pager};
use crate::fmt::{date_short, mtime, thousands, time_short};
use crate::parse::Args;
use crate::shell::{err, Io, Shell};
use crate::sysinfo;
use retro_dosfs::{dos_match, has_wildcards, short_names, split_83};
use std::path::{Path, PathBuf};

pub const DIR: Builtin = Builtin {
    names: &["DIR"],
    summary: "Displays a list of files and subdirectories in a directory.",
    usage: "DIR [drive:][path][filename] [/P] [/W] [/A[[:]attributes]] [/O[[:]sortorder]] [/S] [/B]\n\n  [drive:][path][filename]\n              Specifies drive, directory, and/or files to list.\n  /P          Pauses after each screenful of information.\n  /W          Uses wide list format.\n  /A          Displays files with specified attributes.\n  attributes   D  Directories   H  Hidden files   R  Read-only files\n               -  Prefix meaning \"not\"\n  /O          List by files in sorted order.\n  sortorder    N  By name (alphabetic)   S  By size (smallest first)\n               E  By extension           D  By date & time (earliest first)\n               G  Group directories first   -  Prefix to reverse order\n  /S          Displays files in specified directory and all subdirectories.\n  /B          Uses bare format (no heading information or summary).",
    run: dir,
};

pub const TREE: Builtin = Builtin {
    names: &["TREE"],
    summary: "Graphically displays the directory structure of a drive or path.",
    usage: "TREE [drive:][path] [/F] [/A]\n\n  /F   Displays the names of the files in each directory.\n  /A   Uses ASCII instead of extended characters.",
    run: tree,
};

struct Entry {
    name: String,
    short: String,
    is_dir: bool,
    size: u64,
    mtime: i64,
    hidden: bool,
    readonly: bool,
}

fn read_entries(dir: &Path, pattern: &str, a: &Args) -> Vec<Entry> {
    let names = retro_dosfs::list_names(dir);
    let shorts = short_names(&names);
    let attrs = a.value("A");
    let mut v: Vec<Entry> = Vec::new();
    for n in &names {
        let short = shorts.get(n).cloned().unwrap_or_else(|| n.to_uppercase());
        if !(dos_match(pattern, n) || dos_match(pattern, &short)) {
            continue;
        }
        let p = dir.join(n);
        let md = std::fs::metadata(&p).or_else(|_| std::fs::symlink_metadata(&p));
        let Ok(md) = md else { continue };
        let e = Entry {
            name: n.clone(),
            short,
            is_dir: md.is_dir(),
            size: md.len(),
            mtime: md.modified().map(mtime).unwrap_or(0),
            hidden: n.starts_with('.'),
            readonly: md.permissions().readonly(),
        };
        if !attr_filter(&e, attrs.as_deref()) {
            continue;
        }
        v.push(e);
    }
    sort_entries(&mut v, a.value("O").as_deref());
    v
}

/// DOS /A semantics: no /A hides hidden files; /A alone shows everything.
fn attr_filter(e: &Entry, attrs: Option<&str>) -> bool {
    let Some(spec) = attrs else { return !e.hidden };
    if spec.is_empty() {
        return true;
    }
    let mut neg = false;
    let mut hidden_named = false;
    for c in spec.chars() {
        let have = match c {
            '-' => {
                neg = true;
                continue;
            }
            'D' => e.is_dir,
            'H' => {
                hidden_named = true;
                e.hidden
            }
            'R' => e.readonly,
            'S' | 'A' => false,
            _ => continue,
        };
        if have == neg {
            return false;
        }
        neg = false;
    }
    hidden_named || !e.hidden
}

fn sort_entries(v: &mut [Entry], order: Option<&str>) {
    let order = order.unwrap_or("GN");
    let order = if order.is_empty() { "GN" } else { order };
    v.sort_by(|a, b| {
        let mut rev = false;
        for c in order.chars() {
            let ord = match c {
                '-' => {
                    rev = true;
                    continue;
                }
                'N' => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                'E' => split_83(&a.name)
                    .1
                    .to_lowercase()
                    .cmp(&split_83(&b.name).1.to_lowercase()),
                'S' => a.size.cmp(&b.size),
                'D' => a.mtime.cmp(&b.mtime),
                'G' => b.is_dir.cmp(&a.is_dir),
                _ => std::cmp::Ordering::Equal,
            };
            let ord = if rev { ord.reverse() } else { ord };
            rev = false;
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
        }
        std::cmp::Ordering::Equal
    });
}

struct Totals {
    files: u64,
    dirs: u64,
    bytes: u64,
}

fn dir(sh: &mut Shell, a: &Args, io: &mut Io) -> i32 {
    let target = a.words.first().cloned().unwrap_or_default();
    let full = match sh
        .dos
        .resolve(if target.is_empty() { "." } else { &target })
    {
        Ok(p) => p,
        Err(e) => return err(io, &e.to_string()),
    };
    // A directory lists its contents; otherwise the last component is a pattern.
    let (dir, pattern) = if full.is_dir() {
        (full.clone(), "*".to_string())
    } else {
        let name = full
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        (
            full.parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("/")),
            name,
        )
    };
    if !dir.is_dir() {
        return err(io, "Path not found");
    }
    let bare = a.has("B");
    let mut pager = Pager::new(a.has("P"));
    let label = sh
        .dos
        .drive(sh.dos.to_dos(&dir).0)
        .map(|d| d.label.clone())
        .unwrap_or_default();
    let letter = sh.dos.to_dos(&dir).0;
    if !bare {
        let fs = sysinfo::statvfs(&dir);
        let _ = writeln!(io.out, " Volume in drive {letter} is {label}");
        if let Some(fs) = &fs {
            let _ = writeln!(io.out, " Volume Serial Number is {}", fs.serial);
        }
    }
    let mut tot = Totals {
        files: 0,
        dirs: 0,
        bytes: 0,
    };
    let mut any = false;
    let ok = list_dir(
        sh, &dir, &pattern, a, io, &mut pager, &mut tot, &mut any, true,
    );
    if !ok {
        return 1;
    }
    if !any && !bare {
        let _ = writeln!(io.out, "File not found");
        return 1;
    }
    if !bare {
        if a.has("S") {
            let _ = writeln!(io.out, "\n     Total files listed:");
            let _ = writeln!(
                io.out,
                "     {:>10} file(s) {:>14} bytes",
                tot.files,
                thousands(tot.bytes)
            );
        }
        let free = sysinfo::statvfs(&dir).map(|f| f.free).unwrap_or(0);
        let _ = writeln!(
            io.out,
            "     {:>10} dir(s)  {:>14} bytes free",
            tot.dirs,
            thousands(free)
        );
    }
    if has_wildcards(&pattern) || full.is_dir() || any {
        0
    } else {
        1
    }
}

#[allow(clippy::too_many_arguments)]
fn list_dir(
    sh: &Shell,
    dir: &Path,
    pattern: &str,
    a: &Args,
    io: &mut Io,
    pager: &mut Pager,
    tot: &mut Totals,
    any: &mut bool,
    top: bool,
) -> bool {
    let bare = a.has("B");
    let wide = a.has("W");
    let entries = read_entries(dir, pattern, a);
    let lfn = sh.dos.lfn;
    if !entries.is_empty() || top {
        if !bare {
            let _ = writeln!(io.out, "\n Directory of {}\n", sh.dos.display(dir));
            for _ in 0..3 {
                if !pager.line(io) {
                    return false;
                }
            }
        }
        let (mut files, mut bytes, mut dirs) = (0u64, 0u64, 0u64);
        // "." and ".." are shown like DOS, except at a drive root.
        let at_root = sh
            .dos
            .drive(sh.dos.to_dos(dir).0)
            .is_some_and(|d| d.root == dir);
        let mut rows: Vec<(String, bool, u64, i64, String)> = Vec::new();
        if !bare && !at_root && (pattern == "*" || pattern == "*.*") {
            let t = std::fs::metadata(dir)
                .and_then(|m| m.modified())
                .map(mtime)
                .unwrap_or(0);
            rows.push((".".into(), true, 0, t, String::new()));
            rows.push(("..".into(), true, 0, t, String::new()));
        }
        for e in &entries {
            rows.push((e.short.clone(), e.is_dir, e.size, e.mtime, e.name.clone()));
        }
        if wide && !bare {
            let names: Vec<String> = rows
                .iter()
                .map(|(s, d, _, _, l)| {
                    let n = if lfn && !l.is_empty() {
                        l.clone()
                    } else {
                        s.clone()
                    };
                    if *d {
                        format!("[{n}]")
                    } else {
                        n
                    }
                })
                .collect();
            let (cols, _) = crate::term::size();
            let w = names
                .iter()
                .map(|n| n.chars().count())
                .max()
                .unwrap_or(12)
                .clamp(12, 38)
                + 2;
            let per = (cols as usize / w).max(1);
            for chunk in names.chunks(per) {
                let line: String = chunk
                    .iter()
                    .map(|n| {
                        let t: String = n.chars().take(w - 1).collect();
                        format!("{t:<w$}")
                    })
                    .collect();
                let _ = writeln!(io.out, "{}", line.trim_end());
                if !pager.line(io) {
                    return false;
                }
            }
        }
        for (short, is_dir, size, t, long) in &rows {
            if *is_dir {
                dirs += 1;
            } else {
                files += 1;
                bytes += size;
            }
            if wide && !bare {
                continue;
            }
            if bare {
                let n = if lfn && !long.is_empty() { long } else { short };
                if a.has("S") {
                    let _ = writeln!(io.out, "{}", sh.dos.display(&dir.join(n)));
                } else {
                    let _ = writeln!(io.out, "{n}");
                }
            } else {
                let (b, e) = if short == "." || short == ".." {
                    (short.as_str(), "")
                } else {
                    split_83(short)
                };
                let field = if *is_dir {
                    format!("{:<14}", "<DIR>")
                } else {
                    format!("{:>14}", thousands(*size))
                };
                let longcol = if lfn && !long.is_empty() {
                    format!("  {long}")
                } else {
                    String::new()
                };
                let _ = writeln!(
                    io.out,
                    "{b:<8} {e:<3} {field} {}  {}{longcol}",
                    date_short(*t),
                    time_short(*t)
                );
            }
            if !pager.line(io) {
                return false;
            }
        }
        if !bare {
            let _ = writeln!(
                io.out,
                "     {files:>10} file(s) {:>14} bytes",
                thousands(bytes)
            );
            if !pager.line(io) {
                return false;
            }
        }
        if !entries.is_empty() {
            *any = true;
        }
        tot.files += files;
        tot.bytes += bytes;
        tot.dirs += dirs;
    }
    if a.has("S") {
        for e in read_entries(dir, "*", &Args::new("/A:D")) {
            if e.is_dir
                && (!e.hidden || a.has("A"))
                && !std::fs::symlink_metadata(dir.join(&e.name))
                    .map(|m| m.is_symlink())
                    .unwrap_or(false)
                && !list_dir(
                    sh,
                    &dir.join(&e.name),
                    pattern,
                    a,
                    io,
                    pager,
                    tot,
                    any,
                    false,
                )
            {
                return false;
            }
        }
    }
    true
}

fn tree(sh: &mut Shell, a: &Args, io: &mut Io) -> i32 {
    let target = a.words.first().map(String::as_str).unwrap_or(".");
    let root = match sh.dos.resolve(target) {
        Ok(p) if p.is_dir() => p,
        _ => return err(io, "Invalid path"),
    };
    let (letter, _) = sh.dos.to_dos(&root);
    let label = sh
        .dos
        .drive(letter)
        .map(|d| d.label.clone())
        .unwrap_or_default();
    let _ = writeln!(io.out, "Directory PATH listing for Volume {label}");
    if let Some(fs) = sysinfo::statvfs(&root) {
        let _ = writeln!(io.out, "Volume Serial Number is {}", fs.serial);
    }
    let _ = writeln!(io.out, "{}", sh.dos.display(&root));
    let ascii = a.has("A");
    let mut count = 0;
    walk_tree(&root, "", a.has("F"), ascii, io, &mut count);
    if count == 0 {
        let _ = writeln!(io.out, "No subdirectories exist");
    }
    0
}

fn walk_tree(
    dir: &Path,
    prefix: &str,
    files: bool,
    ascii: bool,
    io: &mut Io,
    count: &mut u32,
) -> bool {
    let (tee, ell, bar) = if ascii {
        ("+---", "\\---", "|   ")
    } else {
        ("├───", "└───", "│   ")
    };
    let mut names: Vec<(String, bool)> = retro_dosfs::list_names(dir)
        .into_iter()
        .filter(|n| !n.starts_with('.'))
        .map(|n| {
            let d = dir.join(&n);
            let is_dir = d.is_dir() && !d.is_symlink();
            (n, is_dir)
        })
        .filter(|(_, d)| *d || files)
        .collect();
    names.sort_by_key(|(n, d)| (*d, n.to_lowercase()));
    let n = names.len();
    let has_dirs = names.iter().any(|(_, d)| *d);
    for (i, (name, is_dir)) in names.into_iter().enumerate() {
        if crate::term::interrupted() {
            return false;
        }
        let last = i + 1 == n;
        if is_dir {
            *count += 1;
            let _ = writeln!(io.out, "{prefix}{}{name}", if last { ell } else { tee });
            let next = format!("{prefix}{}", if last { "    " } else { bar });
            if !walk_tree(&dir.join(&name), &next, files, ascii, io, count) {
                return false;
            }
        } else {
            let lead = if has_dirs { bar } else { "    " };
            let _ = writeln!(io.out, "{prefix}{lead}{name}");
        }
    }
    true
}

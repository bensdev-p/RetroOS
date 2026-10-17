//! CD, MD, RD, DELTREE, COPY, MOVE, DEL, REN, TYPE, MORE.

use super::{Builtin, Pager};
use crate::parse::Args;
use crate::shell::{err, os_error, Io, Shell};
use crate::term;
use retro_dosfs::{has_wildcards, split_83};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const CD: Builtin = Builtin {
    names: &["CD", "CHDIR"],
    summary: "Displays the name of or changes the current directory.",
    usage: "CHDIR [drive:][path]\nCHDIR[..]\nCD [drive:][path]\nCD[..]\n\n  ..   Specifies that you want to change to the parent directory.\n\nType CD drive: to display the current directory in the specified drive.\nType CD without parameters to display the current drive and directory.",
    run: |sh, a, io| {
        let Some(target) = a.words.first() else {
            let _ = writeln!(io.out, "{}", sh.dos.cwd_display());
            return 0;
        };
        // "CD D:" shows D's current directory.
        if target.len() == 2 && target.ends_with(':') {
            let save = sh.dos.current_drive();
            let l = target.chars().next().unwrap();
            if sh.dos.set_drive(l).is_err() {
                return err(io, "Invalid drive specification");
            }
            let _ = writeln!(io.out, "{}", sh.dos.cwd_display());
            let _ = sh.dos.set_drive(save);
            return 0;
        }
        match sh.dos.chdir(target) {
            Ok(()) => 0,
            Err(retro_dosfs::PathError::InvalidDrive(_)) => err(io, "Invalid drive specification"),
            Err(retro_dosfs::PathError::Ambiguous(n, c)) => err(io, &format!("Ambiguous name {n}: {}", c.join(", "))),
            Err(_) => err(io, "Invalid directory"),
        }
    },
};

pub const MD: Builtin = Builtin {
    names: &["MD", "MKDIR"],
    summary: "Creates a directory.",
    usage: "MKDIR [drive:]path\nMD [drive:]path\n\nIntermediate directories are created as needed.",
    run: |sh, a, io| {
        if a.words.is_empty() {
            return err(io, "Required parameter missing");
        }
        let mut code = 0;
        for w in &a.words {
            match sh.dos.resolve_new(w) {
                Ok(p) if p.exists() => {
                    let _ = writeln!(io.out, "A subdirectory or file {w} already exists.");
                    code = 1;
                }
                Ok(p) => {
                    if std::fs::create_dir_all(&p).is_err() {
                        let _ = writeln!(io.out, "Unable to create directory");
                        code = 1;
                    }
                }
                Err(e) => code = err(io, &e.to_string()),
            }
        }
        code
    },
};

pub const RD: Builtin = Builtin {
    names: &["RD", "RMDIR"],
    summary: "Removes (deletes) a directory.",
    usage: "RMDIR [/S] [/Q] [drive:]path\nRD [/S] [/Q] [drive:]path\n\n  /S   Removes all directories and files in the specified directory\n       in addition to the directory itself.\n  /Q   Quiet mode, do not ask if ok to remove a directory tree with /S.",
    run: |sh, a, io| {
        if a.words.is_empty() {
            return err(io, "Required parameter missing");
        }
        let mut code = 0;
        for w in &a.words {
            let p = match sh.dos.resolve(w) {
                Ok(p) if p.is_dir() => p,
                _ => {
                    code = err(io, "Invalid path, not directory,\nor directory not empty");
                    continue;
                }
            };
            if p == sh.dos.cwd_path() {
                code = err(io, "Attempt to remove current directory");
                continue;
            }
            if a.has("S") {
                if !a.has("Q") && term::ask(&format!("{}, Are you sure (Y/N)? ", sh.dos.display(&p)), "YN") != 'Y' {
                    continue;
                }
                if let Err(e) = std::fs::remove_dir_all(&p) {
                    code = err(io, &os_error(&e));
                }
            } else if std::fs::remove_dir(&p).is_err() {
                code = err(io, "Invalid path, not directory,\nor directory not empty");
            }
        }
        code
    },
};

pub const DELTREE: Builtin = Builtin {
    names: &["DELTREE"],
    summary: "Deletes a directory and all the subdirectories and files in it.",
    usage: "DELTREE [/Y] [drive:]path [[drive:]path[...]]\n\n  /Y   Suppresses prompting to confirm you want to delete the subdirectory.\n  path Specifies the name of the directory you want to delete.",
    run: |sh, a, io| {
        if a.words.is_empty() {
            return err(io, "Required parameter missing");
        }
        for w in &a.words {
            let targets = match sh.dos.expand(w) {
                Ok(t) => t,
                Err(e) => return err(io, &e.to_string()),
            };
            for p in targets {
                let name = sh.dos.display(&p);
                let is_dir = p.is_dir() && !p.is_symlink();
                if !a.has("Y") {
                    let q = if is_dir {
                        format!("Delete directory \"{name}\" and all its subdirectories? [yn] ")
                    } else {
                        format!("Delete file \"{name}\"? [yn] ")
                    };
                    if term::ask(&q, "YN") != 'Y' {
                        continue;
                    }
                }
                let _ = writeln!(io.out, "Deleting {name}...");
                let r = if is_dir { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
                if let Err(e) = r {
                    return err(io, &os_error(&e));
                }
            }
        }
        0
    },
};

/// Apply a DOS rename pattern ("*.BAK", "NEW?.*") to a name.
pub fn apply_pattern(pattern: &str, name: &str) -> String {
    if !has_wildcards(pattern) {
        return pattern.to_string();
    }
    let (pb, pe) = split_pat(pattern);
    let (nb, ne) = split_83(name);
    let part = |p: &str, n: &str| {
        let nc: Vec<char> = n.chars().collect();
        let mut out = String::new();
        for (i, c) in p.chars().enumerate() {
            match c {
                '*' => {
                    out.extend(nc.iter().skip(i));
                    break;
                }
                '?' => {
                    if let Some(&ch) = nc.get(i) {
                        out.push(ch);
                    }
                }
                c => out.push(c),
            }
        }
        out
    };
    let base = part(pb, nb);
    match pe {
        Some(pe) => {
            let ext = part(pe, ne);
            if ext.is_empty() {
                base
            } else {
                format!("{base}.{ext}")
            }
        }
        None if pb == "*" => name.to_string(),
        None => base,
    }
}

fn split_pat(p: &str) -> (&str, Option<&str>) {
    match p.rfind('.') {
        Some(i) => (&p[..i], Some(&p[i + 1..])),
        None => (p, None),
    }
}

pub(super) fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
        _ => false,
    }
}

/// Overwrite policy: /Y, /-Y, COPYCMD, else ask only when interactive.
pub(super) fn confirm_overwrite(a: &Args, io: &Io) -> bool {
    if a.has("Y") {
        return false;
    }
    if a.has("-Y") {
        return true;
    }
    if let Ok(c) = std::env::var("COPYCMD") {
        let c = c.to_ascii_uppercase();
        if c.contains("/-Y") {
            return true;
        }
        if c.contains("/Y") {
            return false;
        }
    }
    io.tty && term::is_tty(0)
}

pub(super) enum Answer {
    Yes,
    No,
    All,
}

pub(super) fn ask_overwrite(name: &str) -> Answer {
    match term::ask(&format!("Overwrite {name} (Yes/No/All)? "), "YNA") {
        'Y' => Answer::Yes,
        'A' => Answer::All,
        _ => Answer::No,
    }
}

pub const COPY: Builtin = Builtin {
    names: &["COPY"],
    summary: "Copies one or more files to another location.",
    usage: "COPY [/Y | /-Y] source [+ source [+ ...]] [destination] [/V]\n\n  source       Specifies the file or files to be copied.\n  destination  Specifies the directory and/or filename for the new file(s).\n  /V           Verifies that new files are written correctly.\n  /Y           Suppresses prompting to confirm you want to overwrite an\n               existing destination file.\n  /-Y          Causes prompting to confirm you want to overwrite an\n               existing destination file.\n\nTo append files, specify a single file for destination, but multiple files\nfor source (using wildcards or file1+file2+file3 format).",
    run: copy,
};

fn copy(sh: &mut Shell, a: &Args, io: &mut Io) -> i32 {
    // Rejoin "A + B" written with spaces into one source spec.
    let mut words = a.words.clone();
    let mut i = 0;
    while i + 1 < words.len() {
        if words[i].ends_with('+') || words[i + 1].starts_with('+') {
            let next = words.remove(i + 1);
            words[i].push_str(&next);
        } else {
            i += 1;
        }
    }
    let Some(src_spec) = words.first().cloned() else {
        return err(io, "Required parameter missing");
    };
    if words.len() > 2 {
        return err(io, "Too many parameters");
    }
    let concat = src_spec.contains('+');
    let mut sources: Vec<PathBuf> = Vec::new();
    for part in src_spec.split('+').filter(|s| !s.is_empty()) {
        let full = match sh.dos.resolve(part) {
            Ok(p) => p,
            Err(e) => return err(io, &e.to_string()),
        };
        let found = if full.is_dir() {
            sh.dos.expand(&format!("{part}\\*.*")).unwrap_or_default()
        } else {
            sh.dos.expand(part).unwrap_or_default()
        };
        let found: Vec<PathBuf> = found.into_iter().filter(|p| p.is_file()).collect();
        if found.is_empty() {
            let _ = writeln!(io.out, "File not found - {part}");
            let _ = writeln!(io.out, "        0 file(s) copied");
            return 1;
        }
        sources.extend(found);
    }
    let dest_spec = words.get(1).cloned();
    let dest = match &dest_spec {
        None if concat => sources[0].clone(),
        None => sh.dos.cwd_path(),
        Some(d) => match sh.dos.resolve(d) {
            Ok(p) => p,
            Err(e) => return err(io, &e.to_string()),
        },
    };
    let dest_is_dir = dest.is_dir();
    let dest_pattern = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| has_wildcards(n));
    let multi = sources.len() > 1 || has_wildcards(&src_spec);
    let ask = confirm_overwrite(a, io);
    let mut overwrite_all = !ask;

    // Many sources into one file = concatenation (DOS semantics).
    if concat || (multi && !dest_is_dir && dest_pattern.is_none()) {
        let mut buf = Vec::new();
        for s in &sources {
            if multi {
                let _ = writeln!(io.out, "{}", sh.dos.display(s));
            }
            match std::fs::read(s) {
                Ok(b) => buf.extend(b),
                Err(e) => return err(io, &os_error(&e)),
            }
        }
        if dest.exists() && !sources.iter().any(|s| same_file(s, &dest)) && !overwrite_all {
            if let Answer::No = ask_overwrite(&sh.dos.display(&dest)) {
                let _ = writeln!(io.out, "        0 file(s) copied");
                return 0;
            }
        }
        if let Err(e) = std::fs::write(&dest, &buf) {
            return err(io, &os_error(&e));
        }
        let _ = writeln!(io.out, "        1 file(s) copied");
        return 0;
    }

    let mut copied = 0;
    for s in &sources {
        if term::interrupted() {
            break;
        }
        let name = s.file_name().unwrap().to_string_lossy().into_owned();
        let target = if dest_is_dir {
            dest.join(&name)
        } else if let Some(p) = &dest_pattern {
            dest.parent()
                .unwrap_or(Path::new("/"))
                .join(apply_pattern(p, &name))
        } else {
            dest.clone()
        };
        if same_file(s, &target) {
            let _ = writeln!(io.out, "File cannot be copied onto itself");
            continue;
        }
        if multi {
            let _ = writeln!(io.out, "{}", sh.dos.display(s));
        }
        if target.exists() && !overwrite_all {
            match ask_overwrite(&sh.dos.display(&target)) {
                Answer::No => continue,
                Answer::All => overwrite_all = true,
                Answer::Yes => {}
            }
        }
        match std::fs::copy(s, &target) {
            Ok(_) => {
                if a.has("V") && std::fs::read(s).ok() != std::fs::read(&target).ok() {
                    let _ = writeln!(io.out, "Verify error");
                    continue;
                }
                copied += 1;
            }
            Err(e) => {
                let _ = writeln!(io.out, "{}", os_error(&e));
                break;
            }
        }
    }
    let _ = writeln!(io.out, "        {copied} file(s) copied");
    if copied == 0 && !sources.is_empty() {
        1
    } else {
        0
    }
}

pub const MOVE: Builtin = Builtin {
    names: &["MOVE"],
    summary: "Moves files and renames files and directories.",
    usage: "To move one or more files:\nMOVE [/Y | /-Y] [drive:][path]filename1[,...] destination\n\nTo rename a directory:\nMOVE [/Y | /-Y] [drive:][path]dirname1 dirname2\n\n  /Y   Suppresses prompting to confirm overwriting of the destination.\n  /-Y  Causes prompting to confirm overwriting of the destination.",
    run: |sh, a, io| {
        if a.words.len() < 2 {
            return err(io, "Required parameter missing");
        }
        let dest_spec = a.words.last().unwrap();
        let dest = match sh.dos.resolve(dest_spec) {
            Ok(p) => p,
            Err(e) => return err(io, &e.to_string()),
        };
        let mut sources = Vec::new();
        for w in &a.words[..a.words.len() - 1] {
            for part in w.split(',').filter(|s| !s.is_empty()) {
                match sh.dos.expand(part) {
                    Ok(v) if !v.is_empty() => sources.extend(v),
                    _ => {
                        let _ = writeln!(io.out, "File not found - {part}");
                        return 1;
                    }
                }
            }
        }
        if sources.len() > 1 && !dest.is_dir() {
            return err(io, "Cannot move multiple files to a single file");
        }
        let ask = confirm_overwrite(a, io);
        let mut all = !ask;
        let mut code = 0;
        for s in sources {
            let target = if dest.is_dir() { dest.join(s.file_name().unwrap()) } else { dest.clone() };
            if target.exists() && !all && !s.is_dir() {
                match ask_overwrite(&sh.dos.display(&target)) {
                    Answer::No => continue,
                    Answer::All => all = true,
                    Answer::Yes => {}
                }
            }
            let r = std::fs::rename(&s, &target).or_else(|e| {
                // Across filesystems: copy, then delete.
                if e.raw_os_error() == Some(libc::EXDEV) && s.is_file() {
                    std::fs::copy(&s, &target).and_then(|_| std::fs::remove_file(&s))
                } else {
                    Err(e)
                }
            });
            let ok = r.is_ok();
            let _ = writeln!(
                io.out,
                "{} => {} [{}]",
                sh.dos.display(&s),
                sh.dos.display(&target),
                if ok { "ok" } else { "failed" }
            );
            if let Err(e) = r {
                let _ = writeln!(io.out, "{}", os_error(&e));
                code = 1;
            }
        }
        code
    },
};

pub const DEL: Builtin = Builtin {
    names: &["DEL", "ERASE"],
    summary: "Deletes one or more files.",
    usage: "DEL [drive:][path]filename [/P] [/Q]\nERASE [drive:][path]filename [/P] [/Q]\n\n  [drive:][path]filename  Specifies the file(s) to delete.  Specify multiple\n                          files by using wildcards.\n  /P   Prompts for confirmation before deleting each file.\n  /Q   Quiet mode, do not ask if ok to delete on global wildcard.",
    run: |sh, a, io| {
        if a.words.is_empty() {
            return err(io, "Required parameter missing");
        }
        let mut deleted = 0;
        let mut code = 0;
        for w in &a.words {
            let full = match sh.dos.resolve(w) {
                Ok(p) => p,
                Err(e) => return err(io, &e.to_string()),
            };
            let (spec, whole_dir) = if full.is_dir() {
                (format!("{w}\\*.*"), true)
            } else {
                let n = full.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                (w.clone(), n == "*.*" || n == "*")
            };
            if whole_dir && !a.has("Q") && !a.has("P") {
                let _ = writeln!(io.out, "All files in directory will be deleted!");
                if term::ask("Are you sure (Y/N)?", "YN") != 'Y' {
                    continue;
                }
            }
            let files: Vec<PathBuf> =
                sh.dos.expand(&spec).unwrap_or_default().into_iter().filter(|p| !p.is_dir() || p.is_symlink()).collect();
            if files.is_empty() {
                code = err(io, "File not found");
                continue;
            }
            for f in files {
                if a.has("P") && term::ask(&format!("{},    Delete (Y/N)?", sh.dos.display(&f)), "YN") != 'Y' {
                    continue;
                }
                match std::fs::remove_file(&f) {
                    Ok(()) => deleted += 1,
                    Err(e) => code = err(io, &format!("{} - {}", os_error(&e), sh.dos.display(&f))),
                }
            }
        }
        let _ = deleted;
        code
    },
};

pub const REN: Builtin = Builtin {
    names: &["REN", "RENAME"],
    summary: "Renames a file or files.",
    usage: "RENAME [drive:][path]filename1 filename2\nREN [drive:][path]filename1 filename2\n\nNote that you cannot specify a new drive or path for your destination file.",
    run: |sh, a, io| {
        if a.words.len() < 2 {
            return err(io, "Required parameter missing");
        }
        let newname = &a.words[1];
        if newname.contains(['\\', '/', ':']) {
            return err(io, "Invalid parameter");
        }
        let sources = sh.dos.expand(&a.words[0]).unwrap_or_default();
        if sources.is_empty() {
            return err(io, "Duplicate file name or file not found");
        }
        let mut code = 0;
        for s in sources {
            let name = s.file_name().unwrap().to_string_lossy().into_owned();
            let target = s.parent().unwrap_or(Path::new("/")).join(apply_pattern(newname, &name));
            // Allow case-only renames ("REN readme.txt README.TXT").
            let case_only = target.to_string_lossy().to_lowercase() == s.to_string_lossy().to_lowercase();
            if target.exists() && !case_only {
                code = err(io, "Duplicate file name or file not found");
                continue;
            }
            if let Err(e) = std::fs::rename(&s, &target) {
                code = err(io, &os_error(&e));
            }
        }
        code
    },
};

pub const TYPE: Builtin = Builtin {
    names: &["TYPE"],
    summary: "Displays the contents of a text file or files.",
    usage: "TYPE [drive:][path]filename",
    run: |sh, a, io| {
        if a.words.is_empty() {
            return err(io, "Required parameter missing");
        }
        let mut code = 0;
        for w in &a.words {
            let files = sh.dos.expand(w).unwrap_or_default();
            if files.is_empty() {
                code = err(io, &format!("File not found - {w}"));
                continue;
            }
            for f in files {
                if f.is_dir() {
                    code = err(io, "Access denied");
                    continue;
                }
                if a.words.len() > 1 || has_wildcards(w) {
                    let _ = writeln!(io.out, "\n{}\n", sh.dos.display(&f));
                }
                match std::fs::File::open(&f) {
                    Ok(mut file) => {
                        let mut buf = [0u8; 16384];
                        loop {
                            if term::interrupted() {
                                break;
                            }
                            match file.read(&mut buf) {
                                Ok(0) => break,
                                // ^Z ends a text file, as it always has.
                                Ok(n) => {
                                    let chunk = &buf[..n];
                                    if let Some(z) = chunk.iter().position(|&b| b == 0x1A) {
                                        let _ = io.out.write_all(&chunk[..z]);
                                        break;
                                    }
                                    if io.out.write_all(chunk).is_err() {
                                        break;
                                    }
                                }
                                Err(_) => break,
                            }
                        }
                    }
                    Err(e) => code = err(io, &os_error(&e)),
                }
            }
        }
        code
    },
};

pub const MORE: Builtin = Builtin {
    names: &["MORE"],
    summary: "Displays output one screen at a time.",
    usage: "MORE [drive:][path]filename\nMORE < [drive:][path]filename\ncommand-name | MORE [drive:][path][filename]\n\nWhen the -- More -- prompt appears: SPACE shows the next page, ENTER the\nnext line, Q quits.",
    run: |sh, a, io| {
        let data = if let Some(w) = a.words.first() {
            match sh.dos.resolve(w).ok().and_then(|p| std::fs::read(p).ok()) {
                Some(d) => d,
                None => return err(io, &format!("Cannot access file {w}")),
            }
        } else {
            match io.read_input() {
                Some(d) => d,
                None => return 0,
            }
        };
        let text = String::from_utf8_lossy(&data);
        if !io.tty {
            let _ = io.out.write_all(text.as_bytes());
            return 0;
        }
        let rows = term::size().1.max(4) as usize;
        let mut shown = 0usize;
        let mut page = rows - 1;
        for line in text.lines() {
            let _ = writeln!(io.out, "{line}");
            shown += 1;
            if shown >= page {
                let _ = io.out.flush();
                print!("-- More --");
                let _ = std::io::stdout().flush();
                let key = {
                    let _raw = term::RawMode::enable();
                    term::read_byte()
                };
                print!("\r\x1b[K");
                match key {
                    Some(b'q') | Some(b'Q') | Some(3) | None => return 0,
                    Some(b'\r') | Some(b'\n') => page = shown + 1,
                    _ => page = shown + rows - 1,
                }
            }
        }
        let _ = Pager::new(false);
        0
    },
};

#[cfg(test)]
mod tests {
    use super::apply_pattern;

    #[test]
    fn rename_patterns() {
        assert_eq!(apply_pattern("*.BAK", "notes.txt"), "notes.BAK");
        assert_eq!(apply_pattern("NEW.*", "old.doc"), "NEW.doc");
        assert_eq!(apply_pattern("??X*.*", "abcdef.txt"), "abXdef.txt");
        assert_eq!(apply_pattern("*", "Makefile"), "Makefile");
        assert_eq!(apply_pattern("plain.txt", "x.y"), "plain.txt");
    }
}

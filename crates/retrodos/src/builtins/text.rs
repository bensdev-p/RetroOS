//! FIND and SORT — the classic DOS text filters.

use super::Builtin;
use crate::shell::err;
use std::io::Write;

pub const FIND: Builtin = Builtin {
    names: &["FIND"],
    summary: "Searches for a text string in a file or files.",
    usage: "FIND [/V] [/C] [/N] [/I] \"string\" [[drive:][path]filename[ ...]]\n\n  /V        Displays all lines NOT containing the specified string.\n  /C        Displays only the count of lines containing the string.\n  /N        Displays line numbers with the displayed lines.\n  /I        Ignores the case of characters when searching for the string.\n  \"string\"  Specifies the text string to find.\n\nFor the POSIX find program, type UNIX find.",
    run: |sh, a, io| {
        let Some(needle) = a.words.first().cloned() else {
            return err(io, "FIND: Parameter format not correct");
        };
        let ci = a.has("I");
        let matches = |line: &str| {
            let hit = if ci { line.to_lowercase().contains(&needle.to_lowercase()) } else { line.contains(&needle) };
            hit != a.has("V")
        };
        let mut found_any = false;
        let mut run = |name: Option<String>, data: Vec<u8>, out: &mut dyn Write| {
            let text = String::from_utf8_lossy(&data);
            let hits: Vec<(usize, &str)> = text.lines().enumerate().filter(|(_, l)| matches(l)).collect();
            found_any |= !hits.is_empty();
            if a.has("C") {
                match &name {
                    Some(n) => {
                        let _ = writeln!(out, "---------- {n}: {}", hits.len());
                    }
                    None => {
                        let _ = writeln!(out, "{}", hits.len());
                    }
                }
                return;
            }
            if let Some(n) = &name {
                let _ = writeln!(out, "\n---------- {n}");
            }
            for (i, l) in hits {
                if a.has("N") {
                    let _ = writeln!(out, "[{}]{l}", i + 1);
                } else {
                    let _ = writeln!(out, "{l}");
                }
            }
        };
        if a.words.len() > 1 {
            for w in &a.words[1..] {
                let files = sh.dos.expand(w).unwrap_or_default();
                if files.is_empty() {
                    let _ = writeln!(io.out, "File not found - {w}");
                    continue;
                }
                for f in files {
                    let data = std::fs::read(&f).unwrap_or_default();
                    run(Some(sh.dos.display(&f).to_uppercase()), data, io.out);
                }
            }
        } else if let Some(data) = io.read_input() {
            run(None, data, io.out);
        } else {
            return err(io, "FIND: no input. Use FIND \"text\" file, or command | FIND \"text\"");
        }
        if found_any {
            0
        } else {
            1
        }
    },
};

pub const SORT: Builtin = Builtin {
    names: &["SORT"],
    summary: "Reads input, sorts data, and writes the results to the screen, a file,\nor another device.",
    usage: "SORT [/R] [/+n] [[drive:][path]filename]\n  command-name | SORT [/R] [/+n]\n\n  /R   Reverses the sort order; that is, sorts Z to A, then 9 to 0.\n  /+n  Sorts the file according to characters in column n.",
    run: |sh, a, io| {
        let data = if let Some(w) = a.words.first() {
            match sh.dos.resolve(w).ok().and_then(|p| std::fs::read(p).ok()) {
                Some(d) => d,
                None => return err(io, &format!("File not found - {w}")),
            }
        } else {
            match io.read_input() {
                Some(d) => d,
                None => return err(io, "SORT: no input. Use SORT file, or command | SORT"),
            }
        };
        let col = a.switches.iter().find_map(|s| s.strip_prefix('+').and_then(|n| n.parse::<usize>().ok())).unwrap_or(1);
        let text = String::from_utf8_lossy(&data);
        let mut lines: Vec<&str> = text.lines().collect();
        let key = |l: &str| l.chars().skip(col.saturating_sub(1)).collect::<String>().to_lowercase();
        lines.sort_by_key(|l| key(l));
        if a.has("R") {
            lines.reverse();
        }
        for l in lines {
            let _ = writeln!(io.out, "{l}");
        }
        0
    },
};

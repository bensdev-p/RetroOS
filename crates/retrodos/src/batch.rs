//! Batch files: running .BAT programs, parameters, labels.
//!
//! Semantics follow MS-DOS 6 with a few NT conveniences (EXIT /B, IF /I,
//! IF DEFINED, `::` comments). Running a batch file from another one
//! without CALL also returns to the caller (as if CALLed) — simpler to
//! reason about and what most people expect.

use crate::shell::Shell;
use crate::term;
use std::path::{Path, PathBuf};

pub struct Frame {
    lines: Vec<String>,
    pc: usize,
    /// %0, %1, …
    pub args: Vec<String>,
    pub goto: Option<String>,
    pub exit: bool,
}

impl Shell {
    /// Find `NAME` or `NAME.BAT` as a batch file in the current directory, then PATH.
    pub fn find_batch(&self, cmd: &str) -> Option<PathBuf> {
        let has_ext = cmd.to_ascii_lowercase().ends_with(".bat");
        let want = if has_ext {
            cmd.to_string()
        } else {
            format!("{cmd}.bat")
        };
        if cmd.contains(['\\', '/']) || retro_dosfs::split_drive(cmd).0.is_some() {
            let p = self.dos.resolve(&want).ok()?;
            return p.is_file().then_some(p);
        }
        let mut dirs = vec![self.dos.cwd_path()];
        if let Some(path) = self.getenv("PATH") {
            dirs.extend(path.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
        }
        for d in dirs {
            if let Ok(Some(real)) = retro_dosfs::find_entry(&d, &want) {
                let p = d.join(real);
                if p.is_file() {
                    return Some(p);
                }
            }
        }
        None
    }

    /// Run a batch file to completion. Returns the final ERRORLEVEL.
    pub fn run_batch(&mut self, path: &Path, args: Vec<String>) -> i32 {
        let text = match std::fs::read(path) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(_) => {
                println!("Batch file missing");
                return 1;
            }
        };
        if self.batch.len() >= 32 {
            println!("Batch files nested too deeply");
            return 1;
        }
        let lines = text
            .lines()
            .map(|l| l.trim_end_matches(['\r', '\x1a']).to_string())
            .collect();
        let mut all_args = vec![self.dos.display(path)];
        all_args.extend(args);
        let saved_echo = self.echo;
        self.batch.push(Frame {
            lines,
            pc: 0,
            args: all_args,
            goto: None,
            exit: false,
        });
        while let Some(f) = self.batch.last_mut() {
            if f.exit || f.pc >= f.lines.len() || self.exit {
                break;
            }
            let raw = f.lines[f.pc].clone();
            f.pc += 1;
            let line = raw.trim_start();
            if line.is_empty() || line.starts_with(':') {
                continue; // labels and :: comments
            }
            let (quiet, line) = match line.strip_prefix('@') {
                Some(rest) => (true, rest.trim_start()),
                None => (false, line),
            };
            let line = self.substitute_params(line);
            if self.echo && !quiet {
                println!("{}{line}", self.prompt());
            }
            term::take_break();
            self.execute(&line);
            if term::take_break() {
                term::clear_interrupt();
                if term::ask("Terminate batch job (Y/N)? ", "YN") == 'Y' {
                    for f in self.batch.iter_mut() {
                        f.exit = true;
                    }
                }
            }
            if let Some(f) = self.batch.last_mut() {
                if let Some(label) = f.goto.take() {
                    if label.eq_ignore_ascii_case("eof") {
                        f.exit = true;
                    } else if let Some(i) = find_label(&f.lines, &label) {
                        f.pc = i + 1;
                    } else {
                        println!("Label not found");
                        f.exit = true;
                    }
                }
            }
        }
        self.batch.pop();
        self.echo = saved_echo;
        self.errorlevel
    }

    /// Replace %0–%9 and %* with the current batch file's parameters.
    /// `%%` is left alone (the variable pass turns it into `%`).
    pub fn substitute_params(&self, line: &str) -> String {
        let Some(f) = self.batch.last() else {
            return line.to_string();
        };
        let mut out = String::new();
        let mut it = line.chars().peekable();
        while let Some(c) = it.next() {
            if c != '%' {
                out.push(c);
                continue;
            }
            match it.peek().copied() {
                Some('%') => {
                    out.push_str("%%");
                    it.next();
                }
                Some(d @ '0'..='9') => {
                    it.next();
                    out.push_str(
                        f.args
                            .get(d as usize - '0' as usize)
                            .map(String::as_str)
                            .unwrap_or(""),
                    );
                }
                Some('*') => {
                    it.next();
                    out.push_str(&f.args.iter().skip(1).cloned().collect::<Vec<_>>().join(" "));
                }
                _ => out.push('%'),
            }
        }
        out
    }
}

fn find_label(lines: &[String], label: &str) -> Option<usize> {
    let want = label.trim_start_matches(':').trim();
    lines.iter().position(|l| {
        let l = l.trim_start();
        l.starts_with(':')
            && !l.starts_with("::")
            && l[1..]
                .split_whitespace()
                .next()
                .is_some_and(|n| n.eq_ignore_ascii_case(want))
    })
}

/// Split the first word off `s`, honouring quotes; returns (word as written, rest).
pub fn take_word(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    let mut quoted = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => return (&s[..i], &s[i..]),
            _ => {}
        }
    }
    (s, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_words() {
        let lines: Vec<String> = ["@ECHO OFF", ":: comment", ":Start", "GOTO end", ":END rest"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(find_label(&lines, "start"), Some(2));
        assert_eq!(find_label(&lines, ":end"), Some(4));
        assert_eq!(find_label(&lines, "comment"), None);
        assert_eq!(take_word("  \"a b\" c d"), ("\"a b\"", " c d"));
    }
}

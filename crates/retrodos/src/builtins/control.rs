//! IF, FOR, GOTO, CALL, SHIFT, CHOICE — batch-file control flow.

use super::Builtin;
use crate::batch::take_word;
use crate::parse::Args;
use crate::shell::{err, Io, Shell};
use crate::term;
use std::io::Write;
use std::time::{Duration, Instant};

fn unquote(s: &str) -> &str {
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(s)
}

pub const IF: Builtin = Builtin {
    names: &["IF"],
    summary: "Performs conditional processing in batch programs.",
    usage: "IF [/I] [NOT] ERRORLEVEL number command\nIF [/I] [NOT] string1==string2 command\nIF [NOT] EXIST filename command\nIF [NOT] DEFINED variable command\n\n  NOT               Carry out the command only if the condition is false.\n  ERRORLEVEL number True if the last program returned an exit code equal to\n                    or greater than the number specified.\n  string1==string2  True if the strings match (/I ignores case).\n  EXIST filename    True if the file or directory exists.\n  DEFINED variable  True if the environment variable is set.",
    run: |sh, a, io| {
        let mut rest = a.raw.as_str();
        let mut ignore_case = false;
        let mut negate = false;
        loop {
            let (w, r) = take_word(rest);
            match w.to_ascii_uppercase().as_str() {
                "/I" => ignore_case = true,
                "NOT" => negate = !negate,
                _ => break,
            }
            rest = r;
        }
        let (w, r) = take_word(rest);
        let up = w.to_ascii_uppercase();
        let (cond, command) = if up == "ERRORLEVEL" {
            let (n, r2) = take_word(r);
            let Ok(n) = n.parse::<i32>() else { return err(io, "Syntax error") };
            (sh.errorlevel >= n, r2)
        } else if up == "EXIST" {
            let (f, r2) = take_word(r);
            (sh.dos.expand(unquote(f)).map(|v| !v.is_empty()).unwrap_or(false), r2)
        } else if up == "DEFINED" {
            let (v, r2) = take_word(r);
            (sh.getenv(v).is_some_and(|x| !x.is_empty()), r2)
        } else if let Some(i) = rest.find("==") {
            let left = rest[..i].trim();
            let (right, r2) = take_word(&rest[i + 2..]);
            let (l, r_) = (unquote(left), unquote(right));
            let eq = if ignore_case { l.eq_ignore_ascii_case(r_) } else { l == r_ };
            (eq, r2)
        } else {
            return err(io, "Syntax error");
        };
        let command = command.trim();
        if command.is_empty() {
            return err(io, "Syntax error");
        }
        if cond != negate {
            let _ = io.out.flush();
            sh.execute(command);
            sh.errorlevel
        } else {
            sh.errorlevel
        }
    },
};

pub const FOR: Builtin = Builtin {
    names: &["FOR"],
    summary: "Runs a specified command for each file in a set of files.",
    usage: "FOR %variable IN (set) DO command [command-parameters]\n\n  %variable  Specifies a replaceable parameter (use %%variable in a batch file).\n  (set)      Specifies a set of one or more files.  Wildcards may be used.\n  command    Specifies the command to carry out for each file.\n\nExample: FOR %F IN (*.TXT) DO TYPE %F",
    run: |sh, a, io| {
        let (var, rest) = take_word(&a.raw);
        if !var.starts_with('%') || var.len() < 2 {
            return err(io, "Syntax error");
        }
        let (kw, rest) = take_word(rest);
        if !kw.eq_ignore_ascii_case("IN") {
            return err(io, "Syntax error");
        }
        let rest = rest.trim_start();
        let (Some(open), Some(close)) = (rest.find('('), rest.find(')')) else {
            return err(io, "Syntax error");
        };
        if open != 0 || close < open {
            return err(io, "Syntax error");
        }
        let set = &rest[1..close];
        let (kw, command) = take_word(&rest[close + 1..]);
        if !kw.eq_ignore_ascii_case("DO") || command.trim().is_empty() {
            return err(io, "Syntax error");
        }
        let command = command.trim();
        let mut items: Vec<String> = Vec::new();
        for it in set.split([' ', ',', ';', '\t']).filter(|s| !s.is_empty()) {
            if retro_dosfs::has_wildcards(it) {
                // Wildcards expand to the names as typed (directory part kept).
                let prefix = it.rfind(['\\', '/']).map(|i| &it[..=i]).unwrap_or("");
                for p in sh.dos.expand(it).unwrap_or_default() {
                    if p.is_file() {
                        items.push(format!("{prefix}{}", p.file_name().unwrap().to_string_lossy()));
                    }
                }
            } else {
                items.push(it.to_string());
            }
        }
        let _ = io.out.flush();
        for item in items {
            if term::interrupted() || sh.exit {
                break;
            }
            sh.execute(&command.replace(var, &item));
        }
        sh.errorlevel
    },
};

pub const GOTO: Builtin = Builtin {
    names: &["GOTO"],
    summary: "Directs RetroDOS to a labelled line in a batch program.",
    usage: "GOTO label\n\n  label   Specifies a text string used in the batch program as a label.\n\nYou type a label on a line by itself, beginning with a colon.\nGOTO :EOF ends the batch program.",
    run: |sh, a, io| {
        let Some(label) = a.words.first() else { return err(io, "Required parameter missing") };
        if let Some(f) = sh.batch.last_mut() {
            f.goto = Some(label.trim_start_matches(':').to_string());
        }
        sh.errorlevel
    },
};

pub const CALL: Builtin = Builtin {
    names: &["CALL"],
    summary: "Calls one batch program from another.",
    usage: "CALL [drive:][path]filename [batch-parameters]\n\n  batch-parameters   Specifies any command-line information required by\n                     the batch program.",
    run: |sh, a, io| {
        let (name, rest) = take_word(&a.raw);
        if name.is_empty() {
            return err(io, "Required parameter missing");
        }
        let _ = io.out.flush();
        match sh.find_batch(unquote(name)) {
            Some(p) => {
                let args = crate::parse::words(rest);
                sh.run_batch(&p, args)
            }
            // CALL of a program just runs it.
            None => {
                sh.execute(a.raw.trim());
                sh.errorlevel
            }
        }
    },
};

pub const SHIFT: Builtin = Builtin {
    names: &["SHIFT"],
    summary: "Changes the position of replaceable parameters in a batch file.",
    usage: "SHIFT\n\nAfter SHIFT, %1 becomes %0, %2 becomes %1, and so on.",
    run: |sh, _, _| {
        if let Some(f) = sh.batch.last_mut() {
            if !f.args.is_empty() {
                f.args.remove(0);
            }
        }
        sh.errorlevel
    },
};

pub const CHOICE: Builtin = Builtin {
    names: &["CHOICE"],
    summary: "Waits for the user to choose one of a set of choices.",
    usage: "CHOICE [/C[:]choices] [/N] [/S] [/T[:]c,nn] [text]\n\n  /C[:]choices  Specifies allowable keys. Default is YN.\n  /N            Do not display choices and ? at end of prompt string.\n  /S            Treat choice keys as case sensitive.\n  /T[:]c,nn     Default choice to c after nn seconds.\n  text          Prompt string to display.\n\nERRORLEVEL is set to offset of key user presses in choices.",
    run: choice,
};

fn choice(_sh: &mut Shell, a: &Args, io: &mut Io) -> i32 {
    let keys: Vec<char> = a
        .value("C")
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "YN".into())
        .chars()
        .collect();
    let case = a.has("S");
    let timeout = a.value("T").and_then(|t| {
        let (c, n) = t.split_once(',')?;
        Some((c.chars().next()?, n.trim().parse::<u64>().ok()?))
    });
    let text = a.words.join(" ");
    let shown: String = keys
        .iter()
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let prompt = if a.has("N") {
        text
    } else {
        format!("{text}[{shown}]?")
    };
    let _ = write!(io.out, "{prompt}");
    let _ = io.out.flush();
    let norm = |c: char| if case { c } else { c.to_ascii_uppercase() };
    let pos = |c: char| keys.iter().position(|k| norm(*k) == norm(c));
    let start = Instant::now();
    let piped = io.read_input();
    let picked = if let Some(data) = piped {
        String::from_utf8_lossy(&data)
            .trim()
            .chars()
            .next()
            .and_then(pos)
            .or_else(|| timeout.and_then(|(c, _)| pos(c)))
    } else if term::is_tty(0) {
        let _raw = term::RawMode::enable();
        loop {
            let wait = match timeout {
                Some((_, secs)) => {
                    let left = Duration::from_secs(secs).saturating_sub(start.elapsed());
                    if left.is_zero() {
                        break timeout.and_then(|(c, _)| pos(c));
                    }
                    left.as_millis().min(250) as i32
                }
                None => 250,
            };
            if !term::poll_input(wait) {
                continue;
            }
            match term::read_byte() {
                Some(3) | None => break None,
                Some(b) => match pos(b as char) {
                    Some(i) => break Some(i),
                    None => {
                        print!("\x07");
                        let _ = std::io::stdout().flush();
                    }
                },
            }
        }
    } else {
        let mut s = String::new();
        let _ = std::io::stdin().read_line(&mut s);
        s.trim()
            .chars()
            .next()
            .and_then(pos)
            .or_else(|| timeout.and_then(|(c, _)| pos(c)))
    };
    match picked {
        Some(i) => {
            let _ = write!(io.out, "{}\r\n", keys[i]);
            i as i32 + 1
        }
        None => {
            let _ = write!(io.out, "\r\n");
            0
        }
    }
}

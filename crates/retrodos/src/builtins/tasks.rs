//! RUN, START, TASKS, KILL, DESKTOP — programs, the Station task swapper,
//! and the switch to the RetroOS Desktop.

use super::Builtin;
use crate::batch::take_word;
use crate::launch::{Broker, Mode};
use crate::shell::err;

const NO_DISPLAY: &str = "Task switching needs the RetroOS display, which is not available here.";

pub const RUN: Builtin = Builtin {
    names: &["RUN"],
    summary: "Runs a program and waits for it to finish.",
    usage: "RUN [/TEXT | /GUI] program [parameters]\n\n  /TEXT   Run the program in this window, even if it looks graphical.\n  /GUI    Run the program full-screen on the display.\n\nGraphical programs (FIREFOX, CHROME, ...) are recognised automatically, so\nRUN FIREFOX and FIREFOX do the same thing. Ctrl+Alt+Tab switches between\nthe program and RetroDOS.",
    run: |sh, a, io| {
        let mut rest = a.raw.as_str();
        let mut mode = Mode::Auto;
        loop {
            let (w, r) = take_word(rest);
            match w.to_ascii_uppercase().as_str() {
                "/TEXT" => mode = Mode::Text,
                "/GUI" => mode = Mode::Gui,
                _ => break,
            }
            rest = r;
        }
        let (cmd, args) = take_word(rest);
        if cmd.is_empty() {
            return err(io, "Required parameter missing");
        }
        let _ = io.out.flush();
        sh.run_target(cmd.trim_matches('"'), args, mode, true, None, None)
    },
};

pub const START: Builtin = Builtin {
    names: &["START"],
    summary: "Starts a graphical program without waiting for it.",
    usage: "START program [parameters]\n\nThe program keeps running while you use RetroDOS. Ctrl+Alt+Tab switches to\nit; TASKS lists running programs.",
    run: |sh, a, io| {
        let (cmd, args) = take_word(&a.raw);
        if cmd.is_empty() {
            return err(io, "Required parameter missing");
        }
        let _ = io.out.flush();
        sh.run_target(cmd.trim_matches('"'), args, Mode::Gui, false, None, None)
    },
};

pub const TASKS: Builtin = Builtin {
    names: &["TASKS"],
    summary: "Lists running programs, or switches to one.",
    usage: "TASKS [number | /W]\n\n  number   Switch to that task.\n  /W       List windows: position, size and state, top window first.\n\nCtrl+Alt+Tab also switches between tasks.",
    run: |_, a, io| {
        let Some(mut b) = Broker::connect() else { return err(io, NO_DISPLAY) };
        if a.has("W") {
            let wins = match b.windows() {
                Ok(w) => w,
                Err(e) => return err(io, &e),
            };
            let _ = writeln!(io.out, "\n Task  Position     Size         State       Title");
            let _ = writeln!(io.out, " ----  -----------  -----------  ----------  {}", "-".repeat(20));
            for w in &wins {
                let pos = format!("{},{}", w[1], w[2]);
                let size = format!("{}x{}", w[3], w[4]);
                let _ = writeln!(io.out, " {:>4}  {:<11}  {:<11}  {:<10}  {}", w[0], pos, size, w[5], clip(&w[6..].join(" "), 20));
            }
            let _ = writeln!(io.out, "\n {} window(s)", wins.len());
            return 0;
        }
        if let Some(n) = a.words.first() {
            let Ok(id) = n.parse::<u64>() else { return err(io, "Invalid task number") };
            return match b.request(&["SWITCH", &id.to_string()]) {
                Ok(_) => 0,
                Err(_) => err(io, "Invalid task number"),
            };
        }
        let tasks = match b.tasks() {
            Ok(t) => t,
            Err(e) => return err(io, &e),
        };
        let me = std::process::id();
        let _ = writeln!(io.out, "\n Task  Program       Title");
        let _ = writeln!(io.out, " ----  ------------  {}", "-".repeat(44));
        for t in &tasks {
            let name = if t.console { "RETRODOS" } else { t.name.as_str() };
            let here = if t.console && is_ancestor(t.pid, me) { "  (this session)" } else { "" };
            let mark = if t.focused { '*' } else { ' ' };
            let _ = writeln!(io.out, " {:>4}{mark} {:<12}  {}{here}", t.id, clip(name, 12), clip(&t.title, 44));
        }
        let _ = writeln!(io.out, "\n {} task(s)", tasks.len());
        0
    },
};

pub const KILL: Builtin = Builtin {
    names: &["KILL"],
    summary: "Ends a running program.",
    usage: "KILL task [/F]\n\n  task   Task number (see TASKS) or part of the program's title.\n  /F     Force: end the program at once, without letting it save.",
    run: |_, a, io| {
        let Some(want) = a.words.first() else { return err(io, "Required parameter missing") };
        let Some(mut b) = Broker::connect() else { return err(io, NO_DISPLAY) };
        let tasks = match b.tasks() {
            Ok(t) => t,
            Err(e) => return err(io, &e),
        };
        let lw = want.to_lowercase();
        let found: Vec<_> = tasks
            .iter()
            .filter(|t| match want.parse::<u64>() {
                Ok(id) => t.id == id,
                Err(_) => !t.console && (t.name.eq_ignore_ascii_case(want) || t.title.to_lowercase().contains(&lw)),
            })
            .collect();
        let t = match found.as_slice() {
            [t] => t,
            [] => return err(io, "No such task"),
            _ => return err(io, "More than one task matches; use the task number"),
        };
        if t.console && is_ancestor(t.pid, std::process::id()) {
            return err(io, "Use EXIT to end this session");
        }
        let id = t.id.to_string();
        let mut req = vec!["END", id.as_str()];
        if a.has("F") {
            req.push("FORCE");
        }
        match b.request(&req) {
            Ok(_) => 0,
            Err(e) => err(io, &e),
        }
    },
};

pub const DESKTOP: Builtin = Builtin {
    names: &["DESKTOP", "WIN"],
    summary: "Starts the RetroOS Desktop.",
    usage: "DESKTOP\n\nSwitches the screen to the RetroOS Desktop. Nothing is closed: running\nprograms become windows and RetroDOS becomes a DOS Prompt window.\nCtrl+Alt+F1 (Exit to DOS) comes back here.",
    run: |_, _, io| {
        let Some(mut b) = Broker::connect() else {
            return err(io, "The Desktop needs the RetroOS display, which is not available here.");
        };
        match b.request(&["PERSONALITY"]) {
            Ok(r) if r.first().is_some_and(|p| p == "desktop") => {
                let _ = writeln!(io.out, "The RetroOS Desktop is already running.");
                return 0;
            }
            Ok(_) => {}
            Err(e) => return err(io, &e),
        }
        match b.request(&["PERSONALITY", "desktop"]) {
            Ok(_) => 0,
            Err(e) => err(io, &e),
        }
    },
};

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(n - 3).collect::<String>())
    }
}

/// Is `pid` this process or one of its ancestors?
fn is_ancestor(pid: u32, mut me: u32) -> bool {
    for _ in 0..32 {
        if me == pid {
            return true;
        }
        let Ok(s) = std::fs::read_to_string(format!("/proc/{me}/stat")) else {
            return false;
        };
        let Some(i) = s.rfind(')') else { return false };
        match s[i + 2..]
            .split_whitespace()
            .nth(1)
            .and_then(|p| p.parse().ok())
        {
            Some(p) if p > 1 => me = p,
            _ => return false,
        }
    }
    false
}

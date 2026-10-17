//! Built-in (internal) commands.

mod config;
mod control;
mod disk;
mod env;
mod files;
mod listing;
mod programs;
mod sys;
mod tasks;
mod text;

use crate::parse::Args;
use crate::shell::{Io, Shell};

pub struct Builtin {
    /// Primary name first, then aliases.
    pub names: &'static [&'static str],
    pub summary: &'static str,
    /// Syntax and parameter table, shown by `/?` and HELP.
    pub usage: &'static str,
    pub run: fn(&mut Shell, &Args, &mut Io) -> i32,
}

pub fn all() -> &'static [Builtin] {
    BUILTINS
}

pub fn find(name: &str) -> Option<&'static Builtin> {
    BUILTINS
        .iter()
        .find(|b| b.names.iter().any(|n| n.eq_ignore_ascii_case(name)))
}

/// Commands that were internal to COMMAND.COM leave ERRORLEVEL unchanged
/// when they succeed; the ones that were separate programs (FIND, SORT,
/// CHOICE, MEM, …) set it.
pub fn keeps_errorlevel(name: &str) -> bool {
    matches!(
        name,
        "DIR"
            | "CD"
            | "MD"
            | "RD"
            | "COPY"
            | "DEL"
            | "REN"
            | "TYPE"
            | "VER"
            | "VOL"
            | "DATE"
            | "TIME"
            | "CLS"
            | "COLOR"
            | "BEEP"
            | "ECHO"
            | "SET"
            | "PATH"
            | "PROMPT"
            | "REM"
            | "PAUSE"
            | "HELP"
            | "IF"
            | "FOR"
            | "GOTO"
            | "SHIFT"
    )
}

static BUILTINS: &[Builtin] = &[
    listing::DIR,
    listing::TREE,
    files::CD,
    files::MD,
    files::RD,
    files::DELTREE,
    files::COPY,
    files::MOVE,
    files::DEL,
    files::REN,
    files::TYPE,
    files::MORE,
    text::FIND,
    text::SORT,
    sys::VER,
    sys::MEM,
    sys::VOL,
    sys::DATE,
    sys::TIME,
    sys::CLS,
    sys::COLOR,
    sys::BEEP,
    sys::UNIX,
    sys::COMMAND,
    sys::SHUTDOWN,
    sys::RESTART,
    sys::LOGOFF,
    sys::EXIT,
    env::ECHO,
    env::SET,
    env::PATH,
    env::PROMPT,
    env::REM,
    env::PAUSE,
    env::HELP,
    control::IF,
    control::FOR,
    control::GOTO,
    control::CALL,
    control::SHIFT,
    control::CHOICE,
    tasks::RUN,
    tasks::START,
    tasks::TASKS,
    tasks::KILL,
    tasks::DESKTOP,
    programs::EDIT,
    programs::FILER,
    programs::NET,
    programs::WIFI,
    disk::ATTRIB,
    disk::XCOPY,
    disk::CHKDSK,
    disk::EJECT,
    config::MODE,
    config::KEYB,
    config::ADMIN,
    config::ABOUT,
];

/// Paging helper for long outputs (DIR /P, MORE, HELP).
pub struct Pager {
    pub enabled: bool,
    lines: u16,
    rows: u16,
}

impl Pager {
    pub fn new(enabled: bool) -> Pager {
        Pager {
            enabled,
            lines: 0,
            rows: crate::term::size().1.max(4),
        }
    }

    /// Call after each printed line. Returns false if the user pressed Ctrl+C.
    pub fn line(&mut self, io: &mut Io) -> bool {
        if crate::term::interrupted() {
            return false;
        }
        if !self.enabled || !io.tty {
            return true;
        }
        self.lines += 1;
        if self.lines >= self.rows - 1 {
            self.lines = 0;
            let _ = io.out.flush();
            if !wait_any_key("Press any key to continue . . .") {
                return false;
            }
        }
        true
    }
}

/// Wait for any key; false on Ctrl+C.
pub fn wait_any_key(prompt: &str) -> bool {
    use std::io::Write;
    print!("{prompt}");
    let _ = std::io::stdout().flush();
    let ok = {
        let _raw = crate::term::RawMode::enable();
        !matches!(crate::term::read_byte(), Some(3) | None)
    };
    print!("\r\x1b[K");
    if !ok {
        println!("^C");
    }
    let _ = std::io::stdout().flush();
    ok
}

//! Built-in (internal) commands.

mod env;
mod files;
mod listing;
mod sys;
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
    sys::EXIT,
    env::ECHO,
    env::SET,
    env::PATH,
    env::PROMPT,
    env::REM,
    env::PAUSE,
    env::HELP,
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

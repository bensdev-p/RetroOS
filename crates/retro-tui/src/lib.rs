//! retro-tui: the drawing and input layer for RetroOS text-mode programs.
//!
//! Programs draw into a [`Screen`] of cells using Phosphor16 colour indices
//! (DOS IRGB order) and call [`Screen::flush`]; only changed cells are sent.
//! Colours go out as ANSI SGR codes that retro-console maps back to exactly
//! the Phosphor16 palette, so the same program looks right on the RetroOS
//! console and reasonable in any other terminal.

use std::io::{self, Write};

pub mod keys;
pub use keys::{read_key, Key};

/// DOS (IRGB) colour index → ANSI colour number.
const DOS_TO_ANSI: [u8; 8] = [0, 4, 2, 6, 1, 5, 3, 7];

pub const BLACK: u8 = 0;
pub const COBALT: u8 = 1;
pub const FERN: u8 = 2;
pub const TEAL: u8 = 3;
pub const BRICK: u8 = 4;
pub const PLUM: u8 = 5;
pub const OCHRE: u8 = 6;
pub const SILVER: u8 = 7;
pub const SLATE: u8 = 8;
pub const SKY: u8 = 9;
pub const LIME: u8 = 10;
pub const AQUA: u8 = 11;
pub const CORAL: u8 = 12;
pub const ORCHID: u8 = 13;
pub const LEMON: u8 = 14;
pub const WHITE: u8 = 15;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub ch: char,
    pub fg: u8,
    pub bg: u8,
}

pub struct Screen {
    pub cols: u16,
    pub rows: u16,
    cells: Vec<Cell>,
    shown: Vec<Option<Cell>>,
    orig: Option<libc::termios>,
    pub cursor: Option<(u16, u16)>,
    /// Owns the terminal (restores it on drop).
    active: bool,
}

fn sgr(c: Cell) -> String {
    let f = if c.fg >= 8 {
        90 + DOS_TO_ANSI[(c.fg & 7) as usize]
    } else {
        30 + DOS_TO_ANSI[c.fg as usize]
    };
    let b = if c.bg >= 8 {
        100 + DOS_TO_ANSI[(c.bg & 7) as usize]
    } else {
        40 + DOS_TO_ANSI[c.bg as usize]
    };
    format!("\x1b[0;{f};{b}m")
}

pub fn term_size() -> (u16, u16) {
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 {
            return (ws.ws_col, ws.ws_row);
        }
    }
    (80, 25)
}

impl Screen {
    /// Enter full-screen mode: raw input, alternate screen, hidden cursor.
    pub fn open() -> io::Result<Screen> {
        let orig = unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(0, &mut t) == 0 {
                let o = t;
                libc::cfmakeraw(&mut t);
                t.c_oflag |= libc::OPOST | libc::ONLCR;
                libc::tcsetattr(0, libc::TCSANOW, &t);
                Some(o)
            } else {
                None
            }
        };
        print!("\x1b[?1049h\x1b[?25l\x1b[2J");
        io::stdout().flush()?;
        let (cols, rows) = term_size();
        let mut s = Screen::with_size(cols, rows, orig);
        s.active = true;
        Ok(s)
    }

    /// An off-terminal screen (tests).
    pub fn with_size(cols: u16, rows: u16, orig: Option<libc::termios>) -> Screen {
        let blank = Cell {
            ch: ' ',
            fg: SILVER,
            bg: COBALT,
        };
        let n = cols as usize * rows as usize;
        Screen {
            cols,
            rows,
            cells: vec![blank; n],
            shown: vec![None; n],
            orig,
            cursor: None,
            active: false,
        }
    }

    /// Re-read the terminal size; true if it changed (everything is redrawn).
    pub fn check_resize(&mut self) -> bool {
        let (c, r) = term_size();
        if (c, r) == (self.cols, self.rows) {
            return false;
        }
        let fresh = Screen::with_size(c, r, None);
        self.cols = c;
        self.rows = r;
        self.cells = fresh.cells.clone();
        self.shown = fresh.shown.clone();
        true
    }

    pub fn clear(&mut self, fg: u8, bg: u8) {
        self.cells.fill(Cell { ch: ' ', fg, bg });
    }

    pub fn set(&mut self, x: u16, y: u16, c: Cell) {
        if x < self.cols && y < self.rows {
            self.cells[y as usize * self.cols as usize + x as usize] = c;
        }
    }

    pub fn get(&self, x: u16, y: u16) -> Option<Cell> {
        (x < self.cols && y < self.rows)
            .then(|| self.cells[y as usize * self.cols as usize + x as usize])
    }

    pub fn text(&mut self, x: u16, y: u16, s: &str, fg: u8, bg: u8) -> u16 {
        let mut cx = x;
        for ch in s.chars() {
            self.set(cx, y, Cell { ch, fg, bg });
            cx += 1;
        }
        cx
    }

    /// Text centred on row `y`.
    pub fn center(&mut self, y: u16, s: &str, fg: u8, bg: u8) {
        let w = s.chars().count() as u16;
        self.text(self.cols.saturating_sub(w) / 2, y, s, fg, bg);
    }

    pub fn fill(&mut self, x: u16, y: u16, w: u16, h: u16, ch: char, fg: u8, bg: u8) {
        for yy in y..y.saturating_add(h) {
            for xx in x..x.saturating_add(w) {
                self.set(xx, yy, Cell { ch, fg, bg });
            }
        }
    }

    /// A DOS dialog: double frame, centred title, one-cell shadow.
    #[allow(clippy::too_many_arguments)]
    pub fn dialog(&mut self, x: u16, y: u16, w: u16, h: u16, title: &str, fg: u8, bg: u8) {
        self.fill(x, y, w, h, ' ', fg, bg);
        for xx in x + 1..x + w - 1 {
            self.set(xx, y, Cell { ch: '═', fg, bg });
            self.set(xx, y + h - 1, Cell { ch: '═', fg, bg });
        }
        for yy in y + 1..y + h - 1 {
            self.set(x, yy, Cell { ch: '║', fg, bg });
            self.set(x + w - 1, yy, Cell { ch: '║', fg, bg });
        }
        for (cx, cy, ch) in [
            (x, y, '╔'),
            (x + w - 1, y, '╗'),
            (x, y + h - 1, '╚'),
            (x + w - 1, y + h - 1, '╝'),
        ] {
            self.set(cx, cy, Cell { ch, fg, bg });
        }
        if !title.is_empty() {
            let t = format!(" {title} ");
            let tx = x + (w.saturating_sub(t.chars().count() as u16)) / 2;
            self.text(tx, y, &t, fg, bg);
        }
        // Shadow: right two columns and bottom row go dark.
        for yy in y + 1..=y + h {
            for dx in 0..2 {
                self.shade(x + w + dx, yy);
            }
        }
        for xx in x + 2..x + w + 2 {
            self.shade(xx, y + h);
        }
    }

    fn shade(&mut self, x: u16, y: u16) {
        if let Some(mut c) = self.get(x, y) {
            c.bg = BLACK;
            c.fg = SLATE;
            self.set(x, y, c);
        }
    }

    /// Send changed cells to the terminal.
    pub fn flush(&mut self) -> io::Result<()> {
        let mut out = String::new();
        let mut last: Option<(u8, u8)> = None;
        let mut pos: Option<(u16, u16)> = None;
        for y in 0..self.rows {
            for x in 0..self.cols {
                let i = y as usize * self.cols as usize + x as usize;
                let c = self.cells[i];
                if self.shown[i] == Some(c) {
                    continue;
                }
                self.shown[i] = Some(c);
                if pos != Some((x, y)) {
                    out.push_str(&format!("\x1b[{};{}H", y + 1, x + 1));
                }
                if last != Some((c.fg, c.bg)) {
                    out.push_str(&sgr(c));
                    last = Some((c.fg, c.bg));
                }
                out.push(c.ch);
                pos = Some((x + 1, y));
            }
        }
        match self.cursor {
            Some((x, y)) => out.push_str(&format!("\x1b[{};{}H\x1b[?25h\x1b[3 q", y + 1, x + 1)),
            None => out.push_str("\x1b[?25l"),
        }
        let mut so = io::stdout().lock();
        so.write_all(out.as_bytes())?;
        so.flush()
    }

    /// Forget what is on the terminal (after another program drew on it).
    pub fn invalidate(&mut self) {
        self.shown.iter_mut().for_each(|c| *c = None);
    }

    /// Plain text dump (tests).
    pub fn dump(&self) -> String {
        let mut s = String::new();
        for y in 0..self.rows {
            for x in 0..self.cols {
                s.push(self.cells[y as usize * self.cols as usize + x as usize].ch);
            }
            s.push('\n');
        }
        s
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        print!("\x1b[0m\x1b[?25h\x1b[0 q\x1b[?1049l");
        let _ = io::stdout().flush();
        if let Some(o) = self.orig {
            unsafe {
                libc::tcsetattr(0, libc::TCSANOW, &o);
            }
        }
    }
}

/// A single-line text field.
#[derive(Default, Clone)]
pub struct Field {
    pub value: String,
    pub secret: bool,
    pub max: usize,
}

impl Field {
    pub fn new(max: usize, secret: bool) -> Field {
        Field {
            value: String::new(),
            secret,
            max,
        }
    }

    /// Apply an editing key; true if consumed.
    pub fn edit(&mut self, k: &Key) -> bool {
        match k {
            Key::Char(c) if !c.is_control() && self.value.chars().count() < self.max => {
                self.value.push(*c);
                true
            }
            Key::Backspace => {
                self.value.pop();
                true
            }
            Key::Esc => {
                self.value.clear();
                true
            }
            _ => false,
        }
    }

    /// Draw at (x,y) with width w; returns the cursor column.
    pub fn draw(&self, s: &mut Screen, x: u16, y: u16, w: u16, fg: u8, bg: u8) -> u16 {
        let shown: String = if self.secret {
            "*".repeat(self.value.chars().count())
        } else {
            self.value.clone()
        };
        let visible: String = shown
            .chars()
            .rev()
            .take(w as usize - 1)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        s.fill(x, y, w, 1, ' ', fg, bg);
        s.text(x, y, &visible, fg, bg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_draws_frame_and_shadow() {
        let mut s = Screen::with_size(20, 8, None);
        s.dialog(2, 1, 10, 4, "Hi", WHITE, COBALT);
        let d = s.dump();
        let lines: Vec<&str> = d.lines().collect();
        assert!(lines[1].starts_with("  ╔═") && lines[1].contains(" Hi "));
        assert_eq!(s.get(12, 2).unwrap().bg, BLACK);
    }

    #[test]
    fn sgr_mapping() {
        assert_eq!(
            sgr(Cell {
                ch: ' ',
                fg: LEMON,
                bg: COBALT
            }),
            "\x1b[0;93;44m"
        );
        assert_eq!(
            sgr(Cell {
                ch: ' ',
                fg: SILVER,
                bg: BRICK
            }),
            "\x1b[0;37;41m"
        );
    }
}

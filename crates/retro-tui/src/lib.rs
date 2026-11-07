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
    /// Block instead of underline cursor (overtype mode).
    pub block_cursor: bool,
    /// Owns the terminal (restores it on drop).
    active: bool,
    /// Monochrome attributes (amber, green, paper schemes): see [`mono`].
    pub mono: bool,
    scheme_checked: Option<std::time::Instant>,
}

/// The session's colour scheme: Station's (live), else $RETROOS_SCHEME,
/// else startup.conf. None outside the RetroOS console (SSH, the Linux
/// console in safe mode), where the terminal's own colours apply.
pub fn current_scheme() -> Option<String> {
    std::env::var_os("RETROOS_CONSOLE")?;
    if let Some(path) = std::env::var_os("RETROOS_STATION") {
        use std::io::{BufRead, BufReader};
        if let Ok(mut sock) = std::os::unix::net::UnixStream::connect(path) {
            let t = Some(std::time::Duration::from_millis(300));
            let _ = sock.set_read_timeout(t);
            let _ = sock.set_write_timeout(t);
            if sock.write_all(b"SCHEME\n").is_ok() {
                let mut line = String::new();
                if BufReader::new(sock).read_line(&mut line).is_ok() {
                    if let Some(k) = line.trim_end().strip_prefix("OK\t") {
                        return Some(k.to_string());
                    }
                }
            }
        }
    }
    if let Ok(k) = std::env::var("RETROOS_SCHEME") {
        return Some(k);
    }
    let conf = std::fs::read_to_string("/etc/retroos/startup.conf").unwrap_or_default();
    conf.lines()
        .find_map(|l| {
            l.split('#')
                .next()?
                .trim()
                .strip_prefix("scheme=")
                .map(|v| v.trim().to_string())
        })
        .or(Some("classic".into()))
}

/// Whether a scheme shows colours as shades of one ink (amber, green, paper).
pub fn is_mono(scheme: &str) -> bool {
    matches!(scheme, "amber" | "green" | "paper")
}

/// A cell as a monochrome monitor shows it, in the manner of DOS programs'
/// black-and-white modes: light backgrounds (Silver dialogs, Teal bars,
/// selections) become inverse video, every other background black, and the
/// text normal (7), intense (15) or dim (8). The schemes turn 0/7/8/15 into
/// background, ink, bright ink and dim ink (paper: white page, black ink).
pub fn mono(c: Cell) -> Cell {
    let inverse = matches!(c.bg, TEAL | OCHRE | SILVER) || c.bg >= SKY;
    if inverse {
        let fg = if c.fg == c.bg {
            WHITE
        } else if (SKY..=WHITE).contains(&c.fg) && c.fg != WHITE || c.fg == COBALT {
            SLATE // a highlight inside an inverse box
        } else {
            BLACK
        };
        Cell {
            ch: c.ch,
            fg,
            bg: WHITE,
        }
    } else {
        let fg = match c.fg {
            f if f == c.bg => BLACK,
            SLATE => SLATE,
            f if f >= SKY => WHITE,
            _ => SILVER,
        };
        Cell {
            ch: c.ch,
            fg,
            bg: BLACK,
        }
    }
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
        s.mono = current_scheme().is_some_and(|k| is_mono(&k));
        s.scheme_checked = Some(std::time::Instant::now());
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
            block_cursor: false,
            active: false,
            mono: false,
            scheme_checked: None,
        }
    }

    /// Monochrome attributes on or off (everything is redrawn).
    pub fn set_mono(&mut self, on: bool) {
        if on != self.mono {
            self.mono = on;
            self.invalidate();
        }
    }

    /// Re-read the terminal size (and, once a second, the colour scheme);
    /// true if it changed (everything is redrawn).
    pub fn check_resize(&mut self) -> bool {
        if self
            .scheme_checked
            .is_some_and(|t| t.elapsed().as_secs() >= 1)
        {
            self.scheme_checked = Some(std::time::Instant::now());
            let m = current_scheme().is_some_and(|k| is_mono(&k));
            if m != self.mono {
                self.set_mono(m);
                return true;
            }
        }
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
                let c = if self.mono {
                    mono(self.cells[i])
                } else {
                    self.cells[i]
                };
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
            Some((x, y)) => out.push_str(&format!(
                "\x1b[{};{}H\x1b[?25h\x1b[{} q",
                y + 1,
                x + 1,
                if self.block_cursor { 1 } else { 3 }
            )),
            None => out.push_str("\x1b[?25l"),
        }
        let mut so = io::stdout().lock();
        so.write_all(out.as_bytes())?;
        so.flush()
    }

    /// The terminal mode from before `open` (to run another program).
    pub fn original_mode(&self) -> Option<libc::termios> {
        self.orig
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
    fn monochrome_keeps_text_readable() {
        let on_blue = mono(Cell {
            ch: 'a',
            fg: SILVER,
            bg: COBALT,
        });
        assert_eq!((on_blue.fg, on_blue.bg), (SILVER, BLACK));
        let title = mono(Cell {
            ch: 'a',
            fg: WHITE,
            bg: COBALT,
        });
        assert_eq!(title.fg, WHITE);
        let bar = mono(Cell {
            ch: 'a',
            fg: BLACK,
            bg: TEAL,
        });
        assert_eq!((bar.fg, bar.bg), (BLACK, WHITE));
        let sel = mono(Cell {
            ch: 'a',
            fg: BLACK,
            bg: SILVER,
        });
        assert_eq!((sel.fg, sel.bg), (BLACK, WHITE));
        let field = mono(Cell {
            ch: 'a',
            fg: WHITE,
            bg: BLACK,
        });
        assert_eq!((field.fg, field.bg), (WHITE, BLACK));
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

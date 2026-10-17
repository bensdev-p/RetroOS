//! A DOSKEY-style line editor.
//!
//! Keys: ←/→, Ctrl+←/→ (words), Home/End, Backspace/Del, Ins (insert ⇄
//! overtype; the cursor turns from underline to block), Esc (clear line),
//! ↑/↓ and PgUp/PgDn (history), F1 (copy one character of the previous
//! command), F3 (copy the rest of it), F7 (list history), Tab (complete,
//! repeated Tab cycles), Ctrl+C (cancel), Ctrl+L (clear screen).

use crate::term::{self, RawMode};
use std::io::{self, Write};

pub enum Outcome {
    Line(String),
    Cancelled,
    Eof,
}

pub struct Editor {
    pub history: Vec<String>,
    pub max_history: usize,
    insert: bool,
}

/// Returns completions for the token text before the cursor.
pub type Completer<'a> = &'a dyn Fn(&str) -> Vec<String>;

impl Default for Editor {
    fn default() -> Self {
        Editor {
            history: Vec::new(),
            max_history: 200,
            insert: true,
        }
    }
}

enum Key {
    Char(char),
    Enter,
    Backspace,
    Delete,
    Left,
    Right,
    WordLeft,
    WordRight,
    Home,
    End,
    Up,
    Down,
    PgUp,
    PgDn,
    Insert,
    Esc,
    Tab,
    F(u8),
    CtrlC,
    CtrlD,
    CtrlL,
    Other,
}

fn read_key() -> Option<Key> {
    let b = term::read_byte()?;
    Some(match b {
        b'\r' | b'\n' => Key::Enter,
        0x7f | 0x08 => Key::Backspace,
        b'\t' => Key::Tab,
        3 => Key::CtrlC,
        4 => Key::CtrlD,
        12 => Key::CtrlL,
        0x1b => {
            if !term::poll_input(40) {
                return Some(Key::Esc);
            }
            match term::read_byte()? {
                b'[' => csi()?,
                b'O' => match term::read_byte()? {
                    b'H' => Key::Home,
                    b'F' => Key::End,
                    b'P' => Key::F(1),
                    b'Q' => Key::F(2),
                    b'R' => Key::F(3),
                    b'S' => Key::F(4),
                    b'A' => Key::Up,
                    b'B' => Key::Down,
                    b'C' => Key::Right,
                    b'D' => Key::Left,
                    _ => Key::Other,
                },
                _ => Key::Other,
            }
        }
        b if b < 0x20 => Key::Other,
        b if b < 0x80 => Key::Char(b as char),
        b => {
            // UTF-8 multi-byte sequence.
            let n = if b >= 0xF0 {
                3
            } else if b >= 0xE0 {
                2
            } else {
                1
            };
            let mut buf = vec![b];
            for _ in 0..n {
                buf.push(term::read_byte()?);
            }
            match std::str::from_utf8(&buf)
                .ok()
                .and_then(|s| s.chars().next())
            {
                Some(c) => Key::Char(c),
                None => Key::Other,
            }
        }
    })
}

fn csi() -> Option<Key> {
    let mut params = String::new();
    loop {
        let b = term::read_byte()?;
        if (0x40..=0x7e).contains(&b) {
            let ctrl = params.ends_with(";5");
            return Some(match (b, params.split(';').next().unwrap_or("")) {
                (b'A', _) => Key::Up,
                (b'B', _) => Key::Down,
                (b'C', _) if ctrl => Key::WordRight,
                (b'D', _) if ctrl => Key::WordLeft,
                (b'C', _) => Key::Right,
                (b'D', _) => Key::Left,
                (b'H', _) => Key::Home,
                (b'F', _) => Key::End,
                (b'~', "1") | (b'~', "7") => Key::Home,
                (b'~', "4") | (b'~', "8") => Key::End,
                (b'~', "2") => Key::Insert,
                (b'~', "3") => Key::Delete,
                (b'~', "5") => Key::PgUp,
                (b'~', "6") => Key::PgDn,
                (b'~', "11") => Key::F(1),
                (b'~', "12") => Key::F(2),
                (b'~', "13") => Key::F(3),
                (b'~', "14") => Key::F(4),
                (b'~', "15") => Key::F(5),
                (b'~', "17") => Key::F(6),
                (b'~', "18") => Key::F(7),
                (b'~', "19") => Key::F(8),
                (b'P', _) => Key::F(1),
                (b'R', _) => Key::F(3),
                _ => Key::Other,
            });
        }
        params.push(b as char);
    }
}

struct View<'a> {
    out: io::StdoutLock<'a>,
    start_col: usize,
    cols: usize,
}

impl View<'_> {
    fn pos(&self, text: &[char], i: usize) -> (usize, usize) {
        let p = self.start_col + i.min(text.len());
        (p / self.cols, p % self.cols)
    }

    fn move_cursor(&mut self, text: &[char], from: usize, to: usize) {
        let (fr, _) = self.pos(text, from);
        let (tr, tc) = self.pos(text, to);
        if tr < fr {
            let _ = write!(self.out, "\x1b[{}A", fr - tr);
        } else if tr > fr {
            let _ = write!(self.out, "\x1b[{}B", tr - fr);
        }
        let _ = write!(self.out, "\r");
        if tc > 0 {
            let _ = write!(self.out, "\x1b[{tc}C");
        }
    }

    /// Redraw text from index `from` to the end, then park the cursor at `cursor`.
    /// The terminal cursor is assumed to be at `from`.
    fn redraw_from(&mut self, text: &[char], from: usize, cursor: usize) {
        let s: String = text[from..].iter().collect();
        let _ = write!(self.out, "{s}");
        let end = text.len();
        if (self.start_col + end).is_multiple_of(self.cols) && end > from {
            // Ending exactly at the right margin leaves the cursor "pending
            // wrap" on the last column, where an erase would wipe the last
            // character. Wrap for real first; the space is erased below.
            let _ = write!(self.out, " \r");
        }
        let _ = write!(self.out, "\x1b[J");
        self.move_cursor(text, end, cursor);
        let _ = self.out.flush();
    }
}

fn cursor_shape(out: &mut impl Write, insert: bool) {
    // DECSCUSR: 3 = blinking underline (insert), 1 = blinking block (overtype).
    let _ = write!(out, "\x1b[{} q", if insert { 3 } else { 1 });
}

impl Editor {
    /// Read a line. `prompt_width` is the column the cursor is at after the prompt.
    pub fn read_line(&mut self, prompt: &str, complete: Completer) -> Outcome {
        let mut stdout = io::stdout();
        let _ = write!(stdout, "{prompt}");
        let _ = stdout.flush();
        if !term::is_tty(0) {
            let mut s = String::new();
            return match io::stdin().read_line(&mut s) {
                Ok(0) | Err(_) => Outcome::Eof,
                Ok(_) => Outcome::Line(s.trim_end_matches(['\r', '\n']).to_string()),
            };
        }
        let Ok(_raw) = RawMode::enable() else {
            return Outcome::Eof;
        };
        let (cols, _) = term::size();
        let last_prompt_line = prompt.rsplit('\n').next().unwrap_or(prompt);
        let mut v = View {
            out: io::stdout().lock(),
            start_col: visible_width(last_prompt_line) % cols.max(1) as usize,
            cols: cols.max(1) as usize,
        };
        cursor_shape(&mut v.out, self.insert);
        let _ = v.out.flush();

        let mut text: Vec<char> = Vec::new();
        let mut cur = 0usize;
        let mut hist_idx = self.history.len();
        let template: Vec<char> = self
            .history
            .last()
            .map(|s| s.chars().collect())
            .unwrap_or_default();
        let mut cycle: Option<(usize, Vec<String>, usize)> = None; // (token start, candidates, index)

        loop {
            let Some(key) = read_key() else {
                return Outcome::Eof;
            };
            if !matches!(key, Key::Tab) {
                cycle = None;
            }
            match key {
                Key::Enter => {
                    let len = text.len();
                    v.move_cursor(&text, cur, len);
                    let _ = write!(v.out, "\r\n");
                    let _ = v.out.flush();
                    let line: String = text.iter().collect();
                    if !line.trim().is_empty() && self.history.last() != Some(&line) {
                        self.history.push(line.clone());
                        if self.history.len() > self.max_history {
                            self.history.remove(0);
                        }
                    }
                    return Outcome::Line(line);
                }
                Key::CtrlC => {
                    let len = text.len();
                    v.move_cursor(&text, cur, len);
                    let _ = write!(v.out, "^C\r\n");
                    let _ = v.out.flush();
                    return Outcome::Cancelled;
                }
                Key::CtrlD if text.is_empty() => {
                    let _ = write!(v.out, "\r\n");
                    return Outcome::Eof;
                }
                Key::CtrlL => {
                    let _ = write!(v.out, "\x1b[2J\x1b[H{prompt}");
                    v.redraw_from(&text, 0, cur);
                }
                Key::Char(c) => {
                    if self.insert || cur == text.len() {
                        text.insert(cur, c);
                    } else {
                        text[cur] = c;
                    }
                    let from = cur;
                    cur += 1;
                    v.redraw_from(&text, from, cur);
                }
                Key::Backspace if cur > 0 => {
                    v.move_cursor(&text, cur, cur - 1);
                    cur -= 1;
                    text.remove(cur);
                    v.redraw_from(&text, cur, cur);
                }
                Key::Delete if cur < text.len() => {
                    text.remove(cur);
                    v.redraw_from(&text, cur, cur);
                }
                Key::Left if cur > 0 => {
                    v.move_cursor(&text, cur, cur - 1);
                    cur -= 1;
                }
                Key::Right if cur < text.len() => {
                    v.move_cursor(&text, cur, cur + 1);
                    cur += 1;
                }
                Key::WordLeft => {
                    let mut n = cur;
                    while n > 0 && text[n - 1] == ' ' {
                        n -= 1;
                    }
                    while n > 0 && text[n - 1] != ' ' {
                        n -= 1;
                    }
                    v.move_cursor(&text, cur, n);
                    cur = n;
                }
                Key::WordRight => {
                    let mut n = cur;
                    while n < text.len() && text[n] != ' ' {
                        n += 1;
                    }
                    while n < text.len() && text[n] == ' ' {
                        n += 1;
                    }
                    v.move_cursor(&text, cur, n);
                    cur = n;
                }
                Key::Home => {
                    v.move_cursor(&text, cur, 0);
                    cur = 0;
                }
                Key::End => {
                    let len = text.len();
                    v.move_cursor(&text, cur, len);
                    cur = len;
                }
                Key::Insert => {
                    self.insert = !self.insert;
                    cursor_shape(&mut v.out, self.insert);
                    let _ = v.out.flush();
                }
                Key::Esc => {
                    v.move_cursor(&text, cur, 0);
                    text.clear();
                    cur = 0;
                    v.redraw_from(&text, 0, 0);
                }
                Key::Up | Key::Down | Key::PgUp | Key::PgDn if !self.history.is_empty() => {
                    hist_idx = match key {
                        Key::Up => hist_idx.saturating_sub(1),
                        Key::Down => (hist_idx + 1).min(self.history.len()),
                        Key::PgUp => 0,
                        _ => self.history.len() - 1,
                    };
                    v.move_cursor(&text, cur, 0);
                    text = self
                        .history
                        .get(hist_idx)
                        .map(|s| s.chars().collect())
                        .unwrap_or_default();
                    cur = text.len();
                    v.redraw_from(&text, 0, cur);
                }
                Key::F(1) if cur < template.len() => {
                    let from = cur;
                    if cur < text.len() {
                        text[cur] = template[cur];
                    } else {
                        text.push(template[cur]);
                    }
                    cur += 1;
                    v.redraw_from(&text, from, cur);
                }
                Key::F(3) if cur < template.len() => {
                    let from = cur;
                    text.truncate(cur);
                    text.extend_from_slice(&template[cur..]);
                    cur = text.len();
                    v.redraw_from(&text, from, cur);
                }
                Key::F(7) => {
                    let len = text.len();
                    v.move_cursor(&text, cur, len);
                    let _ = write!(v.out, "\r\n");
                    for (i, h) in self.history.iter().enumerate() {
                        let _ = write!(v.out, "{:>3}: {h}\r\n", i + 1);
                    }
                    let _ = write!(v.out, "{prompt}");
                    v.redraw_from(&text, 0, cur);
                }
                Key::Tab => {
                    let (start, cands, idx) = match cycle.take() {
                        Some((s, c, i)) => {
                            let n = c.len();
                            (s, c, (i + 1) % n)
                        }
                        None => {
                            let start = token_start(&text, cur);
                            let tok: String = text[start..cur].iter().collect();
                            (start, complete(&tok), 0)
                        }
                    };
                    if cands.is_empty() {
                        let _ = write!(v.out, "\x07");
                        let _ = v.out.flush();
                        continue;
                    }
                    let repl: Vec<char> = cands[idx].chars().collect();
                    v.move_cursor(&text, cur, start);
                    text.splice(start..cur, repl.iter().copied());
                    cur = start + repl.len();
                    v.redraw_from(&text, start, cur);
                    cycle = Some((start, cands, idx));
                }
                _ => {}
            }
        }
    }
}

fn token_start(text: &[char], cur: usize) -> usize {
    let mut quoted = false;
    let mut start = 0;
    for (i, &c) in text[..cur].iter().enumerate() {
        if c == '"' {
            quoted = !quoted;
        } else if c == ' ' && !quoted {
            start = i + 1;
        }
    }
    start
}

/// Width of a string on screen, ignoring ANSI escape sequences.
pub fn visible_width(s: &str) -> usize {
    #[derive(PartialEq)]
    enum St {
        Text,
        Esc,
        Csi,
        Osc,
    }
    let mut st = St::Text;
    let mut n = 0;
    for c in s.chars() {
        st = match st {
            St::Text if c == '\x1b' => St::Esc,
            St::Text => {
                if !c.is_control() {
                    n += 1;
                }
                St::Text
            }
            St::Esc if c == '[' => St::Csi,
            St::Esc if c == ']' => St::Osc,
            St::Esc => St::Text,
            St::Csi if ('@'..='~').contains(&c) => St::Text,
            St::Csi => St::Csi,
            St::Osc if c == '\x07' => St::Text,
            St::Osc => St::Osc,
        };
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths() {
        assert_eq!(visible_width("C:\\>"), 4);
        assert_eq!(visible_width("\x1b[1;33mC:\\>\x1b[0m"), 4);
    }

    #[test]
    fn token_boundaries() {
        let t: Vec<char> = "copy \"my fi".chars().collect();
        assert_eq!(token_start(&t, t.len()), 5);
        let t: Vec<char> = "dir doc".chars().collect();
        assert_eq!(token_start(&t, t.len()), 4);
    }
}

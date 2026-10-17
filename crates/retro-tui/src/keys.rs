//! Key input from a raw-mode terminal (xterm sequences).

use std::io;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Tab,
    BackTab,
    Backspace,
    Delete,
    Insert,
    Esc,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PgUp,
    PgDn,
    F(u8),
    Ctrl(char),
    Alt(char),
    /// A navigation/editing key with modifiers (Shift+Right, Ctrl+Home, …).
    Mod(Mods, Box<Key>),
    Mouse(Mouse),
    Resize,
    Unknown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

/// An SGR (1006) mouse report; `x`, `y` are 0-based cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mouse {
    pub x: u16,
    pub y: u16,
    /// 0 left, 1 middle, 2 right, 64/65 wheel up/down; +32 while dragging.
    pub button: u16,
    pub pressed: bool,
}

impl Key {
    /// Shift+key as a modified key.
    pub fn shift(k: Key) -> Key {
        Key::Mod(
            Mods {
                shift: true,
                ..Mods::default()
            },
            Box::new(k),
        )
    }

    pub fn ctrl(k: Key) -> Key {
        Key::Mod(
            Mods {
                ctrl: true,
                ..Mods::default()
            },
            Box::new(k),
        )
    }

    /// The key without modifiers, and the modifiers.
    pub fn split(&self) -> (Mods, &Key) {
        match self {
            Key::Mod(m, k) => (*m, k),
            k => (Mods::default(), k),
        }
    }
}

/// Turn on mouse reporting (button events, SGR coordinates).
pub fn enable_mouse() -> &'static str {
    "\x1b[?1000h\x1b[?1002h\x1b[?1006h"
}

pub fn disable_mouse() -> &'static str {
    "\x1b[?1006l\x1b[?1002l\x1b[?1000l"
}

thread_local! {
    /// A byte read ahead that belongs to the next key.
    static PENDING: std::cell::Cell<Option<u8>> = const { std::cell::Cell::new(None) };
}

fn byte(timeout_ms: i32) -> io::Result<Option<u8>> {
    if let Some(b) = PENDING.with(|p| p.take()) {
        return Ok(Some(b));
    }
    let mut fds = libc::pollfd {
        fd: 0,
        events: libc::POLLIN,
        revents: 0,
    };
    let r = unsafe { libc::poll(&mut fds, 1, timeout_ms) };
    if r < 0 {
        let e = io::Error::last_os_error();
        return if e.kind() == io::ErrorKind::Interrupted {
            Ok(None)
        } else {
            Err(e)
        };
    }
    if r == 0 {
        return Ok(None);
    }
    let mut b = [0u8; 1];
    let n = unsafe { libc::read(0, b.as_mut_ptr().cast(), 1) };
    match n {
        1 => Ok(Some(b[0])),
        0 => Err(io::Error::new(io::ErrorKind::UnexpectedEof, "input closed")),
        _ => Err(io::Error::last_os_error()),
    }
}

/// Wait up to `timeout_ms` (-1 = forever) for a key.
pub fn read_key(timeout_ms: i32) -> io::Result<Option<Key>> {
    let Some(b) = byte(timeout_ms)? else {
        return Ok(None);
    };
    Ok(Some(match b {
        b'\r' | b'\n' => Key::Enter,
        b'\t' => Key::Tab,
        0x7f | 0x08 => Key::Backspace,
        0x1b => match byte(30)? {
            None => Key::Esc,
            Some(b'[') => csi()?,
            Some(b'O') => match byte(30)? {
                Some(b'P') => Key::F(1),
                Some(b'Q') => Key::F(2),
                Some(b'R') => Key::F(3),
                Some(b'S') => Key::F(4),
                Some(b'H') => Key::Home,
                Some(b'F') => Key::End,
                Some(b'A') => Key::Up,
                Some(b'B') => Key::Down,
                Some(b'C') => Key::Right,
                Some(b'D') => Key::Left,
                _ => Key::Unknown,
            },
            Some(c) if c.is_ascii() && !c.is_ascii_control() => {
                Key::Alt((c as char).to_ascii_lowercase())
            }
            // Esc, then another key typed quickly (Esc, Ctrl+S): two keys.
            Some(b) => {
                PENDING.with(|p| p.set(Some(b)));
                Key::Esc
            }
        },
        b @ 1..=26 => Key::Ctrl((b'a' + b - 1) as char),
        b if b < 0x80 => Key::Char(b as char),
        b => {
            let n = if b >= 0xF0 {
                3
            } else if b >= 0xE0 {
                2
            } else {
                1
            };
            let mut v = vec![b];
            for _ in 0..n {
                if let Some(x) = byte(30)? {
                    v.push(x);
                }
            }
            String::from_utf8(v)
                .ok()
                .and_then(|s| s.chars().next())
                .map(Key::Char)
                .unwrap_or(Key::Unknown)
        }
    }))
}

fn csi() -> io::Result<Key> {
    let mut p = String::new();
    loop {
        let Some(b) = byte(30)? else {
            return Ok(Key::Unknown);
        };
        if (0x40..=0x7e).contains(&b) {
            return Ok(decode_csi(&p, b));
        }
        p.push(b as char);
    }
}

/// Decode `ESC [ <params> <final>`.
pub fn decode_csi(p: &str, fin: u8) -> Key {
    if let Some(m) = p.strip_prefix('<') {
        let v: Vec<u16> = m.split(';').filter_map(|n| n.parse().ok()).collect();
        if v.len() == 3 && (fin == b'M' || fin == b'm') {
            return Key::Mouse(Mouse {
                button: v[0],
                x: v[1].saturating_sub(1),
                y: v[2].saturating_sub(1),
                pressed: fin == b'M',
            });
        }
        return Key::Unknown;
    }
    let mut parts = p.split(';');
    let first = parts.next().unwrap_or("").to_string();
    let m: u8 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    let base = match (fin, first.as_str()) {
        (b'A', _) => Key::Up,
        (b'B', _) => Key::Down,
        (b'C', _) => Key::Right,
        (b'D', _) => Key::Left,
        (b'H', _) => Key::Home,
        (b'F', _) => Key::End,
        (b'Z', _) => return Key::BackTab,
        (b'P', _) => Key::F(1),
        (b'Q', _) => Key::F(2),
        (b'R', _) => Key::F(3),
        (b'S', _) => Key::F(4),
        (b'~', "1") | (b'~', "7") => Key::Home,
        (b'~', "4") | (b'~', "8") => Key::End,
        (b'~', "2") => Key::Insert,
        (b'~', "3") => Key::Delete,
        (b'~', "5") => Key::PgUp,
        (b'~', "6") => Key::PgDn,
        (b'~', n) => match n.parse::<u8>().unwrap_or(0) {
            n @ 11..=15 => Key::F(n - 10),
            n @ 17..=21 => Key::F(n - 11),
            23 => Key::F(11),
            24 => Key::F(12),
            _ => return Key::Unknown,
        },
        _ => return Key::Unknown,
    };
    if m <= 1 {
        return base;
    }
    let bits = m - 1;
    Key::Mod(
        Mods {
            shift: bits & 1 != 0,
            alt: bits & 2 != 0,
            ctrl: bits & 4 != 0,
        },
        Box::new(base),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_and_mouse() {
        assert_eq!(decode_csi("1;2", b'C'), Key::shift(Key::Right));
        assert_eq!(decode_csi("1;5", b'H'), Key::ctrl(Key::Home));
        assert_eq!(decode_csi("3;2", b'~'), Key::shift(Key::Delete));
        assert_eq!(decode_csi("", b'A'), Key::Up);
        assert_eq!(decode_csi("15", b'~'), Key::F(5));
        assert_eq!(
            decode_csi("<0;10;3", b'M'),
            Key::Mouse(Mouse {
                x: 9,
                y: 2,
                button: 0,
                pressed: true
            })
        );
    }
}

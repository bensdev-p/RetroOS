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
    Resize,
    Unknown,
}

fn byte(timeout_ms: i32) -> io::Result<Option<u8>> {
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
            Some(_) => Key::Unknown,
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
            let first = p.split(';').next().unwrap_or("").to_string();
            return Ok(match (b, first.as_str()) {
                (b'A', _) => Key::Up,
                (b'B', _) => Key::Down,
                (b'C', _) => Key::Right,
                (b'D', _) => Key::Left,
                (b'H', _) => Key::Home,
                (b'F', _) => Key::End,
                (b'Z', _) => Key::BackTab,
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
                    11..=15 => Key::F(n.parse::<u8>().unwrap() - 10),
                    17..=21 => Key::F(n.parse::<u8>().unwrap() - 11),
                    23 => Key::F(11),
                    24 => Key::F(12),
                    _ => Key::Unknown,
                },
                _ => Key::Unknown,
            });
        }
        p.push(b as char);
    }
}

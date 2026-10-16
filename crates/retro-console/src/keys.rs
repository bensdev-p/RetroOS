//! Keyboard → terminal byte sequences (xterm conventions).

use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym, Modifiers};

/// xterm modifier parameter: 1 + shift + 2·alt + 4·ctrl.
fn modparam(m: &Modifiers) -> u8 {
    1 + m.shift as u8 + 2 * m.alt as u8 + 4 * m.ctrl as u8
}

fn csi(final_: char, m: &Modifiers, app: bool) -> String {
    let p = modparam(m);
    if p > 1 {
        format!("\x1b[1;{p}{final_}")
    } else if app {
        format!("\x1bO{final_}")
    } else {
        format!("\x1b[{final_}")
    }
}

fn tilde(n: u8, m: &Modifiers) -> String {
    let p = modparam(m);
    if p > 1 {
        format!("\x1b[{n};{p}~")
    } else {
        format!("\x1b[{n}~")
    }
}

/// Encode a key press. `app_cursor` is DECCKM (application cursor keys).
pub fn encode(ev: &KeyEvent, m: &Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    let s = match ev.keysym {
        Keysym::Return | Keysym::KP_Enter => {
            if m.alt {
                "\x1b\r".to_string()
            } else {
                "\r".into()
            }
        }
        Keysym::Tab => "\t".into(),
        Keysym::ISO_Left_Tab => "\x1b[Z".into(),
        Keysym::BackSpace => {
            let b = if m.ctrl { "\x08" } else { "\x7f" };
            if m.alt {
                format!("\x1b{b}")
            } else {
                b.into()
            }
        }
        Keysym::Escape => "\x1b".into(),
        Keysym::Up | Keysym::KP_Up => csi('A', m, app_cursor),
        Keysym::Down | Keysym::KP_Down => csi('B', m, app_cursor),
        Keysym::Right | Keysym::KP_Right => csi('C', m, app_cursor),
        Keysym::Left | Keysym::KP_Left => csi('D', m, app_cursor),
        Keysym::Home | Keysym::KP_Home => csi('H', m, app_cursor),
        Keysym::End | Keysym::KP_End => csi('F', m, app_cursor),
        Keysym::Insert | Keysym::KP_Insert => tilde(2, m),
        Keysym::Delete | Keysym::KP_Delete => tilde(3, m),
        Keysym::Page_Up | Keysym::KP_Page_Up => tilde(5, m),
        Keysym::Page_Down | Keysym::KP_Page_Down => tilde(6, m),
        Keysym::F1 => csi('P', m, true),
        Keysym::F2 => csi('Q', m, true),
        Keysym::F3 => csi('R', m, true),
        Keysym::F4 => csi('S', m, true),
        Keysym::F5 => tilde(15, m),
        Keysym::F6 => tilde(17, m),
        Keysym::F7 => tilde(18, m),
        Keysym::F8 => tilde(19, m),
        Keysym::F9 => tilde(20, m),
        Keysym::F10 => tilde(21, m),
        Keysym::F11 => tilde(23, m),
        Keysym::F12 => tilde(24, m),
        _ => {
            let text = ev.utf8.clone().filter(|t| !t.is_empty())?;
            // Ctrl+Space / Ctrl+@ → NUL.
            if m.ctrl && (ev.keysym == Keysym::space || ev.keysym == Keysym::at) {
                return Some(if m.alt { vec![0x1b, 0] } else { vec![0] });
            }
            if m.alt {
                format!("\x1b{text}")
            } else {
                text
            }
        }
    };
    Some(s.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(sym: Keysym, utf8: Option<&str>) -> KeyEvent {
        KeyEvent {
            time: 0,
            raw_code: 0,
            keysym: sym,
            utf8: utf8.map(String::from),
        }
    }

    #[test]
    fn sequences() {
        let none = Modifiers::default();
        let ctrl = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        assert_eq!(
            encode(&ev(Keysym::Up, None), &none, false).unwrap(),
            b"\x1b[A"
        );
        assert_eq!(
            encode(&ev(Keysym::Up, None), &none, true).unwrap(),
            b"\x1bOA"
        );
        assert_eq!(
            encode(&ev(Keysym::Left, None), &ctrl, false).unwrap(),
            b"\x1b[1;5D"
        );
        assert_eq!(
            encode(&ev(Keysym::F3, None), &none, false).unwrap(),
            b"\x1bOR"
        );
        assert_eq!(
            encode(&ev(Keysym::F8, None), &none, false).unwrap(),
            b"\x1b[19~"
        );
        assert_eq!(
            encode(&ev(Keysym::c, Some("\x03")), &ctrl, false).unwrap(),
            b"\x03"
        );
        assert_eq!(
            encode(&ev(Keysym::Return, None), &none, false).unwrap(),
            b"\r"
        );
    }
}

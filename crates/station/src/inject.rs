//! Synthetic key input for automated tests (`station --test-ipc`). Keys go
//! through the same path as real keyboards, hotkeys included. Uses US-layout
//! evdev scancodes.

use crate::state::Station;
use smithay::backend::input::KeyState;

const SHIFT: u32 = 42;
const CTRL: u32 = 29;
const ALT: u32 = 56;

/// (evdev code, needs shift) for a character on a US keyboard.
fn us_key(c: char) -> Option<(u32, bool)> {
    const ROW1: &str = "1234567890-=";
    const ROW1S: &str = "!@#$%^&*()_+";
    const Q: &str = "qwertyuiop[]";
    const QS: &str = "QWERTYUIOP{}";
    const A: &str = "asdfghjkl;'`";
    const AS: &str = "ASDFGHJKL:\"~";
    const Z: &str = "zxcvbnm,./";
    const ZS: &str = "ZXCVBNM<>?";
    let find = |s: &str, base: u32| s.chars().position(|x| x == c).map(|i| base + i as u32);
    if let Some(k) = find(ROW1, 2) {
        return Some((k, false));
    }
    if let Some(k) = find(ROW1S, 2) {
        return Some((k, true));
    }
    if let Some(k) = find(Q, 16) {
        return Some((k, false));
    }
    if let Some(k) = find(QS, 16) {
        return Some((k, true));
    }
    if let Some(k) = find(A, 30) {
        return Some((if c == '`' { 41 } else { k }, false));
    }
    if let Some(k) = find(AS, 30) {
        return Some((if c == '~' { 41 } else { k }, true));
    }
    if let Some(k) = find(Z, 44) {
        return Some((k, false));
    }
    if let Some(k) = find(ZS, 44) {
        return Some((k, true));
    }
    match c {
        ' ' => Some((57, false)),
        '\\' => Some((43, false)),
        '|' => Some((43, true)),
        '\n' | '\r' => Some((28, false)),
        '\t' => Some((15, false)),
        _ => None,
    }
}

fn named(k: &str) -> Option<u32> {
    Some(match k.to_ascii_lowercase().as_str() {
        "enter" | "return" => 28,
        "tab" => 15,
        "esc" | "escape" => 1,
        "backspace" => 14,
        "space" => 57,
        "up" => 103,
        "down" => 108,
        "left" => 105,
        "right" => 106,
        "home" => 102,
        "end" => 107,
        "pgup" => 104,
        "pgdn" => 109,
        "insert" | "ins" => 110,
        "delete" | "del" => 111,
        "print" => 99,
        "ctrl" => CTRL,
        "alt" => ALT,
        "shift" => SHIFT,
        f if f.starts_with('f') && (2..=3).contains(&f.len()) => {
            let n: u32 = f[1..].parse().ok()?;
            match n {
                1..=10 => 58 + n,
                11 => 87,
                12 => 88,
                _ => return None,
            }
        }
        s if s.chars().count() == 1 => us_key(s.chars().next()?)?.0,
        _ => return None,
    })
}

impl Station {
    fn tap(&mut self, code: u32, state: KeyState) {
        let t = self.now_ms();
        self.handle_key(code + 8, state, t);
    }

    pub fn inject_text(&mut self, text: &str) {
        for c in text.chars() {
            let Some((code, shift)) = us_key(c) else {
                continue;
            };
            if shift {
                self.tap(SHIFT, KeyState::Pressed);
            }
            self.tap(code, KeyState::Pressed);
            self.tap(code, KeyState::Released);
            if shift {
                self.tap(SHIFT, KeyState::Released);
            }
        }
    }

    /// "ctrl+alt+tab", "enter", "f3"…; "down:alt" / "up:alt" press or
    /// release one key (to hold Alt across steps).
    pub fn inject_combo(&mut self, combo: &str) -> bool {
        for (prefix, state) in [("down:", KeyState::Pressed), ("up:", KeyState::Released)] {
            if let Some(k) = combo.strip_prefix(prefix) {
                let Some(code) = named(k) else { return false };
                self.tap(code, state);
                return true;
            }
        }
        let codes: Option<Vec<u32>> = combo.split('+').map(named).collect();
        let Some(codes) = codes else { return false };
        for c in &codes {
            self.tap(*c, KeyState::Pressed);
        }
        for c in codes.iter().rev() {
            self.tap(*c, KeyState::Released);
        }
        true
    }
}

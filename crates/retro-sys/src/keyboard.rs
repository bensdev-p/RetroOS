//! The keyboard layout: /etc/default/keyboard (read by Station and the
//! Linux console), written by the `set-keyboard` helper (KEYB, Control
//! Panel → Keyboard). The list of layouts is XKB's.

use crate::admin::{self, AdminError};

pub const DEFAULT_KEYBOARD: &str = "/etc/default/keyboard";
const EVDEV_LST: &str = "/usr/share/X11/xkb/rules/evdev.lst";

/// (layout, variant); variant "" when none. "us" when unset.
pub fn current() -> (String, String) {
    parse_default(&std::fs::read_to_string(DEFAULT_KEYBOARD).unwrap_or_default())
}

pub fn parse_default(text: &str) -> (String, String) {
    let get = |key: &str| {
        text.lines()
            .find_map(|l| l.trim().strip_prefix(key))
            .map(|v| v.trim().trim_matches('"').to_string())
            .unwrap_or_default()
    };
    let layout = get("XKBLAYOUT=");
    let layout = if layout.is_empty() {
        "us".into()
    } else {
        layout
    };
    // Several layouts ("us,de"): the first is the one in use.
    let first = |s: String| s.split(',').next().unwrap_or("").to_string();
    (first(layout), first(get("XKBVARIANT=")))
}

/// Every XKB layout as (name, description), by description; a small
/// built-in list when xkb-data is missing.
pub fn layouts() -> Vec<(String, String)> {
    let mut v = parse_section(
        &std::fs::read_to_string(EVDEV_LST).unwrap_or_default(),
        "layout",
    );
    if v.is_empty() {
        v = [
            ("us", "English (US)"),
            ("gb", "English (UK)"),
            ("de", "German"),
            ("fr", "French"),
            ("es", "Spanish"),
            ("it", "Italian"),
        ]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
    }
    v.sort_by_key(|a| a.1.to_lowercase());
    v
}

/// The `! section` lines of evdev.lst as (name, rest).
fn parse_section(text: &str, section: &str) -> Vec<(String, String)> {
    let head = format!("! {section}");
    let mut on = false;
    let mut v = Vec::new();
    for l in text.lines() {
        if l.starts_with('!') {
            on = l.trim() == head;
            continue;
        }
        if !on {
            continue;
        }
        let l = l.trim();
        if let Some((name, desc)) = l.split_once(char::is_whitespace) {
            v.push((name.to_string(), desc.trim().to_string()));
        }
    }
    v
}

/// Save the layout (the caller tells Station: broker KEYMAP).
pub fn set(layout: &str, variant: &str) -> Result<(), AdminError> {
    let mut args = vec![layout];
    if !variant.is_empty() {
        args.push(variant);
    }
    admin::run_helper("set-keyboard", &args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_default_file_and_the_layout_list() {
        let f = "XKBMODEL=\"pc105\"\nXKBLAYOUT=\"ch,us\"\nXKBVARIANT=\"fr\"\n";
        assert_eq!(parse_default(f), ("ch".into(), "fr".into()));
        assert_eq!(parse_default(""), ("us".into(), "".into()));
        let lst = "! model\n  pc105  Generic\n! layout\n  us              English (US)\n  de              German\n! variant\n  chr  us: Cherokee\n";
        assert_eq!(
            parse_section(lst, "layout"),
            vec![
                ("us".to_string(), "English (US)".to_string()),
                ("de".to_string(), "German".to_string())
            ]
        );
    }
}

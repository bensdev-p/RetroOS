//! The person's desktop preferences (Control Panel → Mouse and Desktop):
//! `~/.config/retroos/desktop.conf`, `key=value` lines. Station reads them at
//! start and again on the broker's `PREFS` request (the Control Panel sends
//! it after saving); retro-ui programs read the double-click time at start.

use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefs {
    /// -2 (slowest) to 2 (fastest); libinput's pointer acceleration.
    pub pointer_speed: i32,
    /// Swap the left and right buttons.
    pub left_handed: bool,
    /// Two clicks closer than this are a double-click.
    pub double_click_ms: u32,
    /// A key of [`PATTERNS`].
    pub pattern: String,
}

impl Default for Prefs {
    fn default() -> Prefs {
        Prefs {
            pointer_speed: 0,
            left_handed: false,
            double_click_ms: 500,
            pattern: "check".into(),
        }
    }
}

/// Desktop patterns (STYLE.md §4: 8×8, two colours of the scheme): key,
/// name, and eight rows of eight bits, a set bit in the scheme's first
/// desktop colour (Cobalt in Phosphor Classic), a clear one in the second.
pub const PATTERNS: &[(&str, &str, [u8; 8])] = &[
    (
        "check",
        "Checkerboard",
        [0xAA, 0x55, 0xAA, 0x55, 0xAA, 0x55, 0xAA, 0x55],
    ),
    (
        "dots",
        "Dots",
        [0xFF, 0xFF, 0xF7, 0xFF, 0xFF, 0xFF, 0x7F, 0xFF],
    ),
    (
        "diagonal",
        "Diagonal",
        [0xEE, 0xDD, 0xBB, 0x77, 0xEE, 0xDD, 0xBB, 0x77],
    ),
    (
        "bricks",
        "Bricks",
        [0x00, 0xFE, 0xFE, 0xFE, 0x00, 0xEF, 0xEF, 0xEF],
    ),
    (
        "grid",
        "Grid",
        [0x00, 0x7F, 0x7F, 0x7F, 0x7F, 0x7F, 0x7F, 0x7F],
    ),
    ("plain", "Plain", [0xFF; 8]),
];

/// A pattern's rows (the checkerboard for an unknown key).
pub fn pattern(key: &str) -> [u8; 8] {
    PATTERNS
        .iter()
        .find(|(k, _, _)| *k == key)
        .map_or(PATTERNS[0].2, |p| p.2)
}

/// Whether pixel (`x`, `y`) of a pattern (in pattern pixels) is the first
/// colour.
pub fn pattern_bit(rows: &[u8; 8], x: i32, y: i32) -> bool {
    let row = rows[y.rem_euclid(8) as usize];
    (row >> (7 - x.rem_euclid(8))) & 1 == 1
}

pub fn path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        });
    base.join("retroos/desktop.conf")
}

pub fn parse(text: &str) -> Prefs {
    let mut p = Prefs::default();
    for line in text.lines() {
        let Some((k, v)) = line.split('#').next().unwrap_or("").split_once('=') else {
            continue;
        };
        let v = v.trim();
        match k.trim() {
            "pointer_speed" => p.pointer_speed = v.parse::<i32>().unwrap_or(0).clamp(-2, 2),
            "left_handed" => p.left_handed = matches!(v, "yes" | "true" | "1" | "on"),
            "double_click_ms" => {
                p.double_click_ms = v.parse::<u32>().unwrap_or(500).clamp(200, 1000)
            }
            "pattern" if PATTERNS.iter().any(|(k, _, _)| *k == v) => p.pattern = v.into(),
            _ => {}
        }
    }
    p
}

pub fn load() -> Prefs {
    parse(&std::fs::read_to_string(path()).unwrap_or_default())
}

pub fn save(p: &Prefs) -> std::io::Result<()> {
    let path = path();
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let body = format!(
        "# RetroOS desktop preferences (Control Panel: Mouse, Desktop).\n\
         pointer_speed={}\nleft_handed={}\ndouble_click_ms={}\npattern={}\n",
        p.pointer_speed,
        if p.left_handed { "yes" } else { "no" },
        p.double_click_ms,
        p.pattern
    );
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(tmp, path)
}

/// The double-click time, for programs that count clicks themselves.
pub fn double_click() -> Duration {
    Duration::from_millis(load().double_click_ms as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_what_it_writes() {
        let p = parse("pointer_speed=5\nleft_handed=yes\ndouble_click_ms=350\npattern=diagonal\n");
        assert_eq!(
            p,
            Prefs {
                pointer_speed: 2,
                left_handed: true,
                double_click_ms: 350,
                pattern: "diagonal".into()
            }
        );
        assert_eq!(parse("pattern=paisley\n").pattern, "check");
        assert_eq!(parse(""), Prefs::default());
    }

    #[test]
    fn the_checkerboard_alternates() {
        let c = pattern("check");
        assert!(pattern_bit(&c, 0, 0) && !pattern_bit(&c, 1, 0) && !pattern_bit(&c, 0, 1));
        assert!(pattern_bit(&c, 8, 8), "repeats every 8");
        assert!(pattern_bit(&pattern("plain"), 3, 5));
    }
}

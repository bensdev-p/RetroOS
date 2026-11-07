//! Screen settings that are not in startup.conf: the brightness of a panel
//! with a backlight (the Touch Display 2), set through logind so the person
//! at the screen needs no special rights.

use std::path::{Path, PathBuf};
use zbus::blocking::{Connection, Proxy};

const BACKLIGHT: &str = "/sys/class/backlight";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Backlight {
    pub name: String,
    pub level: u32,
    pub max: u32,
}

impl Backlight {
    /// 0–100.
    pub fn percent(&self) -> u32 {
        (self.level * 100 + self.max / 2) / self.max.max(1)
    }
}

fn read_u32(p: &Path) -> Option<u32> {
    std::fs::read_to_string(p).ok()?.trim().parse().ok()
}

/// The first backlight (None on HDMI monitors).
pub fn backlight() -> Option<Backlight> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(BACKLIGHT)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    dirs.sort();
    dirs.into_iter().find_map(|d| {
        Some(Backlight {
            name: d.file_name()?.to_string_lossy().into_owned(),
            level: read_u32(&d.join("brightness"))?,
            max: read_u32(&d.join("max_brightness")).filter(|m| *m > 0)?,
        })
    })
}

/// Set the backlight to `percent` (at least 5 %, so the screen never goes
/// fully dark from a slider).
pub fn set_brightness(b: &Backlight, percent: u32) -> Result<(), String> {
    let level = (b.max * percent.clamp(5, 100) + 50) / 100;
    let conn = Connection::system().map_err(|e| e.to_string())?;
    let p = Proxy::new(
        &conn,
        "org.freedesktop.login1",
        "/org/freedesktop/login1/session/auto",
        "org.freedesktop.login1.Session",
    )
    .map_err(|e| e.to_string())?;
    p.call::<_, _, ()>(
        "SetBrightness",
        &("backlight", b.name.as_str(), level.max(1)),
    )
    .map_err(|e| {
        let s = e.to_string();
        if s.contains("AccessDenied") || s.contains("NotInControl") {
            "Only the person at the screen can change its brightness.".into()
        } else {
            s
        }
    })
}

/// The colour schemes (startup.conf `scheme=`) and their names.
pub const SCHEMES: &[(&str, &str)] = &[
    ("classic", "Classic"),
    ("paper", "Paper"),
    ("amber", "Amber"),
    ("green", "Green"),
    ("high", "High Contrast"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_rounds() {
        let b = Backlight {
            name: "11-0045".into(),
            level: 31,
            max: 31,
        };
        assert_eq!(b.percent(), 100);
        assert_eq!(Backlight { level: 15, ..b }.percent(), 48);
    }
}

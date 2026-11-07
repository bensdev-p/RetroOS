//! Which way up the screen is (docs/DECISIONS.md D-042).
//!
//! `rotate=` in /etc/retroos/startup.conf is how far the picture is turned
//! **clockwise** on the panel: 0, 90, 180 or 270. `retroos.rotate=` on the
//! kernel command line (cmdline.txt on the card's boot partition, editable
//! from any PC when the screen is unreadable) overrides it, and
//! $RETROOS_ROTATE overrides both (tests). `auto`
//! (the default) leaves landscape screens alone and turns a portrait panel
//! (the Raspberry Pi Touch Display 2 is 720×1280) a quarter turn so RetroOS
//! is landscape; if it comes out upside down, `rotate=270`. Station (the
//! desktop and the prompt) and the POST screen (retro-kms) both use this.

pub const STARTUP_CONF: &str = "/etc/retroos/startup.conf";

/// The `key=` setting from startup.conf text (comments after `#` ignored).
pub fn conf_value(conf: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    conf.lines().find_map(|l| {
        l.split('#')
            .next()?
            .trim()
            .strip_prefix(prefix.as_str())
            .map(|v| v.trim().to_string())
    })
}

/// Degrees clockwise for a panel whose native mode is `w`×`h`, given the
/// setting's value (None or anything unrecognised = auto). `flip`, `right`
/// and `left` turn the picture from where `auto` puts it (upside down, a
/// quarter turn clockwise, counter-clockwise), so they mean the same on a
/// landscape monitor and on a portrait panel shown landscape (D-044); the
/// numbers are the panel's own turns, for cmdline.txt.
pub fn rotation_for(setting: Option<&str>, w: u32, h: u32) -> u32 {
    let auto = if h > w { 90 } else { 0 };
    match setting.map(str::trim) {
        Some("0") => 0,
        Some("90") => 90,
        Some("180") => 180,
        Some("270") => 270,
        Some("flip") => (auto + 180) % 360,
        Some("right") => (auto + 90) % 360,
        Some("left") => (auto + 270) % 360,
        _ => auto,
    }
}

/// The system's rotation for a panel with native mode `w`×`h`.
pub fn rotation(w: u32, h: u32) -> u32 {
    let set = std::env::var("RETROOS_ROTATE")
        .ok()
        .or_else(|| {
            std::fs::read_to_string("/proc/cmdline")
                .ok()
                .and_then(|c| cmdline_value(&c, "retroos.rotate"))
        })
        .or_else(|| {
            std::fs::read_to_string(STARTUP_CONF)
                .ok()
                .and_then(|c| conf_value(&c, "rotate"))
        });
    rotation_for(set.as_deref(), w, h)
}

/// `key=value` from a kernel command line.
pub fn cmdline_value(cmdline: &str, key: &str) -> Option<String> {
    cmdline
        .split_whitespace()
        .find_map(|w| w.strip_prefix(key)?.strip_prefix('=').map(String::from))
}

/// The size of the picture (what programs and the canvas see) on a panel of
/// native size `w`×`h` turned `deg` clockwise.
pub fn logical_size(w: u32, h: u32, deg: u32) -> (u32, u32) {
    if deg == 90 || deg == 270 {
        (h, w)
    } else {
        (w, h)
    }
}

/// Where the picture's pixel (x, y) lands on the panel: the picture is
/// `w`×`h` (logical), turned `deg` clockwise.
pub fn to_panel(x: u32, y: u32, w: u32, h: u32, deg: u32) -> (u32, u32) {
    match deg {
        90 => (h - 1 - y, x),
        180 => (w - 1 - x, h - 1 - y),
        270 => (y, w - 1 - x),
        _ => (x, y),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_turns_portrait_panels_only() {
        assert_eq!(rotation_for(None, 1920, 1080), 0);
        assert_eq!(rotation_for(None, 720, 1280), 90);
        assert_eq!(rotation_for(Some("auto"), 720, 1280), 90);
        assert_eq!(rotation_for(Some("270"), 720, 1280), 270);
        assert_eq!(rotation_for(Some("0"), 720, 1280), 0);
        assert_eq!(rotation_for(Some("180"), 1920, 1080), 180);
        assert_eq!(rotation_for(Some("sideways"), 1920, 1080), 0);
        // Relative to auto: upside down stays landscape on a portrait panel.
        assert_eq!(rotation_for(Some("flip"), 1200, 1920), 270);
        assert_eq!(rotation_for(Some("flip"), 1920, 1080), 180);
        assert_eq!(rotation_for(Some("right"), 1920, 1080), 90);
        assert_eq!(rotation_for(Some("left"), 1200, 1920), 0);
    }

    #[test]
    fn conf_values() {
        let c = "mode=text\nrotate=270   # Touch Display 2, cable on the left\n# rotate=90\n";
        assert_eq!(conf_value(c, "rotate").as_deref(), Some("270"));
        assert_eq!(conf_value(c, "mode").as_deref(), Some("text"));
        assert_eq!(conf_value(c, "scheme"), None);
    }

    #[test]
    fn kernel_command_line() {
        let c = "console=tty1 root=PARTUUID=52455452-02 retroos.rotate=270 quiet\n";
        assert_eq!(cmdline_value(c, "retroos.rotate").as_deref(), Some("270"));
        assert_eq!(cmdline_value("quiet", "retroos.rotate"), None);
    }

    #[test]
    fn quarter_turn_clockwise() {
        // A 1280×720 picture on a 720×1280 panel: its top-left corner goes
        // to the panel's top-right, its top-right to the bottom-right.
        assert_eq!(logical_size(720, 1280, 90), (1280, 720));
        assert_eq!(to_panel(0, 0, 1280, 720, 90), (719, 0));
        assert_eq!(to_panel(1279, 0, 1280, 720, 90), (719, 1279));
        assert_eq!(to_panel(0, 0, 1280, 720, 270), (0, 1279));
        assert_eq!(to_panel(0, 0, 4, 3, 180), (3, 2));
        assert_eq!(to_panel(2, 1, 4, 3, 0), (2, 1));
    }
}

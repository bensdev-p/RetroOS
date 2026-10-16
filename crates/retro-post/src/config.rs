//! /etc/retroos/startup.conf — the startup settings (Setup / Control Panel → Startup).

use std::path::Path;

pub const PATH: &str = "/etc/retroos/startup.conf";
/// One-shot override written by the Startup Menu, read by retro-session.
pub const CHOICE: &str = "/run/retroos/startup-choice";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// "text" (RetroDOS) or "desktop".
    pub mode: String,
    pub fast_post: bool,
    pub chime: bool,
    pub menu_timeout: u32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            mode: "text".into(),
            fast_post: true,
            chime: true,
            menu_timeout: 30,
        }
    }
}

fn yes(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "yes" | "on" | "true" | "1"
    )
}

impl Config {
    pub fn load(path: &Path) -> Config {
        let mut c = Config::default();
        let Ok(s) = std::fs::read_to_string(path) else {
            return c;
        };
        for line in s.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let v = v.trim();
            match k.trim() {
                "mode" if v == "text" || v == "desktop" => c.mode = v.into(),
                "fast_post" => c.fast_post = yes(v),
                "chime" => c.chime = yes(v),
                "menu_timeout" => c.menu_timeout = v.parse().unwrap_or(30).clamp(1, 600),
                _ => {}
            }
        }
        c
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let b = |v: bool| if v { "yes" } else { "no" };
        let body = format!(
            "# RetroOS startup settings. Edit with Setup (DEL at boot) or Control Panel.\n\
             mode={}          # text = RetroDOS prompt, desktop = RetroOS Desktop\n\
             fast_post={}\n\
             chime={}\n\
             menu_timeout={}\n",
            self.mode,
            b(self.fast_post),
            b(self.chime),
            self.menu_timeout
        );
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, body)?;
        std::fs::rename(tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let p = std::env::temp_dir().join(format!("retro-startup-{}.conf", std::process::id()));
        let c = Config {
            mode: "desktop".into(),
            fast_post: false,
            chime: true,
            menu_timeout: 10,
        };
        c.save(&p).unwrap();
        assert_eq!(Config::load(&p), c);
        std::fs::write(&p, "mode=bogus\nchime=off\n").unwrap();
        let d = Config::load(&p);
        assert_eq!(d.mode, "text");
        assert!(!d.chime);
    }
}

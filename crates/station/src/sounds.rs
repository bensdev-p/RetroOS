//! Interface sounds (docs/STYLE.md §6, D-053): Station plays them for
//! everyone (`SOUND <name>` on the broker) so that only one sounds at a
//! time and the person's setting is honoured in one place. They go through
//! PipeWire (`pw-play`), so they follow the volume and the output in use.

use crate::state::Station;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The sounds there are (retro-synth writes them at build time).
pub const SOUNDS: &[&str] = &[
    "startup", "shutdown", "beep", "alert", "error", "notify", "crumple",
];

#[derive(Default)]
pub struct Player {
    child: Option<Child>,
    last: Option<(String, Instant)>,
    /// The last sound asked for and wanted (the broker's LASTSOUND, for
    /// host tests, which have no sound files or PipeWire).
    pub wanted: Option<String>,
}

fn dir() -> std::path::PathBuf {
    std::env::var_os("RETROOS_SOUNDS")
        .map(Into::into)
        .unwrap_or_else(|| "/usr/share/retroos/sounds".into())
}

/// The person's choice: all, essential or off (~/.config/retroos/sound.conf,
/// the file retro_sys::sound writes).
fn level() -> String {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        });
    std::fs::read_to_string(base.join("retroos/sound.conf"))
        .unwrap_or_default()
        .lines()
        .find_map(|l| {
            l.trim()
                .strip_prefix("ui_sounds=")
                .map(|v| v.trim().to_string())
        })
        .unwrap_or_else(|| "all".into())
}

/// Whether `sound` plays at `level` (Essential: the chimes and errors).
pub fn plays(level: &str, sound: &str) -> bool {
    match level {
        "off" => false,
        "essential" => matches!(sound, "error" | "startup" | "shutdown"),
        _ => true,
    }
}

impl Station {
    /// Play an interface sound; false if unknown or not wanted.
    pub fn play_sound(&mut self, name: &str) -> bool {
        if !SOUNDS.contains(&name) || !plays(&level(), name) {
            return false;
        }
        let p = &mut self.sounds;
        p.wanted = Some(name.to_string());
        // A burst of the same sound (a program printing BEL in a loop) is one.
        if p.last
            .as_ref()
            .is_some_and(|(n, t)| n == name && t.elapsed() < Duration::from_millis(150))
        {
            return true;
        }
        // Never more than one at a time: the new one cuts the old.
        if let Some(mut c) = p.child.take() {
            if c.try_wait().ok().flatten().is_none() {
                let _ = c.kill();
            }
            let _ = c.wait();
        }
        let file = dir().join(format!("{name}.wav"));
        if !file.exists() {
            return false;
        }
        p.last = Some((name.to_string(), Instant::now()));
        p.child = Command::new("pw-play")
            .arg(&file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok();
        p.child.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn essential_is_chimes_and_errors() {
        assert!(plays("all", "beep"));
        assert!(!plays("essential", "beep"));
        assert!(plays("essential", "error"));
        assert!(plays("essential", "shutdown"));
        assert!(!plays("off", "error"));
    }
}

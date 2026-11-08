//! The startup chime (docs/STYLE.md §6), played through ALSA before any
//! sound server runs (D-049).
//!
//! On the Pi the sound goes out over HDMI, and the HDMI audio device plays
//! only while the display link is up, so the chime starts once the POST
//! screen is lit and the monitor has had a moment to sync, on the sound card
//! of the HDMI port in use (`vc4hdmi0` for HDMI-A-1, `vc4hdmi1` for
//! HDMI-A-2; `sysdefault:` lets alsa-lib's card file convert to the IEC958
//! samples those cards take). The POST screen waits for it to finish before
//! it exits, since systemd ends whatever a finished service left running.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub const SOUND: &str = "/usr/share/retroos/sounds/startup.wav";
/// How long after the display is lit the chime starts (monitors re-sync).
pub const DELAY: Duration = Duration::from_millis(800);
/// How long to wait for the sound card to appear.
const CARD_WAIT: Duration = Duration::from_secs(5);

/// The ALSA card id for an HDMI connector name ("HDMI-A-2" → "vc4hdmi1").
fn hdmi_card(connector: &str) -> Option<String> {
    let n: u32 = connector.strip_prefix("HDMI-A-")?.parse().ok()?;
    Some(format!("vc4hdmi{}", n.checked_sub(1)?))
}

/// The card to play on: the HDMI port the picture is on; for a DSI panel
/// (no sound) any connected HDMI port; else None (card 0).
pub fn card_for(connector: &str, drm: &Path) -> Option<String> {
    if let Some(c) = hdmi_card(connector) {
        return Some(c);
    }
    let mut ports: Vec<String> = std::fs::read_dir(drm)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let conn = name.split_once('-')?.1.to_string(); // "card1-HDMI-A-1"
            let status = std::fs::read_to_string(e.path().join("status")).ok()?;
            (status.trim() == "connected" && conn.starts_with("HDMI-A-")).then_some(conn)
        })
        .collect();
    ports.sort();
    ports.first().and_then(|c| hdmi_card(c))
}

/// The ALSA device name for `card`.
pub fn device(card: Option<&str>) -> String {
    match card {
        Some(c) => format!("sysdefault:CARD={c}"),
        None => "sysdefault".into(),
    }
}

/// Whether the card exists yet (ALSA's /proc/asound/<id> link, or card 0).
fn card_ready(card: Option<&str>) -> bool {
    match card {
        Some(c) => Path::new("/proc/asound").join(c).exists(),
        None => Path::new("/proc/asound/card0").exists(),
    }
}

/// A chime waiting for its card, then playing.
pub struct Chime {
    card: Option<String>,
    since: Instant,
    child: Option<Child>,
    given_up: bool,
}

impl Chime {
    pub fn new(connector: &str) -> Chime {
        Chime {
            card: card_for(connector, Path::new("/sys/class/drm")),
            since: Instant::now(),
            child: None,
            given_up: false,
        }
    }

    /// Playing (or given up).
    pub fn started(&self) -> bool {
        self.child.is_some() || self.given_up
    }

    /// Call every frame: starts playing once the card is there.
    pub fn poll(&mut self, log: &dyn Fn(&str)) {
        if self.child.is_some() || self.given_up || !Path::new(SOUND).exists() {
            return;
        }
        if !card_ready(self.card.as_deref()) {
            if self.since.elapsed() > CARD_WAIT {
                self.given_up = true;
                log(&format!(
                    "RETRO-POST: chime: no sound card {} appeared",
                    self.card.as_deref().unwrap_or("card0")
                ));
            }
            return;
        }
        let dev = device(self.card.as_deref());
        match Command::new("aplay")
            .args(["-q", "-D", &dev, SOUND])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => {
                log(&format!("RETRO-POST: chime on {dev}"));
                self.child = Some(c);
            }
            Err(e) => {
                self.given_up = true;
                log(&format!("RETRO-POST: chime: cannot run aplay: {e}"));
            }
        }
    }

    /// Let the chime finish (at most `max`), and log how it went.
    pub fn finish(mut self, max: Duration, log: &dyn Fn(&str)) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        let until = Instant::now() + max;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        let mut err = String::new();
                        if let Some(mut e) = child.stderr.take() {
                            use std::io::Read;
                            let _ = e.read_to_string(&mut err);
                        }
                        let err = err.trim().replace('\n', " / ");
                        log(&format!("RETRO-POST: chime failed ({status}): {err}"));
                    }
                    return;
                }
                Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(20)),
                _ => {
                    let _ = child.kill();
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hdmi_ports_map_to_their_cards() {
        assert_eq!(hdmi_card("HDMI-A-1").as_deref(), Some("vc4hdmi0"));
        assert_eq!(hdmi_card("HDMI-A-2").as_deref(), Some("vc4hdmi1"));
        assert_eq!(hdmi_card("DSI-1"), None);
        assert_eq!(hdmi_card("HDMI-A-0"), None);
        assert_eq!(device(Some("vc4hdmi0")), "sysdefault:CARD=vc4hdmi0");
        assert_eq!(device(None), "sysdefault");
    }

    #[test]
    fn a_panel_plays_on_a_connected_hdmi_port() {
        let d = std::env::temp_dir().join(format!("retro-chime-{}", std::process::id()));
        for (name, status) in [
            ("card1-DSI-2", "connected"),
            ("card2-HDMI-A-1", "disconnected"),
            ("card2-HDMI-A-2", "connected"),
        ] {
            std::fs::create_dir_all(d.join(name)).unwrap();
            std::fs::write(d.join(name).join("status"), format!("{status}\n")).unwrap();
        }
        assert_eq!(card_for("DSI-2", &d).as_deref(), Some("vc4hdmi1"));
        assert_eq!(card_for("HDMI-A-1", &d).as_deref(), Some("vc4hdmi0"));
        std::fs::remove_dir_all(&d).unwrap();
        assert_eq!(card_for("DSI-1", &d), None);
    }
}

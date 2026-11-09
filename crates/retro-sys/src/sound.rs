//! Sound through PipeWire and WirePlumber (DESIGN.md §10.3, D-047): the
//! outputs (HDMI, USB, Bluetooth), which one is in use, their volume and
//! mute, and the programs playing. PipeWire has no D-Bus interface, so this
//! reads its state with `pw-dump` (JSON) and changes it with `wpctl`, both
//! part of the image; volumes use WirePlumber's cubic scale, as `wpctl` and
//! every PipeWire mixer show them.
//!
//! `$RETROOS_TEST_SOUND` replaces PipeWire with a pretend set of devices
//! (host tests, which have no sound server).

use std::process::Command;

/// A sound output (or a program's stream).
#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    /// PipeWire's object id (what `wpctl` takes).
    pub id: u32,
    /// What people see: "Built-in Audio Digital Stereo (HDMI)", "Firefox".
    pub name: String,
    /// 0.0–1.0 (WirePlumber's cubic scale; 1.0 = full, no boost).
    pub volume: f32,
    pub muted: bool,
    /// The output in use (outputs only).
    pub default: bool,
}

impl Channel {
    pub fn percent(&self) -> u32 {
        (self.volume.clamp(0.0, 1.5) * 100.0).round() as u32
    }
}

/// Everything a mixer shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mixer {
    pub outputs: Vec<Channel>,
    /// Programs playing sound now.
    pub streams: Vec<Channel>,
}

impl Mixer {
    /// The output in use (else the first).
    pub fn output(&self) -> Option<&Channel> {
        self.outputs
            .iter()
            .find(|c| c.default)
            .or(self.outputs.first())
    }
}

fn friendly(e: std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        "The sound service is not installed.".into()
    } else {
        e.to_string()
    }
}

/// The current state.
pub fn mixer() -> Result<Mixer, String> {
    if std::env::var_os("RETROOS_TEST_SOUND").is_some() {
        return Ok(fake::state());
    }
    let out = Command::new("pw-dump")
        .arg("--no-colors")
        .output()
        .map_err(friendly)?;
    if !out.status.success() {
        return Err("The sound service is not running.".into());
    }
    let mut m = parse_dump(&String::from_utf8_lossy(&out.stdout));
    // An output's volume and mute as WirePlumber keeps them: on a sound
    // card it can hold them on the card's route rather than the node's own
    // properties, which then lag or disagree (HW 4.22: a mute seemed to
    // undo itself). wpctl reads them the way set_volume/set_mute write them.
    for o in m.outputs.iter_mut() {
        if let Some((v, muted)) = Command::new("wpctl")
            .args(["get-volume", &o.id.to_string()])
            .output()
            .ok()
            .and_then(|r| parse_get_volume(&String::from_utf8_lossy(&r.stdout)))
        {
            o.volume = v;
            o.muted = muted;
        }
    }
    Ok(m)
}

/// `wpctl get-volume`'s answer: "Volume: 0.40" or "Volume: 0.40 [MUTED]".
pub fn parse_get_volume(s: &str) -> Option<(f32, bool)> {
    let rest = s.trim().strip_prefix("Volume:")?.trim();
    let v: f32 = rest.split_whitespace().next()?.parse().ok()?;
    Some((v.clamp(0.0, 1.5), rest.contains("[MUTED]")))
}

/// Read `pw-dump`'s JSON.
pub fn parse_dump(json: &str) -> Mixer {
    let Ok(serde_json::Value::Array(objs)) = serde_json::from_str::<serde_json::Value>(json) else {
        return Mixer::default();
    };
    // The default sink's node name, from the "default" metadata.
    let default_sink = objs
        .iter()
        .filter(|o| o["type"] == "PipeWire:Interface:Metadata")
        .filter(|o| o["props"]["metadata.name"] == "default")
        .flat_map(|o| o["metadata"].as_array().cloned().unwrap_or_default())
        .find(|m| m["key"] == "default.audio.sink")
        .and_then(|m| match &m["value"] {
            serde_json::Value::Object(v) => {
                v.get("name").and_then(|n| n.as_str()).map(String::from)
            }
            serde_json::Value::String(s) => serde_json::from_str::<serde_json::Value>(s)
                .ok()
                .and_then(|v| v["name"].as_str().map(String::from)),
            _ => None,
        });
    let mut mixer = Mixer::default();
    for o in &objs {
        if o["type"] != "PipeWire:Interface:Node" {
            continue;
        }
        let props = &o["info"]["props"];
        let class = props["media.class"].as_str().unwrap_or("");
        let id = o["id"].as_u64().unwrap_or(0) as u32;
        // Volume and mute: the Props entry with channel volumes.
        let p = o["info"]["params"]["Props"]
            .as_array()
            .and_then(|a| {
                a.iter()
                    .find(|p| p.get("channelVolumes").is_some())
                    .cloned()
            })
            .unwrap_or_default();
        let vols: Vec<f64> = p["channelVolumes"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_f64()).collect())
            .unwrap_or_default();
        let linear = if vols.is_empty() {
            1.0
        } else {
            vols.iter().sum::<f64>() / vols.len() as f64
        };
        let volume = linear.max(0.0).cbrt() as f32;
        let muted = p["mute"].as_bool().unwrap_or(false);
        let s = |k: &str| props[k].as_str().unwrap_or("").to_string();
        match class {
            "Audio/Sink" => {
                let name = s("node.name");
                let desc = s("node.description");
                mixer.outputs.push(Channel {
                    id,
                    default: default_sink.as_deref() == Some(name.as_str()),
                    name: if desc.is_empty() { name } else { desc },
                    volume,
                    muted,
                });
            }
            "Stream/Output/Audio" => {
                let mut name = s("application.name");
                if name.is_empty() {
                    name = s("node.name");
                }
                mixer.streams.push(Channel {
                    id,
                    name,
                    volume,
                    muted,
                    default: false,
                });
            }
            _ => {}
        }
    }
    mixer.outputs.sort_by(|a, b| a.name.cmp(&b.name));
    mixer
}

fn wpctl(args: &[&str]) -> Result<(), String> {
    let out = Command::new("wpctl")
        .args(args)
        .output()
        .map_err(friendly)?;
    if out.status.success() {
        Ok(())
    } else {
        let e = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if e.is_empty() {
            "The sound service did not accept the change.".into()
        } else {
            e
        })
    }
}

/// Set a channel's volume (0.0–1.0).
pub fn set_volume(id: u32, volume: f32) -> Result<(), String> {
    let v = volume.clamp(0.0, 1.0);
    if std::env::var_os("RETROOS_TEST_SOUND").is_some() {
        fake::change(id, |c| c.volume = v);
        return Ok(());
    }
    wpctl(&["set-volume", &id.to_string(), &format!("{v:.2}")])
}

pub fn set_mute(id: u32, muted: bool) -> Result<(), String> {
    if std::env::var_os("RETROOS_TEST_SOUND").is_some() {
        fake::change(id, |c| c.muted = muted);
        return Ok(());
    }
    wpctl(&["set-mute", &id.to_string(), if muted { "1" } else { "0" }])
}

/// Use this output for sound from now on.
pub fn set_default(id: u32) -> Result<(), String> {
    if std::env::var_os("RETROOS_TEST_SOUND").is_some() {
        let mut m = fake::STATE.lock().unwrap_or_else(|e| e.into_inner());
        let st = m.get_or_insert_with(fake::initial);
        for c in st.outputs.iter_mut() {
            c.default = c.id == id;
        }
        return Ok(());
    }
    wpctl(&["set-default", &id.to_string()])
}

/// Interface sounds (STYLE.md §6): Off, Essential (the chime and errors),
/// All. A per-user setting in ~/.config/retroos/sound.conf.
pub const UI_SOUNDS: &[(&str, &str)] = &[
    ("all", "All"),
    ("essential", "Essential (errors only)"),
    ("off", "Off"),
];

fn conf_path() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        });
    base.join("retroos/sound.conf")
}

/// "all", "essential" or "off".
pub fn ui_sounds() -> String {
    std::fs::read_to_string(conf_path())
        .unwrap_or_default()
        .lines()
        .find_map(|l| {
            l.trim()
                .strip_prefix("ui_sounds=")
                .map(|v| v.trim().to_string())
        })
        .filter(|v| UI_SOUNDS.iter().any(|(k, _)| k == v))
        .unwrap_or_else(|| "all".into())
}

pub fn set_ui_sounds(level: &str) -> Result<(), String> {
    if !UI_SOUNDS.iter().any(|(k, _)| *k == level) {
        return Err("Unknown setting.".into());
    }
    let p = conf_path();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        &p,
        format!("# RetroOS interface sounds: all, essential, off\nui_sounds={level}\n"),
    )
    .map_err(|e| e.to_string())
}

/// Whether a sound of this kind plays at `level` (errors are essential).
pub fn plays(level: &str, sound: &str) -> bool {
    match level {
        "off" => false,
        "essential" => matches!(sound, "error" | "startup" | "shutdown"),
        _ => true,
    }
}

/// The pretend sound system for host tests.
mod fake {
    use super::{Channel, Mixer};
    use std::sync::Mutex;

    pub static STATE: Mutex<Option<Mixer>> = Mutex::new(None);

    pub fn initial() -> Mixer {
        let c = |id, name: &str, volume, default| Channel {
            id,
            name: name.into(),
            volume,
            muted: false,
            default,
        };
        Mixer {
            outputs: vec![
                c(45, "Built-in Audio Digital Stereo (HDMI)", 0.70, true),
                c(52, "USB Audio Analog Stereo", 0.40, false),
            ],
            streams: vec![
                c(80, "Firefox", 1.0, false),
                c(81, "RetroArch", 0.55, false),
            ],
        }
    }

    pub fn state() -> Mixer {
        let mut m = STATE.lock().unwrap_or_else(|e| e.into_inner());
        m.get_or_insert_with(initial).clone()
    }

    pub fn change(id: u32, f: impl FnOnce(&mut Channel)) {
        let mut m = STATE.lock().unwrap_or_else(|e| e.into_inner());
        let st = m.get_or_insert_with(initial);
        if let Some(c) = st
            .outputs
            .iter_mut()
            .chain(st.streams.iter_mut())
            .find(|c| c.id == id)
        {
            f(c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUMP: &str = r#"[
      {"id": 0, "type": "PipeWire:Interface:Core", "info": {}},
      {"id": 38, "type": "PipeWire:Interface:Metadata", "props": {"metadata.name": "default"},
       "metadata": [{"subject": 0, "key": "default.audio.sink", "type": "Spa:String:JSON",
                     "value": {"name": "alsa_output.usb"}}]},
      {"id": 45, "type": "PipeWire:Interface:Node", "info": {
         "props": {"media.class": "Audio/Sink", "node.name": "alsa_output.hdmi",
                   "node.description": "Built-in Audio Digital Stereo (HDMI)"},
         "params": {"Props": [{"volume": 1.0}, {"mute": false, "channelVolumes": [0.064, 0.064]}]}}},
      {"id": 52, "type": "PipeWire:Interface:Node", "info": {
         "props": {"media.class": "Audio/Sink", "node.name": "alsa_output.usb",
                   "node.description": "USB Audio"},
         "params": {"Props": [{"mute": true, "channelVolumes": [1.0, 1.0]}]}}},
      {"id": 80, "type": "PipeWire:Interface:Node", "info": {
         "props": {"media.class": "Stream/Output/Audio", "application.name": "Firefox"},
         "params": {"Props": [{"mute": false, "channelVolumes": [0.125]}]}}},
      {"id": 90, "type": "PipeWire:Interface:Node", "info": {
         "props": {"media.class": "Audio/Source", "node.name": "mic"}}}
    ]"#;

    #[test]
    fn reads_outputs_streams_and_the_default() {
        let m = parse_dump(DUMP);
        assert_eq!(m.outputs.len(), 2);
        let hdmi = &m.outputs[0];
        assert_eq!(hdmi.name, "Built-in Audio Digital Stereo (HDMI)");
        assert_eq!(hdmi.percent(), 40, "cubic scale: 0.064 is 40 %");
        assert!(!hdmi.default);
        assert!(m.outputs[1].default && m.outputs[1].muted);
        assert_eq!(m.output().unwrap().id, 52);
        assert_eq!(m.streams.len(), 1);
        assert_eq!(
            (m.streams[0].name.as_str(), m.streams[0].percent()),
            ("Firefox", 50)
        );
        assert_eq!(parse_dump("not json"), Mixer::default());
    }

    #[test]
    fn wpctl_volume_lines() {
        assert_eq!(parse_get_volume("Volume: 0.40\n"), Some((0.4, false)));
        assert_eq!(
            parse_get_volume("Volume: 1.00 [MUTED]\n"),
            Some((1.0, true))
        );
        assert_eq!(parse_get_volume("Object not found\n"), None);
    }

    #[test]
    fn essential_sounds() {
        assert!(plays("all", "beep"));
        assert!(!plays("essential", "beep"));
        assert!(plays("essential", "error"));
        assert!(!plays("off", "error"));
    }
}

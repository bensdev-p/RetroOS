//! The player behind the Media Player (D-057): **mpv**, run as its own
//! process and driven over its JSON IPC socket. mpv plays with the GPU and
//! the Pi's video decoder and shows video in its own window (with
//! Station's frame), as Windows 3.1's Media Player showed video in a
//! separate MCI window; audio plays without a window. The front-end only
//! sends commands and watches a few properties.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// What the player is doing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    /// The file loaded ("" when none).
    pub path: String,
    pub title: String,
    /// Seconds.
    pub pos: f64,
    pub duration: f64,
    pub paused: bool,
    /// Nothing loaded (or stopped).
    pub idle: bool,
    /// 0–100.
    pub volume: f64,
    /// The last error ("Cannot play …").
    pub error: Option<String>,
}

pub trait Player {
    fn load(&mut self, path: &Path);
    fn set_pause(&mut self, paused: bool);
    fn stop(&mut self);
    /// To `secs` from the start.
    fn seek(&mut self, secs: f64);
    fn set_volume(&mut self, v: f64);
    /// Read what the player said since; the state now.
    fn poll(&mut self) -> &State;
}

/// "1:23" or "1:02:03".
pub fn clock(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// One line mpv wrote on its socket, applied to `st`.
pub fn apply_event(st: &mut State, line: &str) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return;
    };
    match v["event"].as_str() {
        Some("property-change") => {
            let d = &v["data"];
            match v["name"].as_str() {
                Some("time-pos") => st.pos = d.as_f64().unwrap_or(0.0),
                Some("duration") => st.duration = d.as_f64().unwrap_or(0.0),
                Some("pause") => st.paused = d.as_bool().unwrap_or(false),
                Some("idle-active") => st.idle = d.as_bool().unwrap_or(false),
                Some("volume") => st.volume = d.as_f64().unwrap_or(st.volume),
                Some("media-title") => st.title = d.as_str().unwrap_or("").to_string(),
                // After a failed load mpv says null: keep the name tried.
                Some("path") => {
                    if let Some(p) = d.as_str() {
                        st.path = p.to_string();
                    }
                }
                _ => {}
            }
        }
        Some("end-file") if v["reason"] == "error" => {
            let why = v["file_error"].as_str().unwrap_or("unknown error");
            st.error = Some(format!("Cannot play this file ({why})."));
        }
        Some("start-file") => st.error = None,
        _ => {}
    }
}

const PROPS: [&str; 7] = [
    "time-pos",
    "duration",
    "pause",
    "idle-active",
    "volume",
    "media-title",
    "path",
];

pub struct Mpv {
    child: Child,
    sock: Option<UnixStream>,
    reader: Option<BufReader<UnixStream>>,
    sock_path: PathBuf,
    started: Instant,
    state: State,
    /// Commands waiting for the socket.
    queue: Vec<serde_json::Value>,
}

impl Mpv {
    pub fn start() -> Result<Mpv, String> {
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let sock_path = dir.join(format!("retroos-media-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&sock_path);
        let child = Command::new("mpv")
            .arg("--idle=yes")
            .arg("--keep-open=yes")
            .arg("--force-window=no")
            .arg("--no-terminal")
            .arg("--hwdec=auto-safe")
            // No sound device (HDMI audio gone): play on, silently.
            .arg("--audio-fallback-to-null=yes")
            .arg(format!("--input-ipc-server={}", sock_path.display()))
            .arg("--title=${media-title} - Video")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    "The player (mpv) is not installed. INSTALL MPV adds it.".to_string()
                } else {
                    format!("Cannot start the player (mpv): {e}")
                }
            })?;
        let mut m = Mpv {
            child,
            sock: None,
            reader: None,
            sock_path,
            started: Instant::now(),
            state: State {
                idle: true,
                volume: 100.0,
                ..State::default()
            },
            queue: Vec::new(),
        };
        for (i, p) in PROPS.iter().enumerate() {
            m.queue
                .push(serde_json::json!({"command": ["observe_property", i + 1, p]}));
        }
        Ok(m)
    }

    /// Connect once mpv has made its socket (it takes a moment).
    fn connect(&mut self) {
        if self.sock.is_some() {
            return;
        }
        let Ok(s) = UnixStream::connect(&self.sock_path) else {
            if self.started.elapsed() > Duration::from_secs(10) {
                self.state.error = Some("The player (mpv) did not start.".into());
            }
            return;
        };
        let _ = s.set_nonblocking(true);
        if let Ok(r) = s.try_clone() {
            self.reader = Some(BufReader::new(r));
        }
        self.sock = Some(s);
    }

    fn send(&mut self, cmd: serde_json::Value) {
        self.queue.push(cmd);
        self.flush();
    }

    fn flush(&mut self) {
        self.connect();
        let Some(s) = self.sock.as_mut() else { return };
        for c in self.queue.drain(..) {
            let _ = s.write_all(format!("{c}\n").as_bytes());
        }
    }
}

impl Player for Mpv {
    fn load(&mut self, path: &Path) {
        self.state.error = None;
        // Shown at once (and kept if mpv cannot open it).
        self.state.path = path.to_string_lossy().into_owned();
        self.state.title.clear();
        self.send(serde_json::json!({"command": ["loadfile", path.to_string_lossy(), "replace"]}));
        self.send(serde_json::json!({"command": ["set_property", "pause", false]}));
    }

    fn set_pause(&mut self, paused: bool) {
        self.send(serde_json::json!({"command": ["set_property", "pause", paused]}));
    }

    fn stop(&mut self) {
        // Back to the start, paused: Stop then Play plays from the start.
        self.send(serde_json::json!({"command": ["set_property", "pause", true]}));
        self.send(serde_json::json!({"command": ["seek", 0, "absolute"]}));
    }

    fn seek(&mut self, secs: f64) {
        self.send(serde_json::json!({"command": ["seek", secs.max(0.0), "absolute"]}));
    }

    fn set_volume(&mut self, v: f64) {
        self.send(serde_json::json!({"command": ["set_property", "volume", v.clamp(0.0, 100.0)]}));
    }

    fn poll(&mut self) -> &State {
        self.flush();
        let mut lines = Vec::new();
        if let Some(r) = self.reader.as_mut() {
            let mut line = String::new();
            loop {
                line.clear();
                match r.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => lines.push(line.clone()),
                    Err(_) => break, // WouldBlock: nothing more now
                }
            }
        }
        for l in lines {
            apply_event(&mut self.state, &l);
        }
        &self.state
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        if let Some(s) = self.sock.as_mut() {
            let _ = s.write_all(b"{\"command\": [\"quit\"]}\n");
        }
        std::thread::sleep(Duration::from_millis(50));
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.sock_path);
    }
}

/// A pretend player for host tests (`RETROOS_TEST_MEDIA=1`): every file
/// is 3:45 long and plays at real speed.
#[derive(Default)]
pub struct Fake {
    state: State,
    since: Option<(Instant, f64)>,
}

impl Fake {
    pub fn new() -> Fake {
        Fake {
            state: State {
                idle: true,
                volume: 100.0,
                ..State::default()
            },
            since: None,
        }
    }
}

impl Player for Fake {
    fn load(&mut self, path: &Path) {
        self.state.path = path.to_string_lossy().into_owned();
        self.state.title = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.state.duration = 225.0;
        self.state.idle = false;
        self.state.pos = 0.0;
        self.set_pause(false);
    }
    fn set_pause(&mut self, paused: bool) {
        self.poll();
        self.state.paused = paused;
        self.since = (!paused).then(|| (Instant::now(), self.state.pos));
    }
    fn stop(&mut self) {
        self.set_pause(true);
        self.state.pos = 0.0;
    }
    fn seek(&mut self, secs: f64) {
        self.state.pos = secs.clamp(0.0, self.state.duration);
        if !self.state.paused {
            self.since = Some((Instant::now(), self.state.pos));
        }
    }
    fn set_volume(&mut self, v: f64) {
        self.state.volume = v.clamp(0.0, 100.0);
    }
    fn poll(&mut self) -> &State {
        if let Some((t, p)) = self.since {
            self.state.pos = (p + t.elapsed().as_secs_f64()).min(self.state.duration);
        }
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clocks() {
        assert_eq!(clock(83.4), "1:23");
        assert_eq!(clock(3723.0), "1:02:03");
        assert_eq!(clock(-1.0), "0:00");
    }

    #[test]
    fn mpv_events() {
        let mut st = State::default();
        apply_event(
            &mut st,
            r#"{"event":"property-change","id":1,"name":"time-pos","data":12.5}"#,
        );
        apply_event(
            &mut st,
            r#"{"event":"property-change","id":7,"name":"media-title","data":"Song"}"#,
        );
        apply_event(
            &mut st,
            r#"{"event":"property-change","id":3,"name":"pause","data":true}"#,
        );
        apply_event(
            &mut st,
            r#"{"event":"end-file","reason":"error","file_error":"unrecognized file format"}"#,
        );
        assert_eq!((st.pos, st.title.as_str(), st.paused), (12.5, "Song", true));
        assert_eq!(
            st.error.as_deref(),
            Some("Cannot play this file (unrecognized file format).")
        );
        apply_event(
            &mut st,
            r#"{"event":"property-change","name":"path","data":"/m/a.mp3"}"#,
        );
        apply_event(
            &mut st,
            r#"{"event":"property-change","name":"path","data":null}"#,
        );
        assert_eq!(st.path, "/m/a.mp3", "a failed load keeps the name");
        apply_event(&mut st, "not json");
        apply_event(
            &mut st,
            r#"{"event":"property-change","name":"duration","data":null}"#,
        );
        assert_eq!(st.duration, 0.0);
    }
}

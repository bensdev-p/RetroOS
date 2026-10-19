//! A client for Station's launch broker ($RETROOS_STATION; the protocol is
//! described in crates/station/src/broker.rs). One connection per request,
//! so replies never mix with EXIT notices of programs we started.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

/// A window as Station lists it (WINDOWS), top first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowInfo {
    pub id: u64,
    pub state: String,
    pub title: String,
}

/// Send one request; returns the fields of the OK line and the lines after it.
pub fn request(fields: &[&str]) -> Result<(Vec<String>, Vec<String>), String> {
    let path = std::env::var_os("RETROOS_STATION").ok_or("not running under Station")?;
    let s = UnixStream::connect(path).map_err(|e| format!("Station: {e}"))?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    (&s).write_all(format!("{}\n", fields.join("\t")).as_bytes())
        .map_err(|e| e.to_string())?;
    let mut r = BufReader::new(&s);
    let mut first = String::new();
    r.read_line(&mut first).map_err(|e| e.to_string())?;
    let f: Vec<String> = first.trim_end().split('\t').map(String::from).collect();
    if f.first().map(String::as_str) != Some("OK") {
        return Err(f
            .get(1)
            .cloned()
            .unwrap_or_else(|| "Station refused".into()));
    }
    // TASKS and WINDOWS: "OK <n>" then n lines.
    let mut more = Vec::new();
    if matches!(fields.first(), Some(&"TASKS") | Some(&"WINDOWS")) {
        let n: usize = f.get(1).and_then(|v| v.parse().ok()).unwrap_or(0);
        for _ in 0..n {
            let mut l = String::new();
            r.read_line(&mut l).map_err(|e| e.to_string())?;
            more.push(l.trim_end().to_string());
        }
    }
    Ok((f[1..].to_vec(), more))
}

pub fn windows() -> Vec<WindowInfo> {
    let Ok((_, lines)) = request(&["WINDOWS"]) else {
        return vec![];
    };
    lines
        .iter()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.len() >= 8 && f[0] == "WINDOW").then(|| WindowInfo {
                id: f[1].parse().unwrap_or(0),
                state: f[6].to_string(),
                title: f[7..].join(" "),
            })
        })
        .collect()
}

/// Start a program through Station (it shows up in TASKS by `name`).
pub fn launch(name: &str, argv: &[String]) -> Result<(), String> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    let label = format!("=RETROOS_LAUNCH_NAME={name}");
    let mut f: Vec<&str> = vec!["LAUNCH", "nowait", &home, &label];
    f.extend(argv.iter().map(String::as_str));
    request(&f).map(|_| ())
}

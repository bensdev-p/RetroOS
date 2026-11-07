//! /etc/retroos/startup.conf: the settings read before anyone logs in
//! (POST screen, login screen, Station). Control Panel → Startup and
//! Display, DEL Setup and COLOR all edit it.
//!
//! Writing goes through the `set-startup` helper (admin, no password), which
//! accepts exactly the keys and values [`valid`] does. `$RETROOS_STARTUP_CONF`
//! names another file, written directly (tests; `~/` is the home folder).

use crate::admin::{self, AdminError};
use std::path::PathBuf;

pub const PATH: &str = "/etc/retroos/startup.conf";

/// The file in use.
pub fn path() -> PathBuf {
    match std::env::var("RETROOS_STARTUP_CONF") {
        Ok(p) if !p.is_empty() => match p.strip_prefix("~/") {
            Some(rest) => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(rest),
            None => PathBuf::from(p),
        },
        _ => PathBuf::from(PATH),
    }
}

/// The text of the file ("" when missing).
pub fn read() -> String {
    std::fs::read_to_string(path()).unwrap_or_default()
}

/// `key=` from startup.conf text (comments after `#` ignored).
pub fn value(conf: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    conf.lines().find_map(|l| {
        l.split('#')
            .next()?
            .trim()
            .strip_prefix(prefix.as_str())
            .map(|v| v.trim().to_string())
    })
}

/// `key=` with a default.
pub fn get(conf: &str, key: &str) -> String {
    value(conf, key).unwrap_or_else(|| default(key).to_string())
}

/// What a missing setting means.
pub fn default(key: &str) -> &'static str {
    match key {
        "mode" => "text",
        "fast_post" | "chime" => "yes",
        "menu_timeout" => "30",
        "scheme" => "classic",
        _ => "auto",
    }
}

/// Whether `key=value` may be written (the helper checks the same).
pub fn valid(key: &str, value: &str) -> bool {
    match key {
        "mode" => matches!(value, "text" | "desktop"),
        "fast_post" | "chime" => matches!(value, "yes" | "no"),
        "menu_timeout" => value
            .parse::<u32>()
            .is_ok_and(|n| (1..=600).contains(&n) && !value.starts_with('0')),
        "scheme" => matches!(value, "classic" | "paper" | "amber" | "green" | "high"),
        "rotate" => matches!(
            value,
            "auto" | "flip" | "right" | "left" | "0" | "90" | "180" | "270"
        ),
        "ui_scale" => matches!(value, "auto" | "1" | "2" | "3" | "4"),
        "renderer" => matches!(value, "auto" | "gpu" | "cpu"),
        _ => false,
    }
}

/// `conf` with each `key=value` of `changes` replacing that key's line (its
/// comment kept), or added at the end. Other lines are left alone.
pub fn edit(conf: &str, changes: &[(&str, &str)]) -> String {
    let mut out = Vec::new();
    let mut done = vec![false; changes.len()];
    for line in conf.lines() {
        let key = line
            .split('#')
            .next()
            .unwrap_or("")
            .split('=')
            .next()
            .unwrap_or("")
            .trim();
        let hit = line.contains('=') && !line.trim_start().starts_with('#');
        match changes.iter().position(|(k, _)| hit && *k == key) {
            Some(i) if !done[i] => {
                done[i] = true;
                let comment = line.find('#').map(|c| &line[c..]);
                let kv = format!("{}={}", changes[i].0, changes[i].1);
                out.push(match comment {
                    Some(c) => format!("{kv:<18} {c}"),
                    None => kv,
                });
            }
            Some(_) => {} // a repeated key: the first one is the setting
            None => out.push(line.to_string()),
        }
    }
    for (i, (k, v)) in changes.iter().enumerate() {
        if !done[i] {
            out.push(format!("{k}={v}"));
        }
    }
    out.join("\n") + "\n"
}

/// Save `changes` (every pair must be [`valid`]).
pub fn set(changes: &[(&str, &str)]) -> Result<(), AdminError> {
    if let Some((k, v)) = changes.iter().find(|(k, v)| !valid(k, v)) {
        return Err(AdminError::Failed(format!("Invalid setting {k}={v}")));
    }
    if changes.is_empty() {
        return Ok(());
    }
    let p = path();
    if p != std::path::Path::new(PATH) {
        let body = edit(&std::fs::read_to_string(&p).unwrap_or_default(), changes);
        return std::fs::write(&p, body).map_err(|e| AdminError::Failed(e.to_string()));
    }
    let args: Vec<String> = changes.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    admin::run_helper("set-startup", &args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_keep_comments_and_other_lines() {
        let conf = "# RetroOS startup settings.\nmode=text          # text or desktop\nchime=yes\n# rotate=auto\n";
        let got = edit(conf, &[("mode", "desktop"), ("rotate", "90")]);
        assert_eq!(
            got,
            "# RetroOS startup settings.\nmode=desktop       # text or desktop\nchime=yes\n# rotate=auto\nrotate=90\n"
        );
        assert_eq!(value(&got, "rotate").as_deref(), Some("90"));
        assert_eq!(get("", "menu_timeout"), "30");
    }

    #[test]
    fn only_known_settings_are_valid() {
        assert!(valid("menu_timeout", "45"));
        assert!(!valid("menu_timeout", "0"));
        assert!(!valid("menu_timeout", "007"));
        assert!(valid("rotate", "270"));
        assert!(!valid("rotate", "45"));
        assert!(!valid("scheme", "classic;rm"));
        assert!(!valid("shell", "yes"));
    }
}

//! retro-session — the user session greetd starts after login.
//!
//!   retro-session [--mode text|desktop]
//!
//! Picks the startup mode (one-shot Startup Menu choice, then the login
//! screen's choice, then /etc/retroos/startup.conf), prepares the
//! environment (themes, toolkits on Wayland), and runs Station with the
//! RetroDOS console, restarting it if it crashes.

use std::process::Command;
use std::time::{Duration, Instant};

const CHOICE: &str = "/run/retroos/startup-choice";

fn boot_id() -> String {
    std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// The F8 Startup Menu choice applies to the first session of this boot only.
fn startup_menu_choice() -> Option<String> {
    let choice = std::fs::read_to_string(CHOICE).ok()?.trim().to_string();
    let runtime = std::env::var("XDG_RUNTIME_DIR").ok()?;
    let marker = format!("{runtime}/retroos-startup-choice-used");
    let id = boot_id();
    if std::fs::read_to_string(&marker)
        .map(|s| s.trim() == id)
        .unwrap_or(false)
    {
        return None;
    }
    let _ = std::fs::write(&marker, &id);
    matches!(choice.as_str(), "text" | "desktop").then_some(choice)
}

fn configured_mode() -> String {
    std::fs::read_to_string("/etc/retroos/startup.conf")
        .unwrap_or_default()
        .lines()
        .find_map(|l| {
            l.split('#')
                .next()?
                .trim()
                .strip_prefix("mode=")
                .map(|v| v.trim().to_string())
        })
        .filter(|m| m == "text" || m == "desktop")
        .unwrap_or_else(|| "text".into())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg_mode = args
        .iter()
        .position(|a| a == "--mode")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let mode = startup_menu_choice()
        .or(arg_mode)
        .unwrap_or_else(configured_mode);

    // Environment for everything in the session.
    let set = |k: &str, v: &str| std::env::set_var(k, v);
    set("XDG_SESSION_TYPE", "wayland");
    set("XDG_CURRENT_DESKTOP", "RetroOS");
    set("XDG_SESSION_DESKTOP", "retroos");
    set("MOZ_ENABLE_WAYLAND", "1");
    set("QT_QPA_PLATFORM", "wayland;xcb");
    set("SDL_VIDEODRIVER", "wayland,x11");
    set("GDK_BACKEND", "wayland,x11");
    set("_JAVA_AWT_WM_NONREPARENTING", "1");
    if std::path::Path::new("/usr/share/themes/Phosphor").exists() {
        set("GTK_THEME", "Phosphor");
    }
    if std::path::Path::new("/usr/share/icons/Phosphor/cursors").exists() {
        set("XCURSOR_THEME", "Phosphor");
    }
    if let Ok(home) = std::env::var("HOME") {
        let _ = std::env::set_current_dir(&home);
    }
    // Import the environment into the user manager so D-Bus-activated
    // services (PipeWire, portals) see the same session.
    let _ = Command::new("systemctl")
        .args([
            "--user",
            "import-environment",
            "XDG_SESSION_TYPE",
            "XDG_CURRENT_DESKTOP",
            "WAYLAND_DISPLAY",
        ])
        .status();

    let personality = if mode == "desktop" { "desktop" } else { "text" };
    let mut crashes: Vec<Instant> = Vec::new();
    loop {
        let status = Command::new("station")
            .args(["--personality", personality, "--exec", "retro-console"])
            .status();
        match status {
            Ok(s) if s.success() => break,
            Ok(s) => {
                eprintln!("retro-session: station exited with {s}");
                let now = Instant::now();
                crashes.retain(|t| now.duration_since(*t) < Duration::from_secs(60));
                crashes.push(now);
                if crashes.len() >= 3 {
                    eprintln!("retro-session: station keeps failing; ending the session");
                    std::process::exit(1);
                }
                std::thread::sleep(Duration::from_millis(500));
            }
            Err(e) => {
                eprintln!("retro-session: cannot start station: {e}");
                std::process::exit(1);
            }
        }
    }
}

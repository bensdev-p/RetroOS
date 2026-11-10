//! RetroDOS — the command shell of RetroOS.
//!
//!   retrodos               interactive shell (runs C:\AUTOEXEC.BAT first, if present)
//!   retrodos /C command    run one command line and exit with its ERRORLEVEL
//!   retrodos /K command    run a command line, then stay interactive
//!   retrodos /NOLOGO       skip the banner

mod apps;
mod batch;
mod builtins;
mod fmt;
mod launch;
mod lineedit;
mod parse;
mod shell;
mod sysinfo;
mod term;

use shell::{Shell, VERSION};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut once: Option<String> = None;
    let mut keep: Option<String> = None;
    let mut logo = true;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].to_ascii_uppercase();
        match a.as_str() {
            "/C" | "-C" => {
                once = Some(args[i + 1..].join(" "));
                break;
            }
            "/K" | "-K" => {
                keep = Some(args[i + 1..].join(" "));
                break;
            }
            "/NOLOGO" => logo = false,
            "/?" | "--HELP" => {
                println!("Starts a new instance of the RetroDOS command interpreter.\n");
                println!("RETRODOS [/C string | /K string] [/NOLOGO]\n");
                println!(
                    "  /C string  Carries out the command specified by string, and then stops."
                );
                println!("  /K string  Carries out the command specified by string but remains.");
                return;
            }
            "--VERSION" => {
                println!("RetroDOS Version {VERSION}");
                return;
            }
            _ => {}
        }
        i += 1;
    }

    term::install_signal_handlers();
    let mut sh = Shell::new();

    if let Some(line) = once {
        sh.execute(&line);
        std::process::exit(sh.errorlevel);
    }
    if logo && sh.depth == 0 {
        println!("\nRetroDOS Version {VERSION}");
        println!("             (C)Copyright Phosphor Computer Company 1987-1993.\n");
        updates_notice();
    }
    if sh.depth == 0 && keep.is_none() && std::env::var_os("RETROOS_SAFE_MODE").is_none() {
        if let Some(p) = sh.find_batch("C:\\AUTOEXEC.BAT") {
            sh.run_batch(&p, Vec::new());
        }
    }
    if let Some(line) = keep {
        sh.execute(&line);
    }
    sh.interactive();
    std::process::exit(sh.errorlevel);
}

/// "3 updates available. Type UPDATE." under the logo, from what the daily
/// check found (/var/lib/retroos/updates, retro_sys::software, D-055).
fn updates_notice() {
    let path = std::env::var("RETROOS_UPDATES_STATE")
        .unwrap_or_else(|_| "/var/lib/retroos/updates".into());
    let n: u32 = std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .find_map(|l| l.strip_prefix("count=")?.trim().parse().ok())
        .unwrap_or(0);
    match n {
        0 => {}
        1 => println!("1 update available. Type UPDATE.\n"),
        n => println!("{n} updates available. Type UPDATE.\n"),
    }
}

//! RetroDOS — the command shell of RetroOS.
//!
//!   retrodos               interactive shell (runs C:\AUTOEXEC.BAT first, if present)
//!   retrodos /C command    run one command line and exit with its ERRORLEVEL
//!   retrodos /K command    run a command line, then stay interactive
//!   retrodos /NOLOGO       skip the banner

mod builtins;
mod fmt;
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
    }
    if let Some(line) = keep {
        sh.execute(&line);
    }
    sh.interactive();
    std::process::exit(sh.errorlevel);
}

//! BT — Bluetooth in RetroDOS (DESIGN.md §8, §10.2).
//!
//!   retro-bt            the pairing screen
//!   retro-bt on | off   turn Bluetooth on or off (BT ON, BT OFF)
//!   retro-bt --status   print whether it is on, and the paired devices

mod app;

use app::{Action, App};
use retro_sys::bt::{self, Absent};
use retro_tui::{keys, read_key, Screen};
use std::io::Write;
use std::time::{Duration, Instant};

fn status() -> i32 {
    if !bt::enabled() {
        println!("Bluetooth is off.");
        return 0;
    }
    match bt::open_api() {
        Ok(b) => {
            println!("Bluetooth is on ({}).", b.adapter());
            let devs = b.devices().unwrap_or_default();
            let paired: Vec<_> = devs.iter().filter(|d| d.paired).collect();
            if paired.is_empty() {
                println!("No paired devices.");
            }
            for d in paired {
                let state = if d.connected { ", connected" } else { "" };
                println!("  {} ({}{state})", d.name, d.kind.label());
            }
            0
        }
        Err(e) => {
            println!("{}", e.text());
            1
        }
    }
}

fn turn(on: bool) -> i32 {
    match bt::set_enabled(on) {
        Ok(()) => {
            println!("Bluetooth is {}.", if on { "on" } else { "off" });
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

/// Turn Bluetooth on and wait for the adapter.
fn turn_on() -> Result<Box<dyn bt::BtApi>, Absent> {
    bt::set_enabled(true).map_err(Absent::Error)?;
    if std::env::var_os("RETROOS_TEST_BT").is_some() {
        return bt::open_api();
    }
    bt::Bt::open_wait(Duration::from_secs(10)).map(|b| Box::new(b) as Box<dyn bt::BtApi>)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|a| a.to_ascii_lowercase()).as_deref() {
        Some("--status") | Some("/s") => std::process::exit(status()),
        Some("on") => std::process::exit(turn(true)),
        Some("off") => std::process::exit(turn(false)),
        _ => {}
    }
    // Just turned on (CONTROL's Bluetooth page): give bluetoothd a moment.
    let api = match bt::open_api() {
        Err(Absent::Off) if bt::enabled() => {
            bt::Bt::open_wait(Duration::from_secs(8)).map(|b| Box::new(b) as Box<dyn bt::BtApi>)
        }
        r => r,
    };
    let mut app = App::new(api);
    let mut s = match Screen::open() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("BT: {e}");
            std::process::exit(1);
        }
    };
    print!("{}", keys::enable_mouse());
    let _ = std::io::stdout().flush();
    let mut last = Instant::now();
    while !app.quit {
        if s.check_resize() {
            s.invalidate();
        }
        app.draw(&mut s);
        if s.flush().is_err() {
            break;
        }
        let k = match read_key(250) {
            Ok(k) => k,
            Err(_) => break,
        };
        if let Some(k) = k {
            if let Some(a) = app.key(k) {
                app.waiting = Some(match a {
                    Action::TurnOn => "Turning Bluetooth on...".into(),
                    Action::TurnOff => "Turning Bluetooth off...".into(),
                });
                app.draw(&mut s);
                let _ = s.flush();
                let api = match a {
                    Action::TurnOn => turn_on(),
                    Action::TurnOff => match bt::set_enabled(false) {
                        Ok(()) => Err(Absent::Off),
                        Err(e) => Err(Absent::Error(e)),
                    },
                };
                app.waiting = None;
                app.reopen(api);
            }
        }
        if last.elapsed() >= Duration::from_millis(500) {
            app.refresh();
            last = Instant::now();
        }
    }
    print!("{}", keys::disable_mouse());
    let _ = std::io::stdout().flush();
}

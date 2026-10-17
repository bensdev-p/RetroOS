//! WIFI / NET — network setup for RetroDOS.
//!
//!   retro-net            the network screen
//!   retro-net --status   print a status summary and exit (NET /S)

mod app;

use app::App;
use retro_sys::net::{self, WifiApi};
use retro_tui::{keys, read_key, Screen};
use std::io::Write;
use std::time::{Duration, Instant};

fn open_wifi() -> (Option<Box<dyn WifiApi>>, Option<String>) {
    match net::Wifi::open() {
        Ok(Some(w)) => (Some(Box::new(w)), None),
        Ok(None) => (None, None),
        Err(e) if e.contains("not available") || e.contains("ServiceUnknown") => (
            None,
            Some("The wireless service (iwd) is not running.".into()),
        ),
        Err(e) => (None, Some(e)),
    }
}

fn status() -> i32 {
    match net::links() {
        Ok(links) => {
            for l in links.iter().filter(|l| l.kind != "wlan") {
                let addrs = if l.addresses.is_empty() {
                    String::new()
                } else {
                    format!("  {}", l.addresses.join(" "))
                };
                println!("Wired     {:<8} {}{addrs}", l.name, l.friendly_state());
            }
        }
        Err(e) => println!("Wired     unknown ({e})"),
    }
    match open_wifi() {
        (Some(w), _) => {
            let nets = w.networks().unwrap_or_default();
            match nets.iter().find(|n| n.connected) {
                Some(n) => println!(
                    "Wireless  {:<8} Connected to {} ({} dBm)",
                    w.device(),
                    n.ssid,
                    n.dbm
                ),
                None => println!(
                    "Wireless  {:<8} Not connected; {} network(s) in range",
                    w.device(),
                    nets.len()
                ),
            }
        }
        (None, why) => println!(
            "Wireless  {}",
            why.unwrap_or_else(|| "No wireless adapter".into())
        ),
    }
    0
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--status") {
        std::process::exit(status());
    }
    let (wifi, why) = open_wifi();
    if let Some(w) = &wifi {
        let _ = w.scan();
    }
    let mut app = App::new(wifi, why);
    app.refresh(net::links().unwrap_or_default());
    let mut s = match Screen::open() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("NET: {e}");
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
        let k = match read_key(500) {
            Ok(k) => k,
            Err(_) => break,
        };
        if let Some(k) = k {
            if let Some(job) = app.key(k) {
                app.busy = Some(job.busy_text());
                app.draw(&mut s);
                let _ = s.flush();
                app.run(job);
                app.busy = None;
                last = Instant::now() - Duration::from_secs(10);
            }
        }
        if last.elapsed() >= Duration::from_secs(2) {
            app.refresh(net::links().unwrap_or_default());
            last = Instant::now();
        }
    }
    print!("{}", keys::disable_mouse());
    let _ = std::io::stdout().flush();
}

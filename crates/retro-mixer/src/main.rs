//! MIXER / VOLUME — the RetroOS volume mixer in text mode (DESIGN.md §8).
//!
//!   retro-mixer            the mixer screen
//!   retro-mixer --status   print the output and its volume (VOLUME /S)
//!   retro-mixer N          set the output's volume to N % (VOLUME 40)

mod app;

use app::{App, Change};
use retro_sys::sound;
use retro_tui::{keys, read_key, Screen};
use std::io::Write;
use std::time::{Duration, Instant};

fn status() -> i32 {
    match sound::mixer() {
        Ok(m) => match m.output() {
            Some(o) => {
                let mute = if o.muted { ", muted" } else { "" };
                println!("{}: {} %{mute}", o.name, o.percent());
                for s in &m.streams {
                    println!("  {}: {} %", s.name, s.percent());
                }
                0
            }
            None => {
                println!("No sound outputs were found.");
                1
            }
        },
        Err(e) => {
            println!("{e}");
            1
        }
    }
}

fn set_percent(arg: &str) -> i32 {
    let Ok(pct) = arg.trim_end_matches('%').parse::<u32>() else {
        eprintln!("Invalid volume - {arg} (0 to 100)");
        return 1;
    };
    let r = sound::mixer().and_then(|m| {
        let o = m.output().ok_or("No sound outputs were found.")?.clone();
        sound::set_volume(o.id, pct.min(100) as f32 / 100.0)?;
        if o.muted && pct > 0 {
            sound::set_mute(o.id, false)?;
        }
        Ok(o.name)
    });
    match r {
        Ok(name) => {
            println!("{name}: {} %", pct.min(100));
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--status") => std::process::exit(status()),
        Some(a) if a.chars().next().is_some_and(|c| c.is_ascii_digit()) => {
            std::process::exit(set_percent(a))
        }
        _ => {}
    }
    let mut app = App::new(sound::mixer());
    let mut s = match Screen::open() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("MIXER: {e}");
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
            if let Some(c) = app.key(k) {
                let r = match c {
                    Change::Volume(id, v) => sound::set_volume(id, v),
                    Change::Mute(id, m) => sound::set_mute(id, m),
                    Change::Output(id) => sound::set_default(id),
                };
                app.error = r.err();
                last = Instant::now() - Duration::from_secs(5);
            }
        }
        if last.elapsed() >= Duration::from_secs(1) {
            app.refresh(sound::mixer());
            last = Instant::now();
        }
    }
    print!("{}", keys::disable_mouse());
    let _ = std::io::stdout().flush();
}

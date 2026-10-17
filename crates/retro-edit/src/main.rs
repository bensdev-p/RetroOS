//! EDIT — the RetroOS text editor (in the spirit of EDIT.COM).
//!
//!   retro-edit [file]
//!
//! RetroDOS runs it for the EDIT command, translating DOS paths first.

mod app;
mod buffer;

use app::{App, Mode};
use buffer::Buffer;
use retro_tui::{keys, read_key, Key, Screen};
use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "/?" || a == "--help") {
        println!("Starts the RetroOS editor.\n\nEDIT [[drive:][path]filename]");
        return;
    }
    // Real paths start with '/', so only "/?" counts as a switch.
    let buf = match args.iter().find(|a| *a != "/?" && !a.starts_with("--")) {
        Some(f) => match Buffer::open(std::path::Path::new(f)) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("EDIT: cannot open {f}: {e}");
                std::process::exit(1);
            }
        },
        None => Buffer::default(),
    };
    if let Some(log) = std::env::var_os("RETRO_EDIT_LOG") {
        let _ = std::fs::write(log, format!("args {args:?}\n"));
    }
    let mut app = App::new(buf);
    let mut s = match Screen::open() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("EDIT: {e}");
            std::process::exit(1);
        }
    };
    print!("{}", keys::enable_mouse());
    let _ = std::io::stdout().flush();
    loop {
        if s.check_resize() {
            s.invalidate();
        }
        app.draw(&mut s);
        if s.flush().is_err() {
            break;
        }
        let k = match read_key(500) {
            Ok(Some(k)) => k,
            Ok(None) => continue,
            Err(_) => break,
        };
        if let Some(log) = std::env::var_os("RETRO_EDIT_LOG") {
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log)
            {
                let _ = writeln!(f, "{k:?}");
            }
        }
        if matches!(app.mode, Mode::Help) {
            app.mode = Mode::Edit;
            continue;
        }
        if k == Key::Ctrl('l') {
            s.invalidate();
            continue;
        }
        app.key(k);
        if app.quit {
            break;
        }
    }
    print!("{}", keys::disable_mouse());
    let _ = std::io::stdout().flush();
}

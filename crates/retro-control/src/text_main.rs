//! CONTROL — the Control Panel in text mode.
//!
//!   retro-control [applet]    applet: DISPLAY, STARTUP, TIME, KEYBOARD, NETWORK

use retro_control::applet_named;
use retro_control::text::{Action, App};
use retro_tui::{keys, read_key, Screen};
use std::io::Write;

fn open_screen() -> Screen {
    match Screen::open() {
        Ok(s) => {
            print!("{}", keys::enable_mouse());
            let _ = std::io::stdout().flush();
            s
        }
        Err(e) => {
            eprintln!("CONTROL: {e}");
            std::process::exit(1);
        }
    }
}

fn close_screen(s: Screen) {
    print!("{}", keys::disable_mouse());
    let _ = std::io::stdout().flush();
    drop(s);
}

fn run(program: &str) {
    let found = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(program).is_file()))
        .unwrap_or(false);
    if found {
        let _ = std::process::Command::new(program).status();
    }
}

fn main() {
    let arg = std::env::args().nth(1);
    let start = match arg.as_deref() {
        None => None,
        Some("/?" | "-h" | "--help") => {
            println!("Changes RetroOS settings.\n\nCONTROL [DISPLAY | STARTUP | TIME | KEYBOARD | NETWORK | USERS]");
            return;
        }
        Some(a) => match applet_named(a) {
            Some(x) => Some(x),
            None => {
                eprintln!("Invalid parameter - {a}");
                std::process::exit(1);
            }
        },
    };
    let mut app = App::new(start);
    let mut s = open_screen();
    while !app.quit {
        if s.check_resize() {
            s.invalidate();
        }
        app.draw(&mut s);
        if s.flush().is_err() {
            break;
        }
        let k = match read_key(1000) {
            Ok(Some(k)) => k,
            Ok(None) => continue,
            Err(_) => break,
        };
        match app.key(k) {
            Some(Action::Quit) => app.quit = true,
            Some(Action::Run(p)) => {
                close_screen(s);
                run(p);
                s = open_screen();
                s.invalidate();
            }
            Some(Action::RunForm) => {
                app.busy = Some("Working...".into());
                app.draw(&mut s);
                let _ = s.flush();
                app.run_form();
                app.busy = None;
            }
            Some(Action::Apply) => {
                app.busy = Some("Saving...".into());
                app.draw(&mut s);
                let _ = s.flush();
                let done = app.apply();
                app.busy = None;
                if let Some(d) = done {
                    if let Some(scheme) = d.scheme {
                        // Every console (this one too) and the Desktop follow.
                        let _ = retro_ui::station::request(&["SCHEME", &scheme]);
                        s.set_mono(retro_tui::is_mono(&scheme));
                        s.invalidate();
                    }
                    if d.prefs {
                        let _ = retro_ui::station::request(&["PREFS"]);
                    }
                    if d.keymap {
                        let _ = retro_ui::station::request(&["KEYMAP"]);
                    }
                }
            }
            None => {}
        }
    }
    close_screen(s);
}

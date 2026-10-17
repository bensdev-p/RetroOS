//! FILER — the RetroOS file manager (Norton Commander × ProDOS Filer).
//!
//!   retro-filer [left-dir [right-dir]]

mod app;
mod panel;

use app::{Action, App, Mode};
use retro_dosfs::Dos;
use retro_tui::{keys, read_key, Key, Screen};
use std::io::Write;
use std::path::PathBuf;

/// Leave full-screen mode, run a program in the terminal, come back.
fn run_outside(s: &mut Screen, program: &str, args: &[String], pause: bool) {
    print!("{}\x1b[?1049l\x1b[?25h", keys::disable_mouse());
    let _ = std::io::stdout().flush();
    let saved = unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        libc::tcgetattr(0, &mut t);
        t
    };
    if let Some(orig) = s.original_mode() {
        unsafe { libc::tcsetattr(0, libc::TCSANOW, &orig) };
    }
    let status = std::process::Command::new(program).args(args).status();
    if let Err(e) = &status {
        println!("Cannot run {program}: {e}");
    }
    if pause {
        print!("\nPress any key to return to Filer...");
        let _ = std::io::stdout().flush();
    }
    unsafe { libc::tcsetattr(0, libc::TCSANOW, &saved) };
    if pause {
        let _ = read_key(-1);
    }
    print!("\x1b[?1049h\x1b[?25l{}", keys::enable_mouse());
    let _ = std::io::stdout().flush();
    s.invalidate();
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "/?" || a == "--help") {
        println!("Starts the RetroOS file manager.\n\nFILER [left-directory [right-directory]]");
        return;
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/".into());
    let user = std::env::var("USER").unwrap_or_default();
    let mut dos = Dos::new(&home);
    dos.refresh_removable(&user);
    let cwd = std::env::current_dir().unwrap_or_else(|_| home.clone());
    let left = args.first().map(PathBuf::from).unwrap_or(cwd);
    let right = args.get(1).map(PathBuf::from).unwrap_or(home);
    let mut app = App::new(dos, &left, &right);
    let mut s = match Screen::open() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("FILER: {e}");
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
        let k = match read_key(1000) {
            Ok(Some(k)) => k,
            Ok(None) => continue,
            Err(_) => break,
        };
        if matches!(app.mode, Mode::Help) {
            app.mode = Mode::Normal;
            continue;
        }
        if k == Key::Ctrl('l') {
            s.invalidate();
            continue;
        }
        match app.key(k) {
            Action::None => {}
            Action::Quit => break,
            Action::Edit(p) => {
                run_outside(
                    &mut s,
                    "retro-edit",
                    &[p.to_string_lossy().into_owned()],
                    false,
                );
                app.panels.iter_mut().for_each(|p| p.reload());
            }
            Action::Run(p) => {
                run_outside(&mut s, &p.to_string_lossy(), &[], true);
                app.panels.iter_mut().for_each(|p| p.reload());
            }
        }
    }
    print!("{}", keys::disable_mouse());
    let _ = std::io::stdout().flush();
}

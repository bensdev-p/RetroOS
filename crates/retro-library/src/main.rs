//! LIBRARY, INSTALL, UNINSTALL, UPDATE — the Software Library in RetroDOS
//! (DESIGN.md §8, §10.5, D-055).
//!
//!   retro-library [category]         the Library screen
//!   retro-library --install NAME...  INSTALL: a catalog program or a package
//!   retro-library --remove NAME...   UNINSTALL
//!   retro-library --update           UPDATE: look for updates, install them
//!   retro-library --notice           print "N updates available…" if any

mod app;

use app::App;
use retro_sys::admin::AdminError;
use retro_sys::catalog::catalog;
use retro_sys::software::{self, Op};
use retro_tui::{keys, read_key, Screen};
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// Read a line from the terminal without showing it.
fn read_secret(prompt: &str) -> Option<String> {
    print!("{prompt}");
    let _ = std::io::stdout().flush();
    let fd = libc::STDIN_FILENO;
    // SAFETY: plain termios calls on stdin; the old mode is restored below.
    let old = unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(fd, &mut t) != 0 {
            None
        } else {
            let mut quiet = t;
            quiet.c_lflag &= !libc::ECHO;
            libc::tcsetattr(fd, libc::TCSANOW, &quiet);
            Some(t)
        }
    };
    let mut line = String::new();
    let ok = std::io::stdin()
        .lock()
        .read_line(&mut line)
        .is_ok_and(|n| n > 0);
    if let Some(t) = old {
        // SAFETY: restoring the mode read above.
        unsafe { libc::tcsetattr(fd, libc::TCSANOW, &t) };
    }
    println!();
    ok.then(|| line.trim_end_matches(['\n', '\r']).to_string())
}

fn ask_yes(question: &str) -> bool {
    print!("{question} (Y/N)? ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    line.trim().eq_ignore_ascii_case("y")
}

/// Ask for the password while sudo needs it (three tries).
fn authorize() -> Result<(), String> {
    if !software::needs_password() {
        return Ok(());
    }
    for _ in 0..3 {
        let Some(p) = read_secret("Administrator password: ") else {
            return Err("Cancelled.".into());
        };
        match software::authorize(&p) {
            Ok(()) => return Ok(()),
            Err(AdminError::WrongPassword) => println!("That password is not right."),
            Err(e) => {
                return Err(e
                    .to_string()
                    .replace("change this setting", "install software"))
            }
        }
    }
    Err("Three wrong passwords.".into())
}

/// Run a job, drawing a progress bar on one line.
/// Ctrl+C presses while a job runs (the helper itself ignores them).
static INTERRUPTS: AtomicUsize = AtomicUsize::new(0);

extern "C" fn on_interrupt(_: libc::c_int) {
    INTERRUPTS.fetch_add(1, Ordering::SeqCst);
}

fn run(op: Op) -> Result<(), String> {
    authorize()?;
    let job = software::start(op)?;
    // Ctrl+C: Cancel; again: Force Stop (D-058).
    INTERRUPTS.store(0, Ordering::SeqCst);
    // SAFETY: the handler only touches an atomic.
    let old = unsafe {
        libc::signal(
            libc::SIGINT,
            on_interrupt as *const () as libc::sighandler_t,
        )
    };
    let mut seen = 0;
    let mut cancel: Option<software::Cancel> = None;
    let mut last = String::new();
    let result = loop {
        let n = INTERRUPTS.load(Ordering::SeqCst);
        if n > seen {
            seen = n;
            let c = software::cancel(n >= 2).unwrap_or(software::Cancel::Nothing);
            cancel = Some(c);
            println!();
            println!("{}", c.say());
            if n == 1 && c != software::Cancel::Nothing {
                println!(
                    "Ctrl+C again: Force Stop (a program being set up may be left half installed; it is finished before the next installation)."
                );
            }
            last.clear();
        }
        let p = job.progress();
        let filled = (p.percent.min(100) * 30 / 100) as usize;
        let line = format!(
            "\r[{}{}] {:>3}%  {:<40}",
            "\u{2588}".repeat(filled),
            "\u{2591}".repeat(30 - filled),
            p.percent,
            p.text.chars().take(40).collect::<String>()
        );
        if line != last {
            print!("{line}");
            let _ = std::io::stdout().flush();
            last = line;
        }
        if let Some(r) = job.finished() {
            println!();
            break match (cancel, r) {
                (Some(software::Cancel::Stopped), _) => Err(
                    "Stopped. Anything left half installed is finished before the next installation."
                        .into(),
                ),
                (_, r) => r,
            };
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    // SAFETY: restores the handler that was there.
    unsafe { libc::signal(libc::SIGINT, old) };
    result
}

/// What INSTALL / UNINSTALL was given: catalog programs by name, else
/// packages by name.
fn resolve(names: &[String]) -> Result<Vec<(String, Vec<String>)>, String> {
    names
        .iter()
        .map(|n| match catalog().find(n) {
            Some(a) => Ok((a.name.clone(), a.packages.clone())),
            None => {
                let p = n.to_ascii_lowercase();
                if software::valid_package(&p) {
                    Ok((p.clone(), vec![p]))
                } else {
                    Err(format!("Unknown program - {n}"))
                }
            }
        })
        .collect()
}

fn install(names: &[String]) -> i32 {
    let list = match resolve(names) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let installed =
        software::installed(&list.iter().map(|(_, p)| p[0].clone()).collect::<Vec<_>>());
    let todo: Vec<&(String, Vec<String>)> = list
        .iter()
        .filter(|(n, p)| {
            let have = installed.contains(&p[0]);
            if have {
                println!("{n} is already installed.");
            }
            !have
        })
        .collect();
    if todo.is_empty() {
        return 0;
    }
    for (n, p) in &todo {
        match catalog().find(n) {
            Some(a) => println!("{}: {}", a.name, a.blurb),
            None => println!(
                "{n}: the Debian package {} (not in the Library's catalog)",
                p[0]
            ),
        }
    }
    let packages: Vec<String> = todo.iter().flat_map(|(_, p)| p.clone()).collect();
    match run(Op::Install(packages)) {
        Ok(()) => {
            for (n, _) in &todo {
                match catalog().find(n) {
                    Some(a) => {
                        let cmd = a
                            .alias
                            .first()
                            .cloned()
                            .unwrap_or_else(|| a.run.to_ascii_uppercase());
                        println!("{} is installed. Type {cmd} to start it.", a.name);
                        if let Some(note) = &a.note {
                            println!("{note}");
                        }
                    }
                    None => println!("{n} is installed."),
                }
            }
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn remove(names: &[String]) -> i32 {
    let list = match resolve(names) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let installed =
        software::installed(&list.iter().map(|(_, p)| p[0].clone()).collect::<Vec<_>>());
    let todo: Vec<&(String, Vec<String>)> = list
        .iter()
        .filter(|(n, p)| {
            let have = installed.contains(&p[0]);
            if !have {
                println!("{n} is not installed.");
            }
            have
        })
        .collect();
    if todo.is_empty() {
        return 0;
    }
    let names: Vec<&str> = todo.iter().map(|(n, _)| n.as_str()).collect();
    if !ask_yes(&format!("Remove {}", names.join(", "))) {
        return 1;
    }
    match run(Op::Remove(todo.iter().map(|(_, p)| p[0].clone()).collect())) {
        Ok(()) => {
            println!("{} removed.", names.join(", "));
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn update() -> i32 {
    let ups = software::updates();
    if ups.is_empty() {
        println!("No updates are known yet.");
        if !ask_yes("Look for updates and install them now") {
            return 0;
        }
    } else {
        for u in &ups {
            let sec = if u.security { "  (security)" } else { "" };
            println!("  {:<30} {}{sec}", u.package, u.to);
        }
        let n = ups.len();
        if !ask_yes(&format!(
            "Install {n} update{}",
            if n == 1 { "" } else { "s" }
        )) {
            return 0;
        }
    }
    match run(Op::UpgradeAll) {
        Ok(()) => {
            match software::status().count {
                0 => println!("The computer is up to date."),
                n => println!("{n} updates could not be installed yet (they need others first)."),
            }
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
    let rest = || args[1..].to_vec();
    match args.first().map(String::as_str) {
        Some("--install") => std::process::exit(install(&rest())),
        Some("--remove") => std::process::exit(remove(&rest())),
        Some("--update") => std::process::exit(update()),
        Some("--notice") => {
            if let Some(n) = software::notice(&software::status()) {
                println!("{n}");
            }
            return;
        }
        _ => {}
    }
    let mut app = App::new(args.first().map(String::as_str));
    let mut s = match Screen::open() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("LIBRARY: {e}");
            std::process::exit(1);
        }
    };
    print!("{}", keys::enable_mouse());
    let _ = std::io::stdout().flush();
    while !app.quit {
        if s.check_resize() {
            s.invalidate();
        }
        app.tick();
        app.draw(&mut s);
        if s.flush().is_err() {
            break;
        }
        match read_key(100) {
            Ok(Some(k)) => app.key(k),
            Ok(None) => {}
            Err(_) => break,
        }
    }
    print!("{}", keys::disable_mouse());
    let _ = std::io::stdout().flush();
}

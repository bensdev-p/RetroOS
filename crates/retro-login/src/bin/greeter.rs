//! retro-greeter — the RetroOS login screen (a greetd greeter).
//!
//! Runs inside Station + retro-console under greetd. Without $GREETD_SOCK it
//! runs in demo mode and accepts any password (for testing).

use retro_login::greetd::{Greetd, Reply};
use retro_login::{chrome, clock, local_users};
use retro_tui::{
    read_key, Field, Key, Screen, BLACK, BRICK, COBALT, LEMON, SILVER, SLATE, TEAL, WHITE,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Name,
    Password,
}

struct Ui {
    name: Field,
    pass: Field,
    focus: Focus,
    desktop: bool,
    message: String,
    users: Vec<String>,
    busy: bool,
}

fn default_mode_desktop() -> bool {
    std::fs::read_to_string("/etc/retroos/startup.conf")
        .unwrap_or_default()
        .lines()
        .any(|l| l.split('#').next().unwrap_or("").trim() == "mode=desktop")
}

fn draw(s: &mut Screen, ui: &Ui) {
    chrome(
        s,
        "RetroOS 1.0",
        &clock(),
        "Enter Log in   Tab Next field   F2 Start in   F9 Restart   F10 Shut down",
    );
    let (w, h) = (56u16, 13u16);
    let x = s.cols.saturating_sub(w) / 2;
    let y = s.rows.saturating_sub(h) / 2;
    s.center(
        y.saturating_sub(3),
        "Phosphor Computer Company",
        LEMON,
        COBALT,
    );
    s.dialog(x, y, w, h, "Log In", BLACK, SILVER);
    let (lx, fx) = (x + 4, x + 20);
    s.text(lx, y + 2, "Enter your name and password.", BLACK, SILVER);
    s.text(lx, y + 4, "Name:", BLACK, SILVER);
    s.text(lx, y + 6, "Password:", BLACK, SILVER);
    let fw = 30;
    let cn = ui.name.draw(
        s,
        fx,
        y + 4,
        fw,
        WHITE,
        if ui.focus == Focus::Name {
            TEAL
        } else {
            COBALT
        },
    );
    let cp = ui.pass.draw(
        s,
        fx,
        y + 6,
        fw,
        WHITE,
        if ui.focus == Focus::Password {
            TEAL
        } else {
            COBALT
        },
    );
    s.text(lx, y + 8, "Start in:", BLACK, SILVER);
    s.text(
        fx,
        y + 8,
        if ui.desktop {
            "RetroOS Desktop"
        } else {
            "RetroDOS prompt"
        },
        BLACK,
        SILVER,
    );
    s.text(fx + 17, y + 8, "(F2)", SLATE, SILVER);
    if !ui.message.is_empty() {
        let m: String = ui.message.chars().take(w as usize - 8).collect();
        s.text(lx, y + 10, &m, BRICK, SILVER);
    } else if ui.busy {
        s.text(lx, y + 10, "Checking...", BLACK, SILVER);
    }
    if !ui.users.is_empty() && ui.users.len() <= 8 {
        s.center(
            y + h + 2,
            &format!("Users: {}", ui.users.join(", ")),
            SILVER,
            COBALT,
        );
    }
    s.cursor = Some(if ui.focus == Focus::Name {
        (cn, y + 4)
    } else {
        (cp, y + 6)
    });
}

/// Authenticate and start the session. Ok(true) = logged in.
fn login(ui: &mut Ui) -> Result<bool, String> {
    let user = ui.name.value.trim().to_lowercase();
    let mode = if ui.desktop { "desktop" } else { "text" };
    let mut g = match Greetd::connect() {
        Ok(g) => g,
        Err(_) => {
            // Demo mode (no greetd): report what would happen.
            eprintln!("retro-greeter (demo): would log in {user} in {mode} mode");
            return Ok(true);
        }
    };
    let mut reply = g.create_session(&user).map_err(|e| e.to_string())?;
    let mut sent_password = false;
    loop {
        match reply {
            Reply::Success => break,
            Reply::Prompt(true, _) if !sent_password => {
                sent_password = true;
                reply = g.respond(Some(&ui.pass.value)).map_err(|e| e.to_string())?;
            }
            Reply::Prompt(_, p) => {
                let _ = g.cancel();
                return Err(if p.is_empty() {
                    "Your account needs attention. Ask your administrator.".into()
                } else {
                    p
                });
            }
            Reply::Message(_) => reply = g.respond(None).map_err(|e| e.to_string())?,
            Reply::Error(_) => {
                let _ = g.cancel();
                return Ok(false);
            }
        }
    }
    match g
        .start_session(&["retro-session", "--mode", mode], &[])
        .map_err(|e| e.to_string())?
    {
        Reply::Success => Ok(true),
        Reply::Error(e) | Reply::Message(e) | Reply::Prompt(_, e) => {
            let _ = g.cancel();
            Err(e)
        }
    }
}

fn power(action: &str) {
    let _ = std::process::Command::new("systemctl").arg(action).status();
}

fn main() {
    let mut s = match Screen::open() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("retro-greeter: {e}");
            std::process::exit(1);
        }
    };
    let mut ui = Ui {
        name: Field::new(32, false),
        pass: Field::new(128, true),
        focus: Focus::Name,
        desktop: default_mode_desktop(),
        message: String::new(),
        users: local_users(),
        busy: false,
    };
    if ui.users.len() == 1 {
        ui.name.value = ui.users[0].clone();
        ui.focus = Focus::Password;
    }
    loop {
        if s.check_resize() {
            s.invalidate();
        }
        draw(&mut s, &ui);
        let _ = s.flush();
        let key = match read_key(1000) {
            Ok(Some(k)) => k,
            Ok(None) => continue, // clock tick
            Err(_) => std::process::exit(1),
        };
        ui.message.clear();
        match key {
            Key::Tab | Key::Down | Key::Up | Key::BackTab => {
                ui.focus = if ui.focus == Focus::Name {
                    Focus::Password
                } else {
                    Focus::Name
                };
            }
            Key::F(2) => ui.desktop = !ui.desktop,
            Key::F(9) => power("reboot"),
            Key::F(10) => power("poweroff"),
            Key::Enter if ui.focus == Focus::Name => {
                if !ui.name.value.trim().is_empty() {
                    ui.focus = Focus::Password;
                }
            }
            Key::Enter => {
                if ui.name.value.trim().is_empty() {
                    ui.focus = Focus::Name;
                    continue;
                }
                ui.busy = true;
                draw(&mut s, &ui);
                let _ = s.flush();
                match login(&mut ui) {
                    Ok(true) => {
                        s.clear(SILVER, BLACK);
                        s.cursor = None;
                        let _ = s.flush();
                        return;
                    }
                    Ok(false) => {
                        ui.message = "Incorrect name or password. Try again.".into();
                        ui.pass.value.clear();
                    }
                    Err(e) => ui.message = e,
                }
                ui.busy = false;
            }
            k => {
                match ui.focus {
                    Focus::Name => ui.name.edit(&k),
                    Focus::Password => ui.pass.edit(&k),
                };
            }
        }
    }
}

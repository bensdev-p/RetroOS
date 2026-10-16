//! retro-setup — RetroOS first-boot Setup (SETUP.EXE).
//!
//! Runs once, as root, before the login screen: creates the first user (an
//! administrator), names the computer, sets the time zone, keyboard layout
//! and startup options. `--dry-run` shows the plan without changing anything.

use retro_login::chrome;
use retro_tui::{read_key, Field, Key, Screen, BLACK, BRICK, COBALT, LEMON, SILVER, TEAL, WHITE};
use std::path::Path;
use std::process::{Command, Stdio};

const DONE: &str = "/var/lib/retroos/setup-done";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Page {
    Welcome,
    User,
    Computer,
    Startup,
    Confirm,
    Finished,
}

struct Setup {
    page: Page,
    fullname: Field,
    username: Field,
    pass1: Field,
    pass2: Field,
    hostname: Field,
    timezone: Field,
    keyboard: Field,
    desktop: bool,
    autologin: bool,
    focus: usize,
    error: String,
    dry_run: bool,
    log: Vec<String>,
}

fn suggest_username(full: &str) -> String {
    let first = full.split_whitespace().next().unwrap_or("");
    let u: String = first
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(16)
        .collect();
    if u.is_empty() || u.chars().next().unwrap().is_ascii_digit() {
        "user".into()
    } else {
        u
    }
}

pub fn valid_username(u: &str) -> Result<(), &'static str> {
    const RESERVED: &[&str] = &[
        "root",
        "admin",
        "daemon",
        "bin",
        "sys",
        "sync",
        "games",
        "man",
        "lp",
        "mail",
        "news",
        "nobody",
        "systemd",
        "messagebus",
        "_greetd",
        "retro-post",
    ];
    let mut it = u.chars();
    let ok_first = it
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c == '_');
    if !ok_first
        || u.len() > 32
        || !u
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    {
        return Err("A user name uses a-z, 0-9, - and _, and starts with a letter.");
    }
    if RESERVED.contains(&u) || user_exists(u) {
        return Err("That user name is already taken.");
    }
    Ok(())
}

fn user_exists(u: &str) -> bool {
    std::fs::read_to_string("/etc/passwd")
        .unwrap_or_default()
        .lines()
        .any(|l| l.split(':').next() == Some(u))
}

pub fn valid_hostname(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 63
        && !h.starts_with('-')
        && !h.ends_with('-')
        && h.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

fn timezone_ok(tz: &str) -> bool {
    !tz.contains("..") && Path::new("/usr/share/zoneinfo").join(tz).is_file()
}

fn layout_ok(l: &str) -> bool {
    !l.contains('/')
        && !l.contains('.')
        && Path::new("/usr/share/X11/xkb/symbols").join(l).is_file()
}

fn current_timezone() -> String {
    std::fs::read_to_string("/etc/timezone")
        .map(|s| s.trim().to_string())
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "UTC".into())
}

impl Setup {
    fn fields_on_page(&self) -> usize {
        match self.page {
            Page::User => 4,
            Page::Computer => 3,
            Page::Startup => 2,
            _ => 0,
        }
    }

    fn field(&mut self, i: usize) -> Option<&mut Field> {
        match (self.page, i) {
            (Page::User, 0) => Some(&mut self.fullname),
            (Page::User, 1) => Some(&mut self.username),
            (Page::User, 2) => Some(&mut self.pass1),
            (Page::User, 3) => Some(&mut self.pass2),
            (Page::Computer, 0) => Some(&mut self.hostname),
            (Page::Computer, 1) => Some(&mut self.timezone),
            (Page::Computer, 2) => Some(&mut self.keyboard),
            _ => None,
        }
    }

    fn validate(&self) -> Result<(), String> {
        match self.page {
            Page::User => {
                if self.fullname.value.trim().is_empty() {
                    return Err("Type your name.".into());
                }
                valid_username(&self.username.value).map_err(String::from)?;
                if self.pass1.value.is_empty() {
                    return Err("Choose a password.".into());
                }
                if self.pass1.value != self.pass2.value {
                    return Err("The two passwords are different. Type them again.".into());
                }
                Ok(())
            }
            Page::Computer => {
                if !valid_hostname(&self.hostname.value) {
                    return Err("A computer name uses letters, digits and hyphens.".into());
                }
                if !timezone_ok(&self.timezone.value) {
                    return Err(format!(
                        "Unknown time zone {}. Example: America/New_York",
                        self.timezone.value
                    ));
                }
                if !layout_ok(&self.keyboard.value) {
                    return Err(format!(
                        "Unknown keyboard layout {}. Examples: us, gb, de, fr",
                        self.keyboard.value
                    ));
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn next(&mut self) {
        if let Err(e) = self.validate() {
            self.error = e;
            return;
        }
        self.error.clear();
        self.focus = 0;
        self.page = match self.page {
            Page::Welcome => Page::User,
            Page::User => Page::Computer,
            Page::Computer => Page::Startup,
            Page::Startup => Page::Confirm,
            Page::Confirm => Page::Finished,
            Page::Finished => Page::Finished,
        };
    }

    fn back(&mut self) {
        self.error.clear();
        self.focus = 0;
        self.page = match self.page {
            Page::User => Page::Welcome,
            Page::Computer => Page::User,
            Page::Startup => Page::Computer,
            Page::Confirm => Page::Startup,
            p => p,
        };
    }

    fn run(&mut self, args: &[&str], stdin: Option<&str>) -> Result<(), String> {
        self.log.push(args.join(" "));
        if self.dry_run {
            return Ok(());
        }
        let mut c = Command::new(args[0]);
        c.args(&args[1..])
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        if stdin.is_some() {
            c.stdin(Stdio::piped());
        }
        let mut child = c.spawn().map_err(|e| format!("{}: {e}", args[0]))?;
        if let (Some(input), Some(mut si)) = (stdin, child.stdin.take()) {
            use std::io::Write;
            let _ = si.write_all(input.as_bytes());
        }
        let out = child.wait_with_output().map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "{} failed: {}",
                args[0],
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }

    fn write(&mut self, path: &str, body: &str) -> Result<(), String> {
        self.log.push(format!("write {path}"));
        if self.dry_run {
            return Ok(());
        }
        if let Some(d) = Path::new(path).parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, body).map_err(|e| format!("{path}: {e}"))
    }

    /// Make the changes.
    fn apply(&mut self) -> Result<(), String> {
        let user = self.username.value.clone();
        let full = self.fullname.value.trim().replace([':', ','], " ");
        let groups: Vec<&str> = [
            "admin",
            "sudo",
            "audio",
            "video",
            "render",
            "input",
            "netdev",
            "bluetooth",
            "plugdev",
            "dialout",
        ]
        .into_iter()
        .filter(|g| {
            std::fs::read_to_string("/etc/group")
                .unwrap_or_default()
                .lines()
                .any(|l| l.split(':').next() == Some(*g))
        })
        .collect();
        let groups = groups.join(",");
        let mut args = vec![
            "useradd",
            "--create-home",
            "--shell",
            "/bin/bash",
            "--comment",
            &full,
        ];
        if !groups.is_empty() {
            args.extend(["--groups", &groups]);
        }
        args.push(&user);
        self.run(&args, None)?;
        let pw = format!("{user}:{}\n", self.pass1.value);
        self.run(&["chpasswd"], Some(&pw))?;

        let host = self.hostname.value.clone();
        self.write("/etc/hostname", &format!("{host}\n"))?;
        let hosts = format!("127.0.0.1\tlocalhost\n127.0.1.1\t{host}\n::1\t\tlocalhost ip6-localhost ip6-loopback\n");
        self.write("/etc/hosts", &hosts)?;
        let _ = self.run(&["hostname", &host], None);

        let tz = self.timezone.value.clone();
        self.write("/etc/timezone", &format!("{tz}\n"))?;
        if !self.dry_run {
            let _ = std::fs::remove_file("/etc/localtime");
            std::os::unix::fs::symlink(format!("/usr/share/zoneinfo/{tz}"), "/etc/localtime")
                .map_err(|e| e.to_string())?;
        }

        let kb = format!(
            "# Written by RetroOS Setup.\nXKBMODEL=\"pc105\"\nXKBLAYOUT=\"{}\"\nXKBVARIANT=\"\"\nXKBOPTIONS=\"\"\nBACKSPACE=\"guess\"\n",
            self.keyboard.value
        );
        self.write("/etc/default/keyboard", &kb)?;

        let conf = std::fs::read_to_string("/etc/retroos/startup.conf").unwrap_or_default();
        let mode = if self.desktop { "desktop" } else { "text" };
        let mut lines: Vec<String> = conf
            .lines()
            .filter(|l| !l.trim_start().starts_with("mode="))
            .map(String::from)
            .collect();
        lines.insert(0, format!("mode={mode}"));
        self.write("/etc/retroos/startup.conf", &(lines.join("\n") + "\n"))?;

        let mut greetd = String::from(
            "# Written by RetroOS Setup.\n[terminal]\nvt = 1\n\n[default_session]\ncommand = \"retro-login-screen\"\nuser = \"_greetd\"\n",
        );
        if self.autologin {
            greetd.push_str(&format!(
                "\n[initial_session]\ncommand = \"retro-session\"\nuser = \"{user}\"\n"
            ));
        }
        self.write("/etc/greetd/retroos.toml", &greetd)?;
        if Path::new("/usr/bin/ssh-keygen").exists() {
            let _ = self.run(&["ssh-keygen", "-A"], None);
        }
        self.write(DONE, "done\n")?;
        Ok(())
    }

    fn draw(&self, s: &mut Screen) {
        chrome(
            s,
            "RetroOS Setup",
            "",
            match self.page {
                Page::Welcome => "Enter Continue   F3 Exit",
                Page::Confirm => "Enter Set up RetroOS   Esc Back",
                Page::Finished => "Enter Start RetroOS",
                _ => "Enter Continue   Tab Next field   Esc Back",
            },
        );
        let (w, h) = (64u16, 17u16);
        let x = s.cols.saturating_sub(w) / 2;
        let y = s.rows.saturating_sub(h) / 2;
        let title = match self.page {
            Page::Welcome => "Welcome",
            Page::User => "Your Account",
            Page::Computer => "Your Computer",
            Page::Startup => "Startup",
            Page::Confirm => "Ready to Set Up",
            Page::Finished => "Setup Complete",
        };
        s.dialog(x, y, w, h, title, BLACK, SILVER);
        let lx = x + 3;
        let fx = x + 24;
        let fw = w - 28;
        let mut cursor = None;
        let label = |s: &mut Screen, row: u16, t: &str| {
            s.text(lx, y + row, t, BLACK, SILVER);
        };
        match self.page {
            Page::Welcome => {
                for (i, l) in [
                    "Welcome to RetroOS 1.0.",
                    "",
                    "Setup prepares your computer for use. It creates your",
                    "account, names the computer, and sets the time zone and",
                    "keyboard.",
                    "",
                    "To continue, press ENTER.",
                ]
                .iter()
                .enumerate()
                {
                    label(s, 2 + i as u16, l);
                }
            }
            Page::User | Page::Computer => {
                let rows: &[(&str, &Field)] = if self.page == Page::User {
                    &[
                        ("Your name:", &self.fullname),
                        ("User name:", &self.username),
                        ("Password:", &self.pass1),
                        ("Type it again:", &self.pass2),
                    ]
                } else {
                    &[
                        ("Computer name:", &self.hostname),
                        ("Time zone:", &self.timezone),
                        ("Keyboard layout:", &self.keyboard),
                    ]
                };
                for (i, (l, f)) in rows.iter().enumerate() {
                    let row = 2 + i as u16 * 2;
                    label(s, row, l);
                    let bg = if i == self.focus { TEAL } else { COBALT };
                    let cx = f.draw(s, fx, y + row, fw, WHITE, bg);
                    if i == self.focus {
                        cursor = Some((cx, y + row));
                    }
                }
                if self.page == Page::User {
                    label(s, 11, "You will be an administrator of this computer.");
                } else {
                    label(
                        s,
                        9,
                        "Examples: Europe/London, America/New_York, Asia/Tokyo",
                    );
                }
            }
            Page::Startup => {
                let opts = [
                    (
                        "Start in:",
                        if self.desktop {
                            "RetroOS Desktop"
                        } else {
                            "RetroDOS prompt"
                        },
                    ),
                    (
                        "Log in automatically:",
                        if self.autologin { "Yes" } else { "No" },
                    ),
                ];
                for (i, (l, v)) in opts.iter().enumerate() {
                    let row = 2 + i as u16 * 2;
                    label(s, row, l);
                    let (fg, bg) = if i == self.focus {
                        (BLACK, TEAL)
                    } else {
                        (LEMON, COBALT)
                    };
                    s.text(fx, y + row, &format!("[ {v:<17} ]"), fg, bg);
                }
                label(s, 7, "Press SPACE to change the selected option.");
                label(s, 9, "Automatic login skips the password at startup.");
                label(s, 10, "Choose No if other people use this computer.");
            }
            Page::Confirm => {
                let rows = [
                    format!("Name:            {}", self.fullname.value),
                    format!("User name:       {}", self.username.value),
                    format!("Computer name:   {}", self.hostname.value),
                    format!("Time zone:       {}", self.timezone.value),
                    format!("Keyboard:        {}", self.keyboard.value),
                    format!(
                        "Start in:        {}",
                        if self.desktop {
                            "RetroOS Desktop"
                        } else {
                            "RetroDOS prompt"
                        }
                    ),
                    format!(
                        "Automatic login: {}",
                        if self.autologin { "Yes" } else { "No" }
                    ),
                ];
                for (i, r) in rows.iter().enumerate() {
                    label(s, 2 + i as u16, r);
                }
                label(s, 11, "Press ENTER to set up RetroOS, or ESC to go back.");
            }
            Page::Finished => {
                label(s, 2, "RetroOS is ready.");
                label(
                    s,
                    4,
                    &format!(
                        "Log in as {} with the password you chose.",
                        self.username.value
                    ),
                );
                label(
                    s,
                    6,
                    "Type HELP at the C:\\> prompt for a list of commands.",
                );
                if self.dry_run {
                    label(
                        s,
                        8,
                        &format!("(dry run: {} steps not carried out)", self.log.len()),
                    );
                }
                label(s, 11, "Press ENTER to start RetroOS.");
            }
        }
        if !self.error.is_empty() {
            let e: String = self.error.chars().take(w as usize - 6).collect();
            s.text(lx, y + h - 3, &e, BRICK, SILVER);
        }
        s.cursor = cursor;
    }
}

fn main() {
    let dry_run = std::env::args().any(|a| a == "--dry-run");
    if !dry_run && Path::new(DONE).exists() {
        return;
    }
    if !dry_run && unsafe { libc::geteuid() } != 0 {
        eprintln!("retro-setup: must run as root (or use --dry-run)");
        std::process::exit(1);
    }
    let mut st = Setup {
        page: Page::Welcome,
        fullname: Field::new(48, false),
        username: Field::new(32, false),
        pass1: Field::new(128, true),
        pass2: Field::new(128, true),
        hostname: Field {
            value: "retroos".into(),
            secret: false,
            max: 63,
        },
        timezone: Field {
            value: current_timezone(),
            secret: false,
            max: 64,
        },
        keyboard: Field {
            value: "us".into(),
            secret: false,
            max: 32,
        },
        desktop: false,
        autologin: true,
        focus: 0,
        error: String::new(),
        dry_run,
        log: Vec::new(),
    };
    let mut s = Screen::open().expect("terminal");
    loop {
        if s.check_resize() {
            s.invalidate();
        }
        st.draw(&mut s);
        let _ = s.flush();
        let Ok(Some(k)) = read_key(-1) else { continue };
        let n = st.fields_on_page();
        match (st.page, &k) {
            (Page::Welcome, Key::F(3)) => std::process::exit(2),
            (Page::Finished, Key::Enter) => break,
            (Page::Confirm, Key::Enter) => {
                st.error = "Setting up...".into();
                st.draw(&mut s);
                let _ = s.flush();
                match st.apply() {
                    Ok(()) => {
                        st.error.clear();
                        st.next();
                    }
                    Err(e) => st.error = e,
                }
            }
            (_, Key::Esc) if st.page != Page::Welcome => st.back(),
            (_, Key::Tab) | (_, Key::Down) if n > 0 => st.focus = (st.focus + 1) % n,
            (_, Key::BackTab) | (_, Key::Up) if n > 0 => st.focus = (st.focus + n - 1) % n,
            (Page::Startup, Key::Char(' ')) => {
                if st.focus == 0 {
                    st.desktop = !st.desktop;
                } else {
                    st.autologin = !st.autologin;
                }
            }
            (_, Key::Enter) => {
                // Enter moves to the next field; on the last field it continues.
                if n > 0 && st.focus + 1 < n {
                    st.focus += 1;
                } else {
                    st.next();
                }
            }
            (p, k) => {
                let was_full = st.fullname.value.clone();
                let focus = st.focus;
                if let Some(f) = st.field(focus) {
                    f.edit(k);
                }
                // Keep the suggested user name in step with the full name until edited.
                if p == Page::User
                    && focus == 0
                    && (st.username.value.is_empty()
                        || st.username.value == suggest_username(&was_full))
                {
                    st.username.value = suggest_username(&st.fullname.value);
                }
            }
        }
    }
    drop(s);
    if dry_run {
        for l in &st.log {
            println!("{l}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(suggest_username("Ada Lovelace"), "ada");
        assert_eq!(suggest_username("  "), "user");
        assert!(valid_username("ada").is_ok());
        assert!(valid_username("root").is_err());
        assert!(valid_username("9lives").is_err());
        assert!(valid_username("Ada").is_err());
        assert!(valid_hostname("my-pi5"));
        assert!(!valid_hostname("-bad"));
        assert!(!valid_hostname("has space"));
    }
}

//! VER, MEM, VOL, DATE, TIME, CLS, COLOR, BEEP, UNIX, COMMAND, SHUTDOWN, RESTART, EXIT.

use super::{Builtin, Pager};
use crate::fmt::{date_long, now, thousands, time_long};
use crate::parse::Args;
use crate::shell::{err, Io, VERSION};
use crate::sysinfo;
use crate::term;
use std::io::Write;
use std::path::PathBuf;

pub const VER: Builtin = Builtin {
    names: &["VER"],
    summary: "Displays the RetroDOS version.",
    usage: "VER [/R]\n\n  /R   Also displays the operating system kernel and hardware details.",
    run: |_, a, io| {
        let u = sysinfo::uname();
        let _ = writeln!(
            io.out,
            "\nRetroDOS Version {VERSION}  ({} {})",
            u.release, u.machine
        );
        if a.has("R") {
            let model = std::fs::read_to_string("/proc/device-tree/model")
                .map(|s| s.trim_end_matches('\0').to_string())
                .unwrap_or_else(|_| "Unknown computer".into());
            let _ = writeln!(io.out, "RetroOS {VERSION} on {model}");
            let _ = writeln!(io.out, "Computer name: {}", u.nodename);
        }
        0
    },
};

pub const MEM: Builtin = Builtin {
    names: &["MEM"],
    summary: "Displays the amount of used and free memory in your system.",
    usage: "MEM [/C | /F]\n\n  /C   Classifies programs by memory usage (largest first).\n  /F   Displays the amount of free memory.",
    run: |_, a, io| {
        let m = sysinfo::meminfo();
        let g = |k: &str| m.get(k).copied().unwrap_or(0);
        let total = g("MemTotal");
        let avail = g("MemAvailable");
        let used = total.saturating_sub(avail);
        let swap_t = g("SwapTotal");
        let swap_f = g("SwapFree");
        let k = |b: u64| format!("{}K", thousands(b / 1024));
        if a.has("F") {
            let _ = writeln!(io.out, "\nFree Conventional Memory: {:>14}", k(avail));
            let _ = writeln!(io.out, "Free Extended Memory:     {:>14}", k(swap_f));
            return 0;
        }
        if a.has("C") {
            let _ = writeln!(io.out, "\nPrograms in memory:\n");
            let _ = writeln!(io.out, "  Name                 PID      Size");
            let _ = writeln!(io.out, "  ----------------  -------  -------------");
            let mut pager = Pager::new(true);
            for p in sysinfo::processes().into_iter().take(40) {
                let _ = writeln!(io.out, "  {:<16}  {:>7}  {:>13}", p.name.to_uppercase(), p.pid, k(p.rss));
                if !pager.line(io) {
                    break;
                }
            }
            let _ = writeln!(io.out);
        }
        let _ = writeln!(io.out, "\nMemory Type          Total   =     Used   +     Free");
        let _ = writeln!(io.out, "----------------  -----------  -----------  -----------");
        let row = |name: &str, t: u64, u: u64, f: u64| format!("{name:<16}  {:>11}  {:>11}  {:>11}", k(t), k(u), k(f));
        let _ = writeln!(io.out, "{}", row("Conventional", total, used, avail));
        let _ = writeln!(io.out, "{}", row("Extended (zram)", swap_t, swap_t.saturating_sub(swap_f), swap_f));
        let _ = writeln!(io.out, "----------------  -----------  -----------  -----------");
        let _ = writeln!(
            io.out,
            "{}",
            row("Total memory", total + swap_t, used + swap_t.saturating_sub(swap_f), avail + swap_f)
        );
        let _ = writeln!(io.out, "\nLargest executable program size    {:>11}", k(avail));
        let _ = writeln!(io.out, "RetroDOS is resident in the high memory area.");
        0
    },
};

pub const VOL: Builtin = Builtin {
    names: &["VOL"],
    summary: "Displays the disk volume label and serial number, if they exist.",
    usage: "VOL [drive:]",
    run: |sh, a, io| {
        let letter = a
            .words
            .first()
            .and_then(|w| w.chars().next())
            .map(|c| c.to_ascii_uppercase())
            .unwrap_or(sh.dos.current_drive());
        let Some(d) = sh.dos.drive(letter) else {
            return err(io, "Invalid drive specification");
        };
        let _ = writeln!(io.out, " Volume in drive {} is {}", d.letter, d.label);
        if let Some(fs) = sysinfo::statvfs(&d.root) {
            let _ = writeln!(io.out, " Volume Serial Number is {}", fs.serial);
        }
        0
    },
};

fn ntp_active() -> bool {
    std::process::Command::new("timedatectl")
        .args(["show", "-p", "NTP", "--value"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "yes")
        .unwrap_or(false)
}

fn set_clock(value: &str, io: &mut Io) -> i32 {
    if ntp_active() {
        let _ = writeln!(io.out, "The clock is set automatically from the network.");
        let _ = writeln!(
            io.out,
            "Turn off automatic time in Control Panel to set it by hand."
        );
        return 1;
    }
    let ok = std::process::Command::new("timedatectl")
        .args(["set-time", value])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        0
    } else {
        err(
            io,
            "Unable to set the system clock. Use ADMIN DATE to try as an administrator.",
        )
    }
}

fn read_answer(prompt: &str) -> String {
    print!("{prompt}");
    let _ = std::io::stdout().flush();
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
    s.trim().to_string()
}

pub const DATE: Builtin = Builtin {
    names: &["DATE"],
    summary: "Displays or sets the date.",
    usage: "DATE [/T | date]\n\n  /T   Displays the date without asking for a new one.\n\nType DATE without parameters to display the current date setting and\na prompt for a new one.  Press ENTER to keep the same date.",
    run: |_, a, io| {
        let _ = writeln!(io.out, "Current date is {}", date_long(now()));
        let v = match a.words.first() {
            Some(v) => v.clone(),
            None if a.has("T") || !io.tty => return 0,
            None => read_answer("Enter new date (mm-dd-yyyy): "),
        };
        if v.is_empty() {
            return 0;
        }
        let parts: Vec<&str> = v.split(['-', '/', '.']).collect();
        let (m, d, y) = match parts.as_slice() {
            [m, d, y] => (m.parse::<u32>(), d.parse::<u32>(), y.parse::<i32>()),
            _ => return err(io, "Invalid date"),
        };
        let (Ok(m), Ok(d), Ok(mut y)) = (m, d, y) else { return err(io, "Invalid date") };
        if y < 100 {
            y += if y < 80 { 2000 } else { 1900 };
        }
        if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
            return err(io, "Invalid date");
        }
        let tm = crate::fmt::local(now());
        set_clock(&format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", tm.hour, tm.min, tm.sec), io)
    },
};

pub const TIME: Builtin = Builtin {
    names: &["TIME"],
    summary: "Displays or sets the system time.",
    usage: "TIME [/T | time]\n\n  /T   Displays the time without asking for a new one.\n\nType TIME with no parameters to display the current time setting and a prompt\nfor a new one.  Press ENTER to keep the same time.",
    run: |_, a, io| {
        let _ = writeln!(io.out, "Current time is {}", time_long(now()));
        let v = match a.words.first() {
            Some(v) => v.clone(),
            None if a.has("T") || !io.tty => return 0,
            None => read_answer("Enter new time: "),
        };
        if v.is_empty() {
            return 0;
        }
        let nums: Vec<u32> = v.split([':', '.']).filter_map(|s| s.trim_end_matches(['a', 'p', 'A', 'P']).parse().ok()).collect();
        let pm = v.to_ascii_lowercase().ends_with('p');
        let (mut h, mi, s) = match nums.as_slice() {
            [h] => (*h, 0, 0),
            [h, m] => (*h, *m, 0),
            [h, m, s, ..] => (*h, *m, *s),
            _ => return err(io, "Invalid time"),
        };
        if pm && h < 12 {
            h += 12;
        }
        if h > 23 || mi > 59 || s > 59 {
            return err(io, "Invalid time");
        }
        set_clock(&format!("{h:02}:{mi:02}:{s:02}"), io)
    },
};

pub const CLS: Builtin = Builtin {
    names: &["CLS"],
    summary: "Clears the screen.",
    usage: "CLS",
    run: |_, _, io| {
        let _ = write!(io.out, "\x1b[0m\x1b[H\x1b[2J\x1b[3J");
        0
    },
};

/// IRGB (DOS) colour index → ANSI SGR colour index.
const DOS_TO_ANSI: [u8; 8] = [0, 4, 2, 6, 1, 5, 3, 7];

pub const COLOR: Builtin = Builtin {
    names: &["COLOR"],
    summary: "Sets the default console foreground and background colors.",
    usage: "COLOR [scheme | attr]\n\n  scheme  CLASSIC, PAPER, AMBER, GREEN or CONTRAST.\n  attr    Two hex digits: the first is the background, the second the\n          foreground.  Each digit can be any of these values:\n\n    0 = Black    4 = Brick    8 = Slate    C = Coral\n    1 = Cobalt   5 = Plum     9 = Sky      D = Orchid\n    2 = Fern     6 = Ochre    A = Lime     E = Lemon\n    3 = Teal     7 = Silver   B = Aqua     F = White\n\nType COLOR without an argument to restore the scheme's default colors.\nExample: \"COLOR 1E\" gives Lemon on Cobalt.",
    run: |_, a, io| {
        let arg = a.words.first().map(|s| s.to_ascii_uppercase());
        let retro = std::env::var_os("RETROOS_CONSOLE").is_some();
        match arg.as_deref() {
            None => {
                let _ = write!(io.out, "\x1b]7701;attr=default\x07\x1b[0m");
            }
            Some(h) if h.len() == 2 && h.chars().all(|c| c.is_ascii_hexdigit()) => {
                let v = u8::from_str_radix(h, 16).unwrap();
                let (bg, fg) = (v >> 4, v & 15);
                if bg == fg {
                    return 1; // DOS refuses identical colours, silently.
                }
                let _ = write!(io.out, "\x1b]7701;attr={h}\x07");
                if !retro {
                    let sgr = |c: u8, base: u8| {
                        let bright = if c >= 8 { 60 } else { 0 };
                        base + bright + DOS_TO_ANSI[(c & 7) as usize]
                    };
                    let _ = write!(io.out, "\x1b[{};{}m", sgr(fg, 30), sgr(bg, 40));
                }
            }
            Some(name) => {
                let scheme = match name {
                    "CLASSIC" | "PHOSPHOR" => "classic",
                    "PAPER" => "paper",
                    "AMBER" => "amber",
                    "GREEN" => "green",
                    "CONTRAST" | "HIGH" => "high",
                    _ => return err(io, "Invalid parameter - try COLOR /?"),
                };
                let _ = write!(io.out, "\x1b]7701;scheme={scheme}\x07");
            }
        }
        if retro {
            let _ = write!(io.out, "\x1b[H\x1b[2J");
        }
        0
    },
};

pub const BEEP: Builtin = Builtin {
    names: &["BEEP"],
    summary: "Sounds the PC speaker beep.",
    usage: "BEEP",
    run: |_, _, io| {
        let _ = write!(io.out, "\x07");
        0
    },
};

fn user_shell() -> PathBuf {
    let sh = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.contains("retrodos"));
    sh.map(PathBuf::from)
        .filter(|p| p.exists())
        .or_else(|| {
            ["/bin/bash", "/bin/sh"]
                .iter()
                .map(PathBuf::from)
                .find(|p| p.exists())
        })
        .unwrap_or_else(|| PathBuf::from("/bin/sh"))
}

pub const UNIX: Builtin = Builtin {
    names: &["UNIX", "SH"],
    summary: "Opens a POSIX shell, or runs one POSIX command line.",
    usage: "UNIX [command line]\n\nWithout a command line, starts your POSIX shell (bash). Type EXIT to return\nto RetroDOS. With a command line, runs it with the POSIX shell and returns.\nExample: UNIX ls -la | less",
    run: |sh, a, io| {
        let shell = user_shell();
        let mut raw = a.raw.trim();
        // UNIX "a > b; c" passes the quoted text to the POSIX shell untouched.
        if raw.len() >= 2 && raw.starts_with('"') && raw.ends_with('"') {
            raw = &raw[1..raw.len() - 1];
        }
        let mut cmd = sh.command(&shell, &[]);
        if raw.is_empty() {
            let _ = writeln!(io.out, "\nStarting the POSIX shell. Type EXIT to return to RetroDOS.\n");
            let _ = io.out.flush();
            cmd.env("RETRODOS_PARENT", "1");
        } else {
            cmd.arg("-c").arg(raw);
        }
        if !io.tty {
            // Inside a pipeline or redirection: pass the output through.
            cmd.stdout(std::process::Stdio::piped());
            if let Some(data) = io.read_input() {
                cmd.stdin(std::process::Stdio::piped());
                let Ok(mut child) = cmd.spawn() else { return err(io, "Bad command or file name") };
                if let Some(mut si) = child.stdin.take() {
                    std::thread::spawn(move || {
                        let _ = si.write_all(&data);
                    });
                }
                if let Some(mut so) = child.stdout.take() {
                    let _ = std::io::copy(&mut so, io.out);
                }
                return crate::shell::exit_code(child.wait());
            }
            return match cmd.output() {
                Ok(o) => {
                    let _ = io.out.write_all(&o.stdout);
                    o.status.code().unwrap_or(1)
                }
                Err(_) => err(io, "Bad command or file name"),
            };
        }
        sh.wait(cmd)
    },
};

pub const COMMAND: Builtin = Builtin {
    names: &["COMMAND"],
    summary: "Starts a new instance of the RetroDOS command interpreter.",
    usage: "COMMAND [/C string | /K string]\n\n  /C string  Carries out the command specified by string, and then stops.\n  /K string  Carries out the command specified by string but remains.",
    run: |sh, a, _io| {
        let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("retrodos"));
        let mut args: Vec<String> = Vec::new();
        for s in &a.switches {
            args.push(format!("/{s}"));
        }
        args.extend(a.words.iter().cloned());
        let cmd = sh.command(&exe, &args);
        sh.wait(cmd)
    },
};

fn power(a: &Args, io: &mut Io, verb: &str, question: &str, action: &str) -> i32 {
    if !a.has("Y") && term::ask(question, "YN") != 'Y' {
        return 0;
    }
    let _ = writeln!(io.out, "{verb}");
    let _ = io.out.flush();
    let ok = std::process::Command::new("systemctl")
        .arg(action)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        0
    } else {
        err(io, "Unable to do that from here. Ask your administrator.")
    }
}

pub const SHUTDOWN: Builtin = Builtin {
    names: &["SHUTDOWN"],
    summary: "Shuts down the computer.",
    usage: "SHUTDOWN [/Y]\n\n  /Y   Does not ask for confirmation.",
    run: |_, a, io| {
        power(
            a,
            io,
            "Shutting down...",
            "Shut down the computer (Y/N)? ",
            "poweroff",
        )
    },
};

pub const RESTART: Builtin = Builtin {
    names: &["RESTART", "REBOOT"],
    summary: "Restarts the computer.",
    usage: "RESTART [/Y]\n\n  /Y   Does not ask for confirmation.",
    run: |_, a, io| {
        power(
            a,
            io,
            "Restarting...",
            "Restart the computer (Y/N)? ",
            "reboot",
        )
    },
};

pub const LOGOFF: Builtin = Builtin {
    names: &["LOGOFF", "LOGOUT"],
    summary: "Ends your session and returns to the login screen.",
    usage: "LOGOFF [/Y]\n\n  /Y   Does not ask for confirmation.",
    run: |_, a, io| {
        if !a.has("Y") && term::ask("End your session (Y/N)? ", "YN") != 'Y' {
            return 0;
        }
        let id = std::env::var("XDG_SESSION_ID").unwrap_or_default();
        let ok = std::process::Command::new("loginctl")
            .args(["terminate-session", &id])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ok {
            0
        } else {
            err(io, "Unable to end the session from here.")
        }
    },
};

pub const EXIT: Builtin = Builtin {
    names: &["EXIT"],
    summary: "Quits the RetroDOS command interpreter.",
    usage: "EXIT [code]",
    run: |sh, a, _| {
        sh.exit = true;
        a.words
            .first()
            .and_then(|w| w.parse().ok())
            .unwrap_or(sh.errorlevel)
    },
};

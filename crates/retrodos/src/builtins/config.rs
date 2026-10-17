//! MODE, KEYB, ADMIN, ABOUT.

use super::Builtin;
use crate::fmt::thousands;
use crate::parse::Args;
use crate::shell::{err, Io, Shell};
use crate::{sysinfo, term};

fn in_console() -> bool {
    std::env::var_os("RETROOS_CONSOLE").is_some()
}

pub const MODE: Builtin = Builtin {
    names: &["MODE"],
    summary: "Displays or changes the console screen size.",
    usage: "MODE [CON]\nMODE CON [COLS=c] [LINES=n]\nMODE CON RESET\n\n  COLS=c   Columns, for example 80 or 132.\n  LINES=n  Lines, for example 25, 43 or 50.\n  RESET    Fill the screen again (the default).\n\nA fixed size is centred on the screen with a border, like a monitor.",
    run: mode,
};

fn mode(_sh: &mut Shell, a: &Args, io: &mut Io) -> i32 {
    let words: Vec<String> = a.words.iter().map(|w| w.to_ascii_uppercase()).collect();
    let dev = words.first().map(String::as_str).unwrap_or("CON");
    if dev != "CON" && !dev.contains('=') {
        return err(io, &format!("Invalid parameter - {dev}"));
    }
    let settings: Vec<&String> = words
        .iter()
        .filter(|w| w.contains('=') || *w == "RESET")
        .collect();
    let (cols, rows) = term::size();
    if settings.is_empty() {
        let layout = keyboard_layout();
        let _ = writeln!(io.out, "\nStatus for device CON:");
        let _ = writeln!(io.out, "----------------------");
        let _ = writeln!(io.out, "    Columns={cols}");
        let _ = writeln!(io.out, "    Lines={rows}");
        let _ = writeln!(io.out, "    Keyboard layout={}", layout.to_uppercase());
        let _ = writeln!(io.out, "    Code page: UTF-8 (CP437 characters available)");
        return 0;
    }
    if !in_console() {
        return err(
            io,
            "MODE CON can only change the size of the RetroOS console",
        );
    }
    let mut want = (cols as usize, rows as usize);
    let mut reset = false;
    for s in settings {
        let (k, v) = s.split_once('=').unwrap_or((s, ""));
        match k {
            "RESET" => reset = true,
            "COLS" => match v.parse::<usize>() {
                Ok(n) if (20..=400).contains(&n) => want.0 = n,
                _ => return err(io, &format!("Invalid parameter - {s}")),
            },
            "LINES" => match v.parse::<usize>() {
                Ok(n) if (5..=200).contains(&n) => want.1 = n,
                _ => return err(io, &format!("Invalid parameter - {s}")),
            },
            _ => return err(io, &format!("Invalid parameter - {s}")),
        }
    }
    let grid = if reset {
        "auto".to_string()
    } else {
        format!("{}x{}", want.0, want.1)
    };
    let _ = write!(io.out, "\x1b]7701;grid={grid}\x07");
    let _ = io.out.flush();
    0
}

/// XKB layout from /etc/default/keyboard ("us", "de", "gb(intl)").
fn keyboard_layout() -> String {
    let text = std::fs::read_to_string("/etc/default/keyboard").unwrap_or_default();
    let get = |key: &str| {
        text.lines()
            .find_map(|l| l.trim().strip_prefix(key))
            .map(|v| v.trim_matches('"').to_string())
            .filter(|v| !v.is_empty())
    };
    let layout = get("XKBLAYOUT=").unwrap_or_else(|| "us".into());
    match get("XKBVARIANT=") {
        Some(v) => format!("{layout}({v})"),
        None => layout,
    }
}

/// DOS KEYB codes and the XKB layouts they mean.
const KEYB_CODES: &[(&str, &str)] = &[
    ("US", "us"),
    ("UK", "gb"),
    ("GR", "de"),
    ("FR", "fr"),
    ("SP", "es"),
    ("IT", "it"),
    ("SG", "ch"),
    ("SF", "ch(fr)"),
    ("NL", "nl"),
    ("BE", "be"),
    ("DK", "dk"),
    ("NO", "no"),
    ("SV", "se"),
    ("SU", "fi"),
    ("PO", "pt"),
    ("BR", "br"),
    ("LA", "latam"),
    ("CF", "ca"),
    ("JP", "jp"),
    ("PL", "pl"),
    ("CZ", "cz"),
    ("HU", "hu"),
    ("RU", "ru"),
    ("TR", "tr"),
];

pub const KEYB: Builtin = Builtin {
    names: &["KEYB"],
    summary: "Shows or changes the keyboard layout.",
    usage: "KEYB [code]\n\n  code   A DOS keyboard code (US, UK, GR, FR, SP, IT, SG, SF, NL, BE, DK,\n         NO, SV, SU, PO, BR, LA, CF, JP, PL, CZ, HU, RU, TR) or a layout\n         name such as de or gb.\n\nChanging the layout needs an administrator password. It takes effect the\nnext time you log on.",
    run: |sh, a, io| {
        let Some(code) = a.words.first() else {
            let cur = keyboard_layout();
            let dos = KEYB_CODES
                .iter()
                .find(|(_, x)| *x == cur)
                .map(|(d, _)| format!(" ({d})"))
                .unwrap_or_default();
            let _ = writeln!(io.out, "Current keyboard layout is {}{dos}", cur.to_uppercase());
            return 0;
        };
        let up = code.to_ascii_uppercase();
        let xkb = KEYB_CODES
            .iter()
            .find(|(d, _)| *d == up)
            .map(|(_, x)| x.to_string())
            .unwrap_or_else(|| code.to_ascii_lowercase());
        if !xkb.chars().all(|c| c.is_ascii_alphanumeric() || "()_-".contains(c)) {
            return err(io, "Invalid keyboard code specified");
        }
        let (layout, variant) = match xkb.split_once('(') {
            Some((l, v)) => (l.to_string(), v.trim_end_matches(')').to_string()),
            None => (xkb.clone(), String::new()),
        };
        let mut args = vec!["localectl".to_string(), "set-x11-keymap".into(), layout];
        if !variant.is_empty() {
            args.extend(["".into(), variant]);
        }
        let _ = io.out.flush();
        let code = run_admin(sh, &args);
        if code == 0 {
            let _ = writeln!(io.out, "Keyboard layout set to {}. It takes effect the next time you log on.", xkb.to_uppercase());
        } else {
            let _ = writeln!(io.out, "The keyboard layout was not changed.");
        }
        code
    },
};

/// Run `argv` as the administrator (sudo asks for the user's password).
fn run_admin(sh: &mut Shell, argv: &[String]) -> i32 {
    let Some(sudo) = sh.find_program("sudo") else {
        println!("Administrator commands are not available.");
        return 1;
    };
    let mut full = vec![
        "-p".to_string(),
        "Administrator password for %p: ".into(),
        "--".into(),
    ];
    full.extend(argv.iter().cloned());
    let cmd = sh.command(&sudo, &full);
    sh.wait(cmd)
}

pub const ADMIN: Builtin = Builtin {
    names: &["ADMIN"],
    summary: "Runs a command as the administrator.",
    usage: "ADMIN [command]\n\nRuns one RetroDOS command line with administrator rights, after asking for\nyour password. Without a command, starts an administrator RetroDOS; type\nEXIT to return. Only members of the admin group may do this.",
    run: |sh, a, io| {
        let me = std::env::current_exe()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "retrodos".into());
        let _ = io.out.flush();
        if a.raw.trim().is_empty() {
            let _ = writeln!(io.out, "\nStarting RetroDOS as the administrator. Type EXIT to return.\n");
            let _ = io.out.flush();
            return run_admin(sh, &[me, "/NOLOGO".into()]);
        }
        run_admin(sh, &[me, "/C".into(), a.raw.trim().to_string()])
    },
};

pub const ABOUT: Builtin = Builtin {
    names: &["ABOUT"],
    summary: "Shows information about this computer.",
    usage: "ABOUT",
    run: |_, _, io| {
        let u = sysinfo::uname();
        let model = std::fs::read_to_string("/proc/device-tree/model")
            .ok()
            .map(|m| m.trim_end_matches('\0').trim().to_string())
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| format!("{} computer", u.machine));
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let mi = sysinfo::meminfo();
        let total = mi.get("MemTotal").copied().unwrap_or(0) >> 20;
        let avail = mi.get("MemAvailable").copied().unwrap_or(0) >> 20;
        let up = std::fs::read_to_string("/proc/uptime")
            .ok()
            .and_then(|s| s.split_whitespace().next()?.parse::<f64>().ok())
            .unwrap_or(0.0) as u64;
        let uptime = match (up / 86400, (up / 3600) % 24, (up / 60) % 60) {
            (0, 0, m) => format!("{m} minute(s)"),
            (0, h, m) => format!("{h} hour(s) {m} minute(s)"),
            (d, h, _) => format!("{d} day(s) {h} hour(s)"),
        };
        let os = std::fs::read_to_string("/usr/share/retroos/version")
            .map(|v| v.trim().to_string())
            .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string());
        let lines = [
            format!("RetroOS {os}   RetroDOS Version {}", crate::shell::VERSION),
            "(C)Copyright Phosphor Computer Company 1987-1993.".to_string(),
            String::new(),
            format!("Computer:   {model}"),
            format!("Name:       {}", u.nodename),
            format!("Processor:  {cpus} x {} ", u.machine),
            format!(
                "Memory:     {} MB ({} MB free)",
                thousands(total),
                thousands(avail)
            ),
            format!("System:     Linux {}", u.release),
            format!("Running:    {uptime}"),
        ];
        let w = lines.iter().map(|l| l.chars().count()).max().unwrap_or(40) + 4;
        let _ = writeln!(io.out, "\n╔{}╗", "═".repeat(w));
        for l in &lines {
            let _ = writeln!(io.out, "║  {l:<width$}  ║", width = w - 4);
        }
        let _ = writeln!(io.out, "╚{}╝", "═".repeat(w));
        0
    },
};

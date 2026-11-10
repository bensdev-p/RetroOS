//! EDIT, FILER, NET, WIFI — RetroOS programs started under their DOS names.
//!
//! These are built-ins so that `EDIT` always means the RetroOS editor (and
//! not, say, run-mailcap's `edit`) and so DOS paths reach the program as
//! real paths.

use super::Builtin;
use crate::parse::Args;
use crate::shell::{err, Io, Shell};

fn run_tool(sh: &mut Shell, a: &Args, io: &mut Io, program: &str) -> i32 {
    let _ = io.out.flush();
    let mut args = Vec::new();
    for w in &a.words {
        match sh.dos.resolve_new(w) {
            Ok(p) => args.push(p.to_string_lossy().into_owned()),
            Err(_) => return err(io, "Invalid path"),
        }
    }
    let Some(prog) = sh.find_program(program) else {
        return err(io, &format!("{program} is not installed"));
    };
    let cmd = sh.command(&prog, &args);
    sh.wait(cmd)
}

pub const EDIT: Builtin = Builtin {
    names: &["EDIT"],
    summary: "Starts the RetroOS editor, which creates and changes text files.",
    usage: "EDIT [[drive:][path]filename]\n\nIn the editor: F10 or Alt opens the menus, F1 shows the keys.",
    run: |sh, a, io| run_tool(sh, a, io, "retro-edit"),
};

pub const FILER: Builtin = Builtin {
    names: &["FILER"],
    summary: "Starts the RetroOS file manager.",
    usage: "FILER [left-directory [right-directory]]\n\nTwo panels side by side: Tab switches, F5 copies, F6 moves, F8 deletes,\nF9 changes drive, F10 quits. F1 lists all keys.",
    run: |sh, a, io| run_tool(sh, a, io, "retro-filer"),
};

pub const NET: Builtin = Builtin {
    names: &["NET"],
    summary: "Shows and changes network settings.",
    usage: "NET [/S]\n\n  /S   Show the network status and return to the prompt.\n\nWithout /S, opens Network Setup (wired and wireless networks).",
    run: |sh, a, io| {
        if !a.has("S") {
            return run_tool(sh, &Args::new(""), io, "retro-net");
        }
        let Some(prog) = sh.find_program("retro-net") else {
            return err(io, "retro-net is not installed");
        };
        match sh.command(&prog, &["--status".to_string()]).output() {
            Ok(o) => {
                let _ = io.out.write_all(&o.stdout);
                o.status.code().unwrap_or(1)
            }
            Err(e) => err(io, &e.to_string()),
        }
    },
};

pub const WIFI: Builtin = Builtin {
    names: &["WIFI"],
    summary: "Joins a wireless network.",
    usage: "WIFI\n\nOpens Network Setup: choose a network with the arrow keys and press Enter.",
    run: |sh, _, io| run_tool(sh, &Args::new(""), io, "retro-net"),
};

pub const CONTROL: Builtin = Builtin {
    names: &["CONTROL"],
    summary: "Changes RetroOS settings (Control Panel).",
    usage: "CONTROL [DISPLAY | STARTUP | TIME | KEYBOARD | MOUSE | DESKTOP | NETWORK |\n         SOUND | USERS | BLUETOOTH | UPDATES | SOURCES]\n\nWithout a name, shows every Control Panel page; with one, opens that page.\n\u{2191}\u{2193} choose a setting, \u{2190}\u{2192} change it, Enter saves, Esc cancels.",
    run: |sh, a, io| {
        let _ = io.out.flush();
        let Some(prog) = sh.find_program("retro-control") else {
            return err(io, "retro-control is not installed");
        };
        let args: Vec<String> = a.words.iter().take(1).cloned().collect();
        let cmd = sh.command(&prog, &args);
        sh.wait(cmd)
    },
};

pub const MIXER: Builtin = Builtin {
    names: &["MIXER"],
    summary: "Starts the volume mixer.",
    usage: "MIXER\n\nOne bar for the sound output in use (MASTER) and one for each program\nplaying. \u{2190}\u{2192} choose, \u{2191}\u{2193} change the volume, M mutes, O picks the output.",
    run: |sh, _, io| run_tool(sh, &Args::new(""), io, "retro-mixer"),
};

pub const BT: Builtin = Builtin {
    names: &["BT"],
    summary: "Pairs Bluetooth devices.",
    usage: "BT [ON | OFF | /S]\n\n  ON    Turns Bluetooth on (it stays on).\n  OFF   Turns Bluetooth off.\n  /S    Shows whether it is on, and the paired devices.\n\nWithout a switch, lists the devices nearby: choose one and press Enter to\npair with it (keyboards, headphones, game pads).",
    run: |sh, a, io| {
        let Some(prog) = sh.find_program("retro-bt") else {
            return err(io, "retro-bt is not installed");
        };
        let arg = match (a.words.first().map(|w| w.to_ascii_uppercase()), a.switches.first()) {
            (None, None) => return run_tool(sh, &Args::new(""), io, "retro-bt"),
            (None, Some(s)) if s == "S" => "--status".into(),
            (None, Some(s)) => return err(io, &format!("Invalid switch - /{s}")),
            (Some(w), _) if w == "ON" || w == "OFF" => w.to_ascii_lowercase(),
            (Some(w), _) => return err(io, &format!("Invalid parameter - {w}")),
        };
        match sh.command(&prog, &[arg]).output() {
            Ok(o) => {
                let _ = io.out.write_all(&o.stdout);
                let _ = io.out.write_all(&o.stderr);
                o.status.code().unwrap_or(1)
            }
            Err(e) => err(io, &e.to_string()),
        }
    },
};

/// Run retro-library with `args` in this console and return its exit code.
fn library(sh: &mut Shell, io: &mut Io, args: Vec<String>) -> i32 {
    let _ = io.out.flush();
    let Some(prog) = sh.find_program("retro-library") else {
        return err(io, "retro-library is not installed");
    };
    let cmd = sh.command(&prog, &args);
    sh.wait(cmd)
}

pub const LIBRARY: Builtin = Builtin {
    names: &["LIBRARY"],
    summary: "Starts the Software Library.",
    usage: "LIBRARY [category]\n\nBrowse the programs RetroOS can install, by category; install, remove\nand update them. Enter installs the chosen program, U installs updates,\n/ searches everything Debian has.",
    run: |sh, a, io| library(sh, io, a.words.iter().take(1).cloned().collect()),
};

pub const INSTALL: Builtin = Builtin {
    names: &["INSTALL"],
    summary: "Installs a program.",
    usage: "INSTALL name [name...]\n\n  name   A program from the Software Library (CHROMIUM, GIMP, DOSBOX-X…) or\n         any Debian package name.\n\nAsks for the administrator's password, then downloads and installs it.\nType LIBRARY to see what there is.",
    run: |sh, a, io| {
        if a.words.is_empty() {
            return err(io, "Required parameter missing");
        }
        let mut args = vec!["--install".to_string()];
        args.extend(a.words.iter().cloned());
        library(sh, io, args)
    },
};

pub const UNINSTALL: Builtin = Builtin {
    names: &["UNINSTALL"],
    summary: "Removes a program.",
    usage: "UNINSTALL name [name...]\n\nRemoves programs INSTALL (or the Software Library) put on, after asking.",
    run: |sh, a, io| {
        if a.words.is_empty() {
            return err(io, "Required parameter missing");
        }
        let mut args = vec!["--remove".to_string()];
        args.extend(a.words.iter().cloned());
        library(sh, io, args)
    },
};

pub const UPDATE: Builtin = Builtin {
    names: &["UPDATE"],
    summary: "Installs software updates.",
    usage: "UPDATE\n\nLists the updates waiting and installs them. RetroOS looks for updates\nevery day and installs security updates by itself (Control Panel → Updates).",
    run: |sh, _, io| library(sh, io, vec!["--update".to_string()]),
};

pub const VOLUME: Builtin = Builtin {
    names: &["VOLUME"],
    summary: "Shows or sets the sound volume.",
    usage: "VOLUME [n]\n\n  n   The volume of the sound output in use, 0 to 100.\n\nWithout n, shows the output and its volume. MIXER changes each program's.",
    run: |sh, a, io| {
        let Some(prog) = sh.find_program("retro-mixer") else {
            return err(io, "retro-mixer is not installed");
        };
        let arg = match a.words.first() {
            Some(n) if n.trim_end_matches('%').parse::<u32>().is_ok_and(|v| v <= 100) => n.clone(),
            Some(n) => return err(io, &format!("Invalid volume - {n}")),
            None => "--status".into(),
        };
        match sh.command(&prog, &[arg]).output() {
            Ok(o) => {
                let _ = io.out.write_all(&o.stdout);
                let _ = io.out.write_all(&o.stderr);
                o.status.code().unwrap_or(1)
            }
            Err(e) => err(io, &e.to_string()),
        }
    },
};

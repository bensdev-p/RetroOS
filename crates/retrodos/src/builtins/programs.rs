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

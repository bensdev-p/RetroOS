//! EDIT, FILER — RetroOS programs started under their DOS names.
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

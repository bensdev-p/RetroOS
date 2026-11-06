//! retro-cursors THEME_DIR: writes the RetroOS pointers (retro_gfx::cursors)
//! as an Xcursor theme: THEME_DIR/index.theme and THEME_DIR/cursors/<name>
//! for every name toolkits ask for, at 16, 32 and 48 pixels (1×, 2×, 3×).
//! Installed as /usr/share/icons/Phosphor; retro-session selects it
//! (XCURSOR_THEME) and Station passes the size that matches its own pointer.

use retro_gfx::cursors::{xcursor, Cursor};
use std::path::PathBuf;

fn main() {
    let Some(dir) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: retro-cursors THEME_DIR");
        std::process::exit(2);
    };
    let cursors = dir.join("cursors");
    if let Err(e) = std::fs::create_dir_all(&cursors) {
        eprintln!("retro-cursors: {}: {e}", cursors.display());
        std::process::exit(1);
    }
    let theme =
        "[Icon Theme]\nName=Phosphor\nComment=RetroOS pointers: Black outline, White fill\n";
    let mut n = 0;
    let mut write = |p: PathBuf, data: &[u8]| {
        if let Err(e) = std::fs::write(&p, data) {
            eprintln!("retro-cursors: {}: {e}", p.display());
            std::process::exit(1);
        }
        n += 1;
    };
    write(dir.join("index.theme"), theme.as_bytes());
    for c in Cursor::ALL {
        let file = xcursor(c, &[1, 2, 3]);
        for name in c.names() {
            write(cursors.join(name), &file);
        }
    }
    println!("retro-cursors: {} files in {}", n, dir.display());
}

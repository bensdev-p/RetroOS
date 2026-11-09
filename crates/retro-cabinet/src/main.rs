//! File Cabinet: the RetroOS Desktop's file manager (docs/DESIGN.md §9.2).
//!
//!     file-cabinet [FOLDER]
//!
//! One window per folder view: a drive bar (the RetroDOS drives: C: is the
//! home folder, removable media D: and on, Z: the whole system), the folder
//! as a DOS path, and its files as a list (name, size, date) or as icons.
//! Double-click or Enter opens a folder or a file (with the program whose
//! .desktop file lists its type; text in Jot). File: Open, New Folder,
//! Rename, Delete (to the Wastebasket, the freedesktop.org trash), Get Info;
//! Edit: Cut, Copy, Paste (files), Select All; View: Icons / List, hidden
//! files, Refresh. The listing and the copy/move/delete code are FILER's.

mod open;
mod trash;

use retro_apps::DesktopApp;
use retro_dosfs::Dos;
use retro_filer::panel::{self, Entry, Panel};
use retro_gfx::font::{lantern_12, lantern_9};
use retro_gfx::icons::{icon_picture, small_icon, IconArt};
use retro_gfx::widgets::{
    bevel, draw_menu, ellipsize, ellipsize_in, menu_hit, menu_layout, push_button, text_width,
    MenuItem,
};
use retro_gfx::{Canvas, Theme};
use retro_ui::controls::{MenuBar, TextField, FIELD_H, MENU_BAR_H};
use retro_ui::keys::*;
use retro_ui::{station, Event, Program, Rect, SurfaceId, Ui};
use std::path::{Path, PathBuf};
use std::time::Instant;

const BTN_LEFT: u32 = 0x110;
const DRIVE_Y: i32 = MENU_BAR_H + 2;
const PATH_Y: i32 = MENU_BAR_H + 28;
const CONTENT_Y: i32 = MENU_BAR_H + 46;
const STATUS_H: i32 = 20;
const HEAD_H: i32 = 18;
const ROW_H: i32 = 18;
const CELL_W: i32 = 76;
const CELL_H: i32 = 58;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum View {
    List,
    Icons,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Cmd {
    None,
    Open,
    NewFolder,
    Rename,
    Delete,
    Info,
    Close,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Icons,
    List,
    Hidden,
    Refresh,
    About,
}

struct Menu {
    title: usize,
    surface: SurfaceId,
    items: Vec<(MenuItem, Cmd)>,
    sel: Option<usize>,
}

enum Kind {
    /// New folder (None) or rename `old`: a name field.
    Name(Option<String>, TextField),
    /// Throw these away (to the Wastebasket, or for good).
    Delete(Vec<PathBuf>, bool),
    Info(Vec<(String, String)>),
    Message(Vec<String>),
}

struct Dialog {
    surface: SurfaceId,
    kind: Kind,
    buttons: Vec<(Rect, &'static str)>,
    pressed: Option<usize>,
}

struct Cabinet {
    win: Option<SurfaceId>,
    size: (i32, i32),
    dos: Dos,
    user: String,
    home: PathBuf,
    panel: Panel,
    view: View,
    /// Scroll offset of the content, in art pixels.
    scroll: i32,
    apps: Vec<DesktopApp>,
    bar: MenuBar,
    menu: Option<Menu>,
    dialog: Option<Dialog>,
    /// Files cut or copied, and whether they were cut.
    clip: Option<(Vec<PathBuf>, bool)>,
    last_click: Option<(usize, Instant)>,
}

/// "1.2 MB" from bytes.
fn size_text(b: u64) -> String {
    const U: [&str; 4] = ["bytes", "KB", "MB", "GB"];
    let mut v = b as f64;
    let mut u = 0;
    while v >= 1024.0 && u < 3 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{b} bytes")
    } else {
        format!("{v:.1} {}", U[u])
    }
}

fn date_text(t: i64) -> String {
    let (y, mo, d, h, mi, _) = trash::local_time(t);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}")
}

/// Size of a file, or of everything in a folder (stopping at 20 000 files).
fn tree_size(p: &Path) -> (u64, usize) {
    let mut total = 0;
    let mut n = 0;
    let mut stack = vec![p.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(md) = std::fs::symlink_metadata(&d) else {
            continue;
        };
        if md.is_dir() {
            if let Ok(rd) = std::fs::read_dir(&d) {
                stack.extend(rd.flatten().map(|e| e.path()));
            }
        } else {
            total += md.len();
            n += 1;
        }
        if n > 20_000 {
            break;
        }
    }
    (total, n)
}

impl Cabinet {
    fn redraw(&self, ui: &mut Ui) {
        if let Some(w) = self.win {
            ui.redraw(w);
        }
    }

    fn title(&self) -> String {
        format!("File Cabinet - {}", self.dos.display(&self.panel.dir))
    }

    /// Re-read the folder; no ".." at the top of a drive.
    fn reload(&mut self) {
        self.panel.reload();
        let (letter, comps) = self.dos.to_dos(&self.panel.dir);
        let at_root = comps.is_empty() && self.dos.drive(letter).is_some();
        if at_root && self.panel.entries.first().is_some_and(|e| e.name == "..") {
            self.panel.entries.remove(0);
            self.panel.cursor = self.panel.cursor.saturating_sub(1);
        }
    }

    fn go(&mut self, ui: &mut Ui, to: &Path) {
        match self.panel.cd(to) {
            Ok(()) => {
                self.scroll = 0;
                self.reload();
                self.reveal();
                if let Some(w) = self.win {
                    ui.set_title(w, &self.title());
                }
            }
            Err(e) => self.message(
                ui,
                "File Cabinet",
                &[&format!("Cannot open {}:", to.display()), &e],
            ),
        }
        self.redraw(ui);
    }

    // ------------------------------------------------------- geometry

    fn content(&self) -> Rect {
        let (w, h) = self.size;
        Rect::new(0, CONTENT_Y, w, (h - CONTENT_Y - STATUS_H).max(0))
    }

    fn per_row(&self) -> usize {
        ((self.content().w - 8) / CELL_W).max(1) as usize
    }

    /// Rectangle of entry `i` in content coordinates (before scrolling).
    fn item_rect(&self, i: usize) -> Rect {
        let c = self.content();
        match self.view {
            View::List => Rect::new(0, HEAD_H + i as i32 * ROW_H, c.w, ROW_H),
            View::Icons => {
                let pr = self.per_row();
                Rect::new(
                    4 + (i % pr) as i32 * CELL_W,
                    4 + (i / pr) as i32 * CELL_H,
                    CELL_W,
                    CELL_H,
                )
            }
        }
    }

    fn content_h(&self) -> i32 {
        let n = self.panel.entries.len();
        match self.view {
            View::List => HEAD_H + n as i32 * ROW_H + 4,
            View::Icons => 8 + n.div_ceil(self.per_row()) as i32 * CELL_H,
        }
    }

    fn clamp_scroll(&mut self) {
        let max = (self.content_h() - self.content().h).max(0);
        self.scroll = self.scroll.clamp(0, max);
    }

    fn reveal(&mut self) {
        let r = self.item_rect(self.panel.cursor);
        let top = if self.view == View::List { HEAD_H } else { 0 };
        let h = self.content().h;
        if r.y - top < self.scroll {
            self.scroll = r.y - top;
        } else if r.y + r.h > self.scroll + h {
            self.scroll = r.y + r.h - h;
        }
        self.clamp_scroll();
    }

    fn drive_rects(&self) -> Vec<(char, Rect)> {
        let mut x = 6;
        self.dos
            .drives()
            .map(|d| {
                let w = text_width(&format!("{}: {}", d.letter, d.label)) + 34;
                let r = Rect::new(x, DRIVE_Y, w, 22);
                x += w + 4;
                (d.letter, r)
            })
            .collect()
    }

    fn selected(&self, i: usize) -> bool {
        let e = &self.panel.entries[i];
        if self.panel.marked.is_empty() {
            i == self.panel.cursor
        } else {
            self.panel.marked.contains(&e.name)
        }
    }

    // ------------------------------------------------------- commands

    fn menu_items(&self, title: usize) -> Vec<(MenuItem, Cmd)> {
        let sep = || (MenuItem::separator(), Cmd::None);
        let it = |l: &str, s: &str, on: bool, c: Cmd| (MenuItem::new(l, s, on), c);
        let some = !self.panel.selection().is_empty();
        let one = self.panel.selection().len() == 1;
        let check = |mut m: (MenuItem, Cmd), on: bool| {
            m.0.checked = on;
            m
        };
        match title {
            0 => vec![
                it("&Open", "Enter", self.panel.current().is_some(), Cmd::Open),
                sep(),
                it("&New Folder...", "", true, Cmd::NewFolder),
                it("&Rename...", "F2", one, Cmd::Rename),
                it("&Delete...", "Del", some, Cmd::Delete),
                it("Get &Info", "Alt+Enter", some, Cmd::Info),
                sep(),
                it("&Close", "Alt+F4", true, Cmd::Close),
            ],
            1 => vec![
                it("Cu&t", "Ctrl+X", some, Cmd::Cut),
                it("&Copy", "Ctrl+C", some, Cmd::Copy),
                it("&Paste", "Ctrl+V", self.clip.is_some(), Cmd::Paste),
                sep(),
                it("Select &All", "Ctrl+A", true, Cmd::SelectAll),
            ],
            2 => vec![
                check(it("&Icons", "", true, Cmd::Icons), self.view == View::Icons),
                check(it("&List", "", true, Cmd::List), self.view == View::List),
                sep(),
                check(
                    it("&Hidden Files", "", true, Cmd::Hidden),
                    self.panel.show_hidden,
                ),
                it("&Refresh", "F5", true, Cmd::Refresh),
            ],
            _ => vec![it("&About File Cabinet...", "", true, Cmd::About)],
        }
    }

    fn close_menu(&mut self, ui: &mut Ui) {
        if let Some(m) = self.menu.take() {
            ui.close(m.surface);
        }
        self.redraw(ui);
    }

    fn open_menu(&mut self, ui: &mut Ui, title: usize) {
        self.close_menu(ui);
        let Some(win) = self.win else { return };
        let items = self.menu_items(title);
        let only: Vec<MenuItem> = items.iter().map(|(i, _)| i.clone()).collect();
        let (w, h, _) = menu_layout(&only);
        let r = self.bar.rects()[title];
        if let Some(s) = ui.popup(win, r, (w + 1, h + 1), false, true) {
            self.menu = Some(Menu {
                title,
                surface: s,
                items,
                sel: None,
            });
        }
        self.redraw(ui);
    }

    fn run(&mut self, ui: &mut Ui, cmd: Cmd) {
        self.close_menu(ui);
        match cmd {
            Cmd::None => {}
            Cmd::Open => self.open_current(ui),
            Cmd::NewFolder => {
                let f = TextField::new("New Folder");
                self.dialog(
                    ui,
                    "New Folder",
                    (320, 96),
                    Kind::Name(None, f),
                    &["OK", "Cancel"],
                );
            }
            Cmd::Rename => {
                if let Some(e) = self.panel.current().filter(|e| e.name != "..") {
                    let n = e.name.clone();
                    let f = TextField::new(&n);
                    self.dialog(
                        ui,
                        "Rename",
                        (320, 96),
                        Kind::Name(Some(n), f),
                        &["OK", "Cancel"],
                    );
                }
            }
            Cmd::Delete => self.ask_delete(ui, false),
            Cmd::Info => self.info(ui),
            Cmd::Close => ui.quit(),
            Cmd::Cut | Cmd::Copy => {
                let files = self.panel.selection();
                if !files.is_empty() {
                    // The paths go on the text clipboard too (for Jot, the DOS Prompt).
                    let text: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
                    ui.set_clipboard(&text.join("\n"));
                    self.clip = Some((files, cmd == Cmd::Cut));
                }
            }
            Cmd::Paste => self.paste(ui),
            Cmd::SelectAll => {
                self.panel.marked = self
                    .panel
                    .entries
                    .iter()
                    .filter(|e| e.name != "..")
                    .map(|e| e.name.clone())
                    .collect();
            }
            Cmd::Icons | Cmd::List => {
                self.view = if cmd == Cmd::Icons {
                    View::Icons
                } else {
                    View::List
                };
                self.scroll = 0;
                self.reveal();
            }
            Cmd::Hidden => {
                self.panel.show_hidden = !self.panel.show_hidden;
                self.reload();
            }
            Cmd::Refresh => {
                self.dos.refresh_removable(&self.user);
                self.reload();
            }
            Cmd::About => self.message(
                ui,
                "About File Cabinet",
                &["File Cabinet", "Version 1.0", "The RetroOS file manager."],
            ),
        }
        self.redraw(ui);
    }

    fn open_current(&mut self, ui: &mut Ui) {
        let Some(e) = self.panel.current().cloned() else {
            return;
        };
        if e.name == ".." {
            let to = self.panel.dir.parent().map(Path::to_path_buf);
            if let Some(to) = to {
                self.go(ui, &to);
            }
            return;
        }
        let path = self.panel.dir.join(&e.name);
        if e.dir {
            return self.go(ui, &path);
        }
        match open::command_for(&path, &self.apps) {
            Some((name, argv)) => {
                let label = name.replace(' ', "").to_uppercase();
                if let Err(err) = station::launch(&label, &argv) {
                    self.message(
                        ui,
                        "File Cabinet",
                        &[&format!("Cannot start {name}:"), &err],
                    );
                }
            }
            None => self.message(
                ui,
                "File Cabinet",
                &[&format!("No program is set to open {}.", e.name)],
            ),
        }
    }

    fn paste(&mut self, ui: &mut Ui) {
        let Some((files, cut)) = self.clip.clone() else {
            return;
        };
        let dest = self.panel.dir.clone();
        let mut errors = Vec::new();
        let mut pasted = Vec::new();
        for f in &files {
            let r = if cut {
                panel::move_into(f, &dest)
            } else {
                panel::copy_into(f, &dest)
            };
            let n = f
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            match r {
                Ok(()) => pasted.push(n),
                Err(e) => errors.push(format!("{n}: {e}")),
            }
        }
        if cut {
            self.clip = None;
        }
        self.reload();
        // What was pasted becomes the selection.
        if let Some(i) = self
            .panel
            .entries
            .iter()
            .position(|e| pasted.contains(&e.name))
        {
            self.panel.cursor = i;
            self.panel.marked = pasted.into_iter().collect();
            self.reveal();
        }
        if !errors.is_empty() {
            let mut lines = vec!["Some files could not be pasted:".to_string()];
            lines.extend(errors.into_iter().take(6));
            let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
            self.message(ui, "File Cabinet", &refs);
        }
    }

    fn ask_delete(&mut self, ui: &mut Ui, for_good: bool) {
        let files = self.panel.selection();
        if files.is_empty() {
            return;
        }
        self.dialog(
            ui,
            "Delete",
            (360, 100),
            Kind::Delete(files, for_good),
            &["Yes", "No"],
        );
    }

    fn info(&mut self, ui: &mut Ui) {
        let files = self.panel.selection();
        let mut rows = Vec::new();
        if let [one] = files.as_slice() {
            let name = one
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let md = std::fs::symlink_metadata(one).ok();
            let dir = md.as_ref().is_some_and(|m| m.is_dir());
            rows.push(("Name:".into(), name.clone()));
            rows.push(("Kind:".into(), open::kind_of(&name, dir).into()));
            let (bytes, n) = tree_size(one);
            let size = if dir {
                format!("{} in {n} file(s)", size_text(bytes))
            } else {
                size_text(bytes)
            };
            rows.push(("Size:".into(), size));
            let wher = one
                .parent()
                .map(|p| self.dos.display(p))
                .unwrap_or_default();
            rows.push(("Where:".into(), wher));
            if let Some(m) = md {
                use std::os::unix::fs::MetadataExt;
                rows.push(("Modified:".into(), date_text(m.mtime())));
            }
        } else {
            let (mut bytes, mut n) = (0, 0);
            for f in &files {
                let (b, k) = tree_size(f);
                bytes += b;
                n += k;
            }
            rows.push(("Selected:".into(), format!("{} items", files.len())));
            rows.push((
                "Size:".into(),
                format!("{} in {n} file(s)", size_text(bytes)),
            ));
        }
        let h = 46 + rows.len() as i32 * 18;
        self.dialog(ui, "Get Info", (380, h), Kind::Info(rows), &["OK"]);
    }

    // -------------------------------------------------------- dialogs

    fn dialog(
        &mut self,
        ui: &mut Ui,
        title: &str,
        size: (i32, i32),
        kind: Kind,
        labels: &[&'static str],
    ) {
        if let Some(d) = self.dialog.take() {
            ui.close(d.surface);
        }
        let Some(win) = self.win else { return };
        let n = labels.len() as i32;
        let bw = 76;
        let x0 = size.0 - n * (bw + 8) - 4;
        let buttons = labels
            .iter()
            .enumerate()
            .map(|(i, l)| (Rect::new(x0 + i as i32 * (bw + 8), size.1 - 32, bw, 22), *l))
            .collect();
        let surface = ui.dialog(win, title, size);
        self.dialog = Some(Dialog {
            surface,
            kind,
            buttons,
            pressed: None,
        });
    }

    fn message(&mut self, ui: &mut Ui, title: &str, lines: &[&str]) {
        let w = lines
            .iter()
            .map(|l| text_width(l))
            .max()
            .unwrap_or(0)
            .max(200)
            + 40;
        let h = 50 + lines.len() as i32 * 16;
        let lines = lines.iter().map(|l| l.to_string()).collect();
        self.dialog(ui, title, (w, h), Kind::Message(lines), &["OK"]);
    }

    fn close_dialog(&mut self, ui: &mut Ui) {
        if let Some(d) = self.dialog.take() {
            ui.close(d.surface);
        }
        self.redraw(ui);
    }

    fn answer(&mut self, ui: &mut Ui, button: usize) {
        let Some(d) = self.dialog.take() else { return };
        ui.close(d.surface);
        if button != 0 {
            return self.redraw(ui);
        }
        match d.kind {
            Kind::Message(_) | Kind::Info(_) => {}
            Kind::Name(old, field) => {
                let name = field.value().trim().to_string();
                if name.is_empty() || name.contains('/') || name == "." || name == ".." {
                    return self.message(ui, "File Cabinet", &["That is not a valid name."]);
                }
                let target = self.panel.dir.join(&name);
                if target.exists() && old.as_deref() != Some(&name) {
                    return self.message(ui, "File Cabinet", &[&format!("{name} already exists.")]);
                }
                let r = match &old {
                    None => std::fs::create_dir(&target),
                    Some(o) => std::fs::rename(self.panel.dir.join(o), &target),
                };
                if let Err(e) = r {
                    return self.message(ui, "File Cabinet", &[&e.to_string()]);
                }
                self.reload();
                if let Some(i) = self.panel.entries.iter().position(|e| e.name == name) {
                    self.panel.cursor = i;
                    self.panel.marked.clear();
                    self.reveal();
                }
            }
            Kind::Delete(files, for_good) => {
                let mut other_fs = Vec::new();
                let mut errors = Vec::new();
                let mut thrown = false;
                for f in &files {
                    let r = if for_good {
                        panel::delete(f)
                    } else {
                        trash::trash(f, &self.home)
                    };
                    match r {
                        Err(e) if e.raw_os_error() == Some(libc::EXDEV) => other_fs.push(f.clone()),
                        Err(e) => errors.push(format!(
                            "{}: {e}",
                            f.file_name().unwrap_or_default().to_string_lossy()
                        )),
                        Ok(()) => thrown |= !for_good,
                    }
                }
                if thrown {
                    // Into the Wastebasket (STYLE.md §6, D-053).
                    let _ = station::request(&["SOUND", "crumple"]);
                }
                self.panel.marked.clear();
                self.reload();
                if !other_fs.is_empty() {
                    // Removable drives have no Wastebasket: ask about deleting.
                    self.dialog(
                        ui,
                        "Delete",
                        (360, 100),
                        Kind::Delete(other_fs, true),
                        &["Yes", "No"],
                    );
                } else if !errors.is_empty() {
                    let refs: Vec<&str> = errors.iter().map(String::as_str).take(6).collect();
                    self.message(ui, "File Cabinet", &refs);
                }
            }
        }
        self.redraw(ui);
    }

    fn dialog_event(&mut self, ui: &mut Ui, ev: Event) {
        let Some(d) = self.dialog.as_mut() else {
            return;
        };
        let id = d.surface;
        match ev {
            Event::Close => self.close_dialog(ui),
            Event::Key {
                sym,
                text,
                ctrl,
                shift,
                ..
            } => match sym {
                ESCAPE => self.close_dialog(ui),
                RETURN | KP_ENTER => self.answer(ui, 0),
                _ => {
                    let c = text
                        .as_deref()
                        .and_then(|t| t.chars().next())
                        .map(|c| c.to_ascii_lowercase());
                    match &mut d.kind {
                        Kind::Name(_, f) => {
                            f.key(sym, text.as_deref(), ctrl, shift);
                            ui.redraw(id);
                        }
                        Kind::Delete(..) if c == Some('y') => self.answer(ui, 0),
                        Kind::Delete(..) if c == Some('n') => self.answer(ui, 1),
                        _ => {}
                    }
                }
            },
            Event::Button {
                button: BTN_LEFT,
                pressed: true,
                x,
                y,
            } => {
                d.pressed = d.buttons.iter().position(|(r, _)| r.contains(x, y));
                if let Kind::Name(_, f) = &mut d.kind {
                    if Rect::new(16, 32, 288, FIELD_H).contains(x, y) {
                        f.click(x - 16);
                    }
                }
                ui.redraw(id);
            }
            Event::Button {
                button: BTN_LEFT,
                pressed: false,
                x,
                y,
            } => {
                let p = d.pressed.take();
                ui.redraw(id);
                if let Some(b) = p.filter(|&b| d.buttons[b].0.contains(x, y)) {
                    self.answer(ui, b);
                }
            }
            _ => {}
        }
    }

    fn draw_dialog(&self, d: &Dialog, c: &mut Canvas, t: &Theme) {
        let mut sf = c.surface();
        sf.clear(t.face);
        let f = lantern_12();
        match &d.kind {
            Kind::Message(lines) => {
                for (i, l) in lines.iter().enumerate() {
                    sf.text(f, 20, 14 + i as i32 * 16, 1, t.text, None, l);
                }
            }
            Kind::Name(old, field) => {
                let label = if old.is_some() {
                    "New name:"
                } else {
                    "Name of the new folder:"
                };
                sf.text(f, 16, 12, 1, t.text, None, label);
                field.draw(&mut sf, 16, 32, 288, true, t);
            }
            Kind::Delete(files, for_good) => {
                let what = match files.as_slice() {
                    [one] => one
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    many => format!("these {} items", many.len()),
                };
                let q = if *for_good {
                    format!("Delete {what} for good?")
                } else {
                    format!("Move {what} to the Wastebasket?")
                };
                sf.text(f, 20, 14, 1, t.text, None, &ellipsize(&q, 320));
                if *for_good {
                    sf.text(f, 20, 32, 1, t.text, None, "This cannot be undone.");
                }
            }
            Kind::Info(rows) => {
                for (i, (k, v)) in rows.iter().enumerate() {
                    let y = 14 + i as i32 * 18;
                    sf.text(f, 16, y, 1, t.text, None, k);
                    sf.text(f, 96, y, 1, t.text, None, &ellipsize(v, 270));
                }
            }
        }
        for (i, (r, l)) in d.buttons.iter().enumerate() {
            push_button(
                &mut sf,
                r.x,
                r.y,
                r.w,
                r.h,
                1,
                l,
                i == 0,
                d.pressed == Some(i),
                t,
            );
        }
    }

    // ------------------------------------------------------ main window

    fn key(
        &mut self,
        ui: &mut Ui,
        sym: u32,
        text: Option<String>,
        ctrl: bool,
        alt: bool,
        shift: bool,
    ) {
        if self.menu.is_some() {
            return self.menu_key(ui, sym);
        }
        if alt && matches!(sym, RETURN | KP_ENTER) {
            return self.run(ui, Cmd::Info);
        }
        if alt || sym == F10 {
            let c = text.as_deref().and_then(|t| t.chars().next());
            match c.and_then(|c| self.bar.by_letter(c)) {
                Some(t) => self.open_menu(ui, t),
                None if sym == F10 => self.open_menu(ui, 0),
                None => {}
            }
            return;
        }
        let n = self.panel.entries.len();
        let pr = if self.view == View::Icons {
            self.per_row() as isize
        } else {
            1
        };
        let page = (self.content().h / ROW_H).max(2) as isize - 1;
        let letter = char::from_u32(sym).map(|c| c.to_ascii_lowercase());
        let mut moved = true;
        match sym {
            UP => self.panel.move_by(-pr),
            DOWN => self.panel.move_by(pr),
            LEFT if self.view == View::Icons => self.panel.move_by(-1),
            RIGHT if self.view == View::Icons => self.panel.move_by(1),
            HOME => self.panel.cursor = 0,
            END => self.panel.cursor = n.saturating_sub(1),
            PAGE_UP => self.panel.move_by(-page * pr),
            PAGE_DOWN => self.panel.move_by(page * pr),
            RETURN | KP_ENTER => return self.open_current(ui),
            BACKSPACE => {
                let to = self.panel.dir.parent().map(Path::to_path_buf);
                let (l, comps) = self.dos.to_dos(&self.panel.dir);
                if let (Some(to), false) = (to, comps.is_empty() && self.dos.drive(l).is_some()) {
                    self.go(ui, &to);
                }
                return;
            }
            DELETE => return self.ask_delete(ui, shift),
            0xffbf => return self.run(ui, Cmd::Rename), // F2
            0xffc2 => return self.run(ui, Cmd::Refresh), // F5
            _ if ctrl => {
                let cmd = match letter {
                    Some('x') => Cmd::Cut,
                    Some('c') => Cmd::Copy,
                    Some('v') => Cmd::Paste,
                    Some('a') => Cmd::SelectAll,
                    _ => return,
                };
                return self.run(ui, cmd);
            }
            _ => match text.as_deref().and_then(|t| t.chars().next()) {
                Some(c) if !c.is_control() => self.panel.jump(c),
                _ => moved = false,
            },
        }
        if moved {
            if !shift {
                self.panel.marked.clear();
            }
            self.reveal();
            self.redraw(ui);
        }
    }

    fn menu_key(&mut self, ui: &mut Ui, sym: u32) {
        let Some(m) = self.menu.as_mut() else { return };
        let n = m.items.len();
        let usable = |i: usize, m: &Menu| m.items[i].0.enabled && !m.items[i].0.is_separator();
        let mut step = |d: isize| {
            let mut i = m
                .sel
                .map(|s| s as isize)
                .unwrap_or(if d > 0 { -1 } else { 0 });
            for _ in 0..n {
                i = (i + d).rem_euclid(n as isize);
                if usable(i as usize, m) {
                    m.sel = Some(i as usize);
                    break;
                }
            }
        };
        match sym {
            UP => step(-1),
            DOWN => step(1),
            ESCAPE => return self.close_menu(ui),
            LEFT | RIGHT => {
                let k = self.bar.titles.len() as isize;
                let t = (m.title as isize + if sym == LEFT { -1 } else { 1 }).rem_euclid(k);
                return self.open_menu(ui, t as usize);
            }
            RETURN | KP_ENTER => {
                if let Some(s) = m.sel {
                    let cmd = m.items[s].1;
                    return self.run(ui, cmd);
                }
            }
            _ => {
                let c = char::from_u32(sym).map(|c| c.to_ascii_lowercase());
                if let Some(i) = m
                    .items
                    .iter()
                    .position(|(it, _)| it.enabled && c.is_some() && it.mnemonic() == c)
                {
                    let cmd = m.items[i].1;
                    return self.run(ui, cmd);
                }
            }
        }
        if let Some(m) = &self.menu {
            ui.redraw(m.surface);
        }
    }

    fn press(&mut self, ui: &mut Ui, x: i32, y: i32) {
        if y < MENU_BAR_H {
            match self.bar.hit(x, y) {
                Some(t) if self.menu.as_ref().map(|m| m.title) == Some(t) => self.close_menu(ui),
                Some(t) => self.open_menu(ui, t),
                None => self.close_menu(ui),
            }
            return;
        }
        if let Some((letter, _)) = self
            .drive_rects()
            .into_iter()
            .find(|(_, r)| r.contains(x, y))
        {
            if let Some(root) = self.dos.drive(letter).map(|d| d.root.clone()) {
                self.go(ui, &root);
            }
            return;
        }
        let c = self.content();
        if !c.contains(x, y) {
            return;
        }
        let (cx, cy) = (x - c.x, y - c.y + self.scroll);
        let hit = (0..self.panel.entries.len()).find(|&i| self.item_rect(i).contains(cx, cy));
        let Some(i) = hit else {
            self.panel.marked.clear();
            return self.redraw(ui);
        };
        let now = Instant::now();
        let double = self.last_click.is_some_and(|(li, t)| {
            li == i && now.duration_since(t) < retro_gfx::prefs::double_click()
        });
        if ui.shift() && self.panel.entries[i].name != ".." {
            // Shift+click adds to (or takes from) the selection.
            if self.panel.marked.is_empty() {
                if let Some(e) = self.panel.current().filter(|e| e.name != "..") {
                    let n = e.name.clone();
                    self.panel.marked.insert(n);
                }
            }
            let n = self.panel.entries[i].name.clone();
            if !self.panel.marked.remove(&n) {
                self.panel.marked.insert(n);
            }
        } else {
            self.panel.marked.clear();
        }
        self.panel.cursor = i;
        self.last_click = Some((i, now));
        if double {
            self.last_click = None;
            self.panel.marked.clear();
            self.open_current(ui);
        }
        self.redraw(ui);
    }

    fn draw_window(&self, c: &mut Canvas, t: &Theme) {
        let (w, h) = (c.width as i32, c.height as i32);
        let mut sf = c.surface();
        sf.clear(t.face);
        let f = lantern_12();
        // Drive bar and path.
        let here = self.dos.to_dos(&self.panel.dir).0;
        for (letter, r) in self.drive_rects() {
            let d = self.dos.drive(letter).expect("listed drive");
            bevel(&mut sf, r.x, r.y, r.w, r.h, 1, letter == here, t);
            sf.frame(r.x, r.y, r.w as u32, r.h as u32, 1, t.line);
            // Home and system drives show a computer; removable ones a page.
            let art = if d.kind == retro_dosfs::DriveKind::Removable {
                IconArt::Document
            } else {
                IconArt::Program
            };
            small_icon(&mut sf, art, r.x + 5, r.y + 3, 1, t);
            sf.text(
                f,
                r.x + 26,
                r.y + 5,
                1,
                t.text,
                None,
                &format!("{letter}: {}", d.label),
            );
        }
        let path = ellipsize(&self.dos.display(&self.panel.dir), w - 16);
        sf.text(f, 8, PATH_Y, 1, t.text, None, &path);
        // Content.
        let cr = self.content();
        sf.fill_rect(cr.x, cr.y, cr.w as u32, cr.h as u32, t.well);
        let oy = cr.y - self.scroll;
        let visible = |r: Rect| r.y + oy + r.h > cr.y && r.y + oy < cr.y + cr.h;
        let f9 = lantern_9();
        let date_x = w - 136;
        let size_r = w - 150;
        for (i, e) in self.panel.entries.iter().enumerate() {
            let r = self.item_rect(i);
            if !visible(r) {
                continue;
            }
            let art = if e.dir {
                IconArt::Folder
            } else {
                IconArt::Document
            };
            let sel = self.selected(i);
            let y = r.y + oy;
            match self.view {
                View::List => {
                    if y < cr.y + HEAD_H {
                        continue;
                    }
                    let fg = if sel {
                        sf.fill_rect(2, y, (w - 4) as u32, ROW_H as u32, t.sel);
                        t.sel_text
                    } else {
                        t.text
                    };
                    small_icon(&mut sf, art, 6, y + 1, 1, t);
                    sf.text(f, 26, y + 3, 1, fg, None, &ellipsize(&e.name, size_r - 120));
                    if !e.dir {
                        let s = size_text(e.size);
                        sf.text(f, size_r - text_width(&s), y + 3, 1, fg, None, &s);
                    }
                    if e.name != ".." {
                        sf.text(f, date_x, y + 3, 1, fg, None, &date_text(e.mtime));
                    }
                }
                View::Icons => {
                    if y < cr.y - 20 {
                        continue;
                    }
                    icon_picture(&mut sf, art, r.x + (CELL_W - 32) / 2, y + 2, 1, t);
                    let label = ellipsize_in(f9, &e.name, CELL_W - 6);
                    let tw = f9.text_width(&label) as i32;
                    let lx = r.x + (CELL_W - tw) / 2;
                    let fg = if sel {
                        sf.fill_rect(lx - 2, y + 37, (tw + 3) as u32, 11, t.sel);
                        t.sel_text
                    } else {
                        t.text
                    };
                    sf.text(f9, lx, y + 38, 1, fg, None, &label);
                }
            }
        }
        if self.view == View::List {
            // Column heads stay put above the scrolling rows.
            bevel(&mut sf, 0, cr.y, w, HEAD_H, 1, false, t);
            sf.text(f, 26, cr.y + 3, 1, t.text, None, "Name");
            sf.text(
                f,
                size_r - text_width("Size"),
                cr.y + 3,
                1,
                t.text,
                None,
                "Size",
            );
            sf.text(f, date_x, cr.y + 3, 1, t.text, None, "Modified");
        }
        sf.frame(cr.x, cr.y, cr.w as u32, cr.h as u32, 1, t.line);
        // Status line.
        let files: Vec<&Entry> = self
            .panel
            .entries
            .iter()
            .filter(|e| e.name != "..")
            .collect();
        let total: u64 = files.iter().filter(|e| !e.dir).map(|e| e.size).sum();
        let sel = self.panel.selection();
        let status = if sel.len() > 1 {
            format!("{} of {} items selected", sel.len(), files.len())
        } else {
            format!("{} items, {}", files.len(), size_text(total))
        };
        sf.text(f, 8, h - STATUS_H + 4, 1, t.text, None, &status);
        // Menu bar last.
        let open = self.menu.as_ref().map(|m| m.title);
        self.bar.draw(&mut sf, w, open, t);
    }
}

impl Program for Cabinet {
    fn start(&mut self, ui: &mut Ui) {
        self.win = Some(ui.window(&self.title(), "retroos.cabinet", self.size, false));
    }

    fn draw(&mut self, ui: &Ui, id: SurfaceId, c: &mut Canvas) {
        if Some(id) == self.win {
            return self.draw_window(c, &ui.theme);
        }
        if let Some(m) = self.menu.as_ref().filter(|m| m.surface == id) {
            let only: Vec<MenuItem> = m.items.iter().map(|(i, _)| i.clone()).collect();
            let menu = draw_menu(&only, m.sel, 1, &ui.theme);
            return c.surface().blit(&menu, 0, 0);
        }
        if let Some(d) = self.dialog.as_ref().filter(|d| d.surface == id) {
            self.draw_dialog(d, c, &ui.theme);
        }
    }

    fn event(&mut self, ui: &mut Ui, id: SurfaceId, ev: Event) {
        if let Some(m) = self.menu.as_mut().filter(|m| m.surface == id) {
            let only: Vec<MenuItem> = m.items.iter().map(|(i, _)| i.clone()).collect();
            match ev {
                Event::PointerMove { x, y } | Event::PointerEnter { x, y } => {
                    let sel = menu_hit(&only, x, y);
                    if sel.is_some() && sel != m.sel {
                        m.sel = sel;
                        ui.redraw(id);
                    }
                }
                Event::Button {
                    button: BTN_LEFT,
                    pressed: false,
                    x,
                    y,
                } => {
                    if let Some(i) = menu_hit(&only, x, y) {
                        let cmd = m.items[i].1;
                        self.run(ui, cmd);
                    }
                }
                Event::Key { sym, .. } => self.menu_key(ui, sym),
                Event::PopupDone => {
                    self.menu = None;
                    self.redraw(ui);
                }
                _ => {}
            }
            return;
        }
        if self.dialog.as_ref().is_some_and(|d| d.surface == id) {
            return self.dialog_event(ui, ev);
        }
        if Some(id) != self.win {
            return;
        }
        let modal = self.dialog.is_some();
        match ev {
            Event::Configure { w, h } => {
                self.size = (w, h);
                self.clamp_scroll();
            }
            Event::Close => ui.quit(),
            _ if modal => {}
            Event::Button {
                button: BTN_LEFT,
                pressed: true,
                x,
                y,
            } => self.press(ui, x, y),
            Event::Scroll { steps } => {
                self.scroll += (steps * ROW_H as f64 * 3.0).round() as i32;
                self.clamp_scroll();
                self.redraw(ui);
            }
            Event::Key {
                sym,
                text,
                ctrl,
                alt,
                shift,
            } => self.key(ui, sym, text, ctrl, alt, shift),
            _ => {}
        }
    }
}

fn main() {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/".into());
    let user = std::env::var("USER").unwrap_or_default();
    let mut dos = Dos::new(&home);
    dos.refresh_removable(&user);
    let start = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| home.clone());
    let mut cab = Cabinet {
        win: None,
        size: (560, 380),
        dos,
        user,
        home,
        panel: Panel::new(&start),
        view: View::List,
        scroll: 0,
        apps: retro_apps::desktop_apps(),
        bar: MenuBar::new(&["File", "Edit", "View", "Help"]),
        menu: None,
        dialog: None,
        clip: None,
        last_click: None,
    };
    cab.reload();
    if let Err(e) = retro_ui::run(Box::new(cab)) {
        eprintln!("file-cabinet: {e}");
        std::process::exit(1);
    }
}

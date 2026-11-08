//! Jot: the RetroOS text editor for the Desktop (docs/DESIGN.md §9.2), the
//! GUI sibling of EDIT, in the manner of Windows 3.1 Notepad.
//!
//!     jot [FILE]
//!
//! Text in Phosphor Mono (docs/STYLE.md §4), the same buffer code as EDIT
//! (tabs every 8 columns, CR LF files saved as CR LF). File: New, Open…,
//! Save, Save As…; Edit: Undo, Cut, Copy, Paste (the shared clipboard),
//! Delete, Select All; Search: Find…, Find Next. Unsaved changes are asked
//! about before New, Open and Exit.

mod editor;

use editor::{Editor, Move};
use retro_edit::buffer::Buffer;
use retro_gfx::font::{lantern_12, phosphor_mono};
use retro_gfx::widgets::{
    bevel, draw_menu, ellipsize, menu_hit, menu_layout, push_button, MenuItem,
};
use retro_gfx::{Canvas, Surface, Theme};
use retro_ui::controls::{check_box, ListBox, MenuBar, TextField, FIELD_H, MENU_BAR_H};
use retro_ui::keys::*;
use retro_ui::{Event, Program, Rect, SurfaceId, Ui};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const BTN_LEFT: u32 = 0x110;
const SB: i32 = 16;
const CW: i32 = 8;
const CH: i32 = 16;
const MARGIN: i32 = 2;
const BLINK: u32 = 1;
const DOUBLE_CLICK: Duration = Duration::from_millis(450);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Cmd {
    None,
    New,
    Open,
    Save,
    SaveAs,
    Exit,
    Undo,
    Cut,
    Copy,
    Paste,
    Delete,
    SelectAll,
    Find,
    FindNext,
    About,
}

/// What to do once unsaved changes are dealt with.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Then {
    Nothing,
    New,
    Open,
    Exit,
}

struct Menu {
    title: usize,
    surface: SurfaceId,
    items: Vec<(MenuItem, Cmd)>,
    sel: Option<usize>,
}

#[derive(Clone, Copy, PartialEq)]
enum Drag {
    Text,
    VThumb(i32),
    HThumb(i32),
}

// ------------------------------------------------------------- dialogs

struct Button {
    rect: Rect,
    label: &'static str,
}

struct FileDlg {
    save: bool,
    dir: PathBuf,
    name: TextField,
    list: ListBox,
    /// The entries behind `list`: (name, is a directory).
    entries: Vec<(String, bool)>,
    focus_list: bool,
    then: Then,
    last_click: Option<(usize, Instant)>,
}

struct FindDlg {
    what: TextField,
    case: bool,
}

enum Kind {
    File(FileDlg),
    Find(FindDlg),
    /// Save changes? (Yes / No / Cancel)
    Ask(Then),
    Message(Vec<String>),
}

struct Dialog {
    surface: SurfaceId,
    kind: Kind,
    buttons: Vec<Button>,
    pressed: Option<usize>,
}

const FILE_W: i32 = 360;
const FILE_H: i32 = 250;
const FILE_LIST: Rect = Rect {
    x: 8,
    y: 92,
    w: 214,
    h: 148,
};

fn buttons(labels: &[&'static str], x: i32, y: i32, w: i32, vertical: bool) -> Vec<Button> {
    labels
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let i = i as i32;
            let (bx, by) = if vertical {
                (x, y + i * 30)
            } else {
                (x + i * (w + 8), y)
            };
            Button {
                rect: Rect::new(bx, by, w, 22),
                label: l,
            }
        })
        .collect()
}

/// Directory entries for the file dialog: "..", then folders, then files,
/// by name; hidden ones (dot files) left out.
fn list_dir(dir: &Path) -> Vec<(String, bool)> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            if e.path().is_dir() {
                dirs.push(name);
            } else {
                files.push(name);
            }
        }
    }
    dirs.sort_by_key(|n| n.to_lowercase());
    files.sort_by_key(|n| n.to_lowercase());
    let mut v = Vec::new();
    if dir.parent().is_some() {
        v.push(("..".to_string(), true));
    }
    v.extend(dirs.into_iter().map(|d| (d, true)));
    v.extend(files.into_iter().map(|f| (f, false)));
    v
}

impl FileDlg {
    fn new(save: bool, dir: PathBuf, name: &str, then: Then) -> FileDlg {
        let mut d = FileDlg {
            save,
            dir,
            name: TextField::new(name),
            list: ListBox::default(),
            entries: Vec::new(),
            focus_list: false,
            then,
            last_click: None,
        };
        d.chdir(None);
        d
    }

    fn chdir(&mut self, to: Option<&str>) {
        match to {
            Some("..") => {
                if let Some(p) = self.dir.parent() {
                    self.dir = p.to_path_buf();
                }
            }
            Some(d) => self.dir = self.dir.join(d),
            None => {}
        }
        self.entries = list_dir(&self.dir);
        let items = self
            .entries
            .iter()
            .map(|(n, d)| if *d { format!("[{n}]") } else { n.clone() })
            .collect();
        self.list = ListBox::new(items);
        self.list.sel = None;
    }
}

// ------------------------------------------------------------------ Jot

struct Jot {
    win: Option<SurfaceId>,
    size: (i32, i32),
    ed: Editor,
    bar: MenuBar,
    menu: Option<Menu>,
    dialog: Option<Dialog>,
    caret_on: bool,
    drag: Option<Drag>,
    last_find: Option<(String, bool)>,
}

fn title_of(b: &Buffer) -> String {
    let name = b
        .path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".into());
    format!("{name} - Jot")
}

/// Text area, vertical and horizontal scroll bar of a window of `size`.
fn areas(size: (i32, i32)) -> (Rect, Rect, Rect) {
    let (w, h) = size;
    let text = Rect::new(0, MENU_BAR_H, (w - SB).max(0), (h - MENU_BAR_H - SB).max(0));
    let v = Rect::new(w - SB, MENU_BAR_H, SB, (h - MENU_BAR_H - SB).max(0));
    let hz = Rect::new(0, h - SB, (w - SB).max(0), SB);
    (text, v, hz)
}

/// A scroll bar's thumb: its offset along the bar from `pos` of `max`.
fn thumb(len: i32, pos: usize, max: usize) -> i32 {
    let track = (len - 3 * SB).max(0);
    if max == 0 {
        return SB;
    }
    SB + (track as i64 * pos.min(max) as i64 / max as i64) as i32
}

impl Jot {
    fn redraw(&self, ui: &mut Ui) {
        if let Some(w) = self.win {
            ui.redraw(w);
        }
    }

    fn fit_view(&mut self) {
        let (text, _, _) = areas(self.size);
        self.ed.rows = ((text.h - 2 * MARGIN) / CH).max(1) as usize;
        self.ed.cols = ((text.w - 2 * MARGIN) / CW).max(1) as usize;
    }

    fn max_top(&self) -> usize {
        self.ed.buf.lines.len().saturating_sub(self.ed.rows)
    }

    fn max_left(&self) -> usize {
        self.ed.longest().saturating_sub(self.ed.cols) + 1
    }

    fn set_title(&self, ui: &mut Ui) {
        if let Some(w) = self.win {
            ui.set_title(w, &title_of(&self.ed.buf));
        }
    }

    // ------------------------------------------------------- commands

    fn menu_items(&self, title: usize) -> Vec<(MenuItem, Cmd)> {
        let sep = || (MenuItem::separator(), Cmd::None);
        let sel = self.ed.selection().is_some();
        let it = |l: &str, s: &str, on: bool, c: Cmd| (MenuItem::new(l, s, on), c);
        match title {
            0 => vec![
                it("&New", "Ctrl+N", true, Cmd::New),
                it("&Open...", "Ctrl+O", true, Cmd::Open),
                it("&Save", "Ctrl+S", true, Cmd::Save),
                it("Save &As...", "", true, Cmd::SaveAs),
                sep(),
                it("E&xit", "Alt+F4", true, Cmd::Exit),
            ],
            1 => vec![
                it("&Undo", "Ctrl+Z", self.ed.can_undo(), Cmd::Undo),
                sep(),
                it("Cu&t", "Ctrl+X", sel, Cmd::Cut),
                it("&Copy", "Ctrl+C", sel, Cmd::Copy),
                it("&Paste", "Ctrl+V", true, Cmd::Paste),
                it("De&lete", "Del", sel, Cmd::Delete),
                sep(),
                it("Select &All", "Ctrl+A", true, Cmd::SelectAll),
            ],
            2 => vec![
                it("&Find...", "Ctrl+F", true, Cmd::Find),
                it("Find &Next", "F3", self.last_find.is_some(), Cmd::FindNext),
            ],
            _ => vec![it("&About Jot...", "", true, Cmd::About)],
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
            Cmd::New => self.confirm(ui, Then::New),
            Cmd::Open => self.confirm(ui, Then::Open),
            Cmd::Exit => self.confirm(ui, Then::Exit),
            Cmd::Save => self.save(ui, Then::Nothing),
            Cmd::SaveAs => self.file_dialog(ui, true, Then::Nothing),
            Cmd::Undo => self.ed.undo(),
            Cmd::Cut => {
                if let Some(t) = self.ed.cut() {
                    ui.set_clipboard(&t);
                }
            }
            Cmd::Copy => {
                if let Some(t) = self.ed.selected_text() {
                    ui.set_clipboard(&t);
                }
            }
            Cmd::Paste => {
                if let Some(t) = ui.clipboard() {
                    self.ed.insert(&t);
                }
            }
            Cmd::Delete => {
                if self.ed.selection().is_some() {
                    self.ed.delete();
                }
            }
            Cmd::SelectAll => self.ed.select_all(),
            Cmd::Find => self.find_dialog(ui),
            Cmd::FindNext => match self.last_find.clone() {
                Some((what, case)) => self.find_next(ui, &what, case),
                None => self.find_dialog(ui),
            },
            Cmd::About => self.message(
                ui,
                "About Jot",
                &["Jot", "Version 1.0", "The RetroOS text editor."],
            ),
        }
        self.caret_on = true;
        self.redraw(ui);
    }

    /// Do `then`, asking first about unsaved changes.
    fn confirm(&mut self, ui: &mut Ui, then: Then) {
        if self.ed.buf.modified {
            self.open_dialog(
                ui,
                "Jot",
                (340, 104),
                Kind::Ask(then),
                &["Yes", "No", "Cancel"],
            );
        } else {
            self.proceed(ui, then);
        }
    }

    fn proceed(&mut self, ui: &mut Ui, then: Then) {
        match then {
            Then::Nothing => {}
            Then::New => {
                self.ed = Editor::new(Buffer::default());
                self.fit_view();
                self.set_title(ui);
            }
            Then::Open => self.file_dialog(ui, false, Then::Nothing),
            Then::Exit => ui.quit(),
        }
        self.redraw(ui);
    }

    fn save(&mut self, ui: &mut Ui, then: Then) {
        if self.ed.buf.path.is_none() {
            return self.file_dialog(ui, true, then);
        }
        match self.ed.buf.save() {
            Ok(()) => self.proceed(ui, then),
            Err(e) => {
                let name = title_of(&self.ed.buf).replace(" - Jot", "");
                self.message(
                    ui,
                    "Jot",
                    &[&format!("Cannot save {name}:"), &e.to_string()],
                );
            }
        }
    }

    fn load(&mut self, ui: &mut Ui, path: &Path) {
        match Buffer::open(path) {
            Ok(b) => {
                self.ed = Editor::new(b);
                self.fit_view();
                self.set_title(ui);
            }
            Err(e) => {
                let name = path.display().to_string();
                self.message(
                    ui,
                    "Jot",
                    &[&format!("Cannot open {name}:"), &e.to_string()],
                );
            }
        }
    }

    fn find_next(&mut self, ui: &mut Ui, what: &str, case: bool) {
        self.last_find = Some((what.to_string(), case));
        if !self.ed.find(what, case) {
            self.message(ui, "Jot", &[&format!("Cannot find \"{what}\"")]);
        }
        self.redraw(ui);
    }

    // -------------------------------------------------------- dialogs

    fn open_dialog(
        &mut self,
        ui: &mut Ui,
        title: &str,
        size: (i32, i32),
        kind: Kind,
        labels: &[&'static str],
    ) {
        self.close_dialog(ui);
        let Some(win) = self.win else { return };
        let btns = match &kind {
            Kind::File(_) => buttons(labels, 236, 32, 108, true),
            Kind::Find(_) => buttons(labels, size.0 - 96, 10, 86, true),
            _ => {
                let n = labels.len() as i32;
                let bw = 76;
                let x = size.0 - n * (bw + 8) - 4;
                buttons(labels, x, size.1 - 32, bw, false)
            }
        };
        let surface = ui.dialog(win, title, size);
        self.dialog = Some(Dialog {
            surface,
            kind,
            buttons: btns,
            pressed: None,
        });
    }

    fn close_dialog(&mut self, ui: &mut Ui) {
        if let Some(d) = self.dialog.take() {
            ui.close(d.surface);
        }
    }

    fn message(&mut self, ui: &mut Ui, title: &str, lines: &[&str]) {
        let w = lines
            .iter()
            .map(|l| retro_gfx::widgets::text_width(l))
            .max()
            .unwrap_or(0)
            .max(180)
            + 40;
        let h = 50 + lines.len() as i32 * 16;
        let lines = lines.iter().map(|l| l.to_string()).collect();
        self.open_dialog(ui, title, (w, h), Kind::Message(lines), &["OK"]);
    }

    fn file_dialog(&mut self, ui: &mut Ui, save: bool, then: Then) {
        let (dir, name) = match &self.ed.buf.path {
            Some(p) => (
                p.parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| "/".into()),
                p.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            ),
            None => (
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| "/".into()),
                if save {
                    "untitled.txt".into()
                } else {
                    String::new()
                },
            ),
        };
        let title = if save { "Save As" } else { "Open" };
        let dlg = FileDlg::new(save, dir, &name, then);
        self.open_dialog(
            ui,
            title,
            (FILE_W, FILE_H),
            Kind::File(dlg),
            &["OK", "Cancel"],
        );
    }

    fn find_dialog(&mut self, ui: &mut Ui) {
        let (what, case) = self.last_find.clone().unwrap_or_default();
        let what = self
            .ed
            .selected_text()
            .filter(|s| !s.contains('\n'))
            .unwrap_or(what);
        let dlg = FindDlg {
            what: TextField::new(&what),
            case,
        };
        self.open_dialog(
            ui,
            "Find",
            (330, 74),
            Kind::Find(dlg),
            &["Find Next", "Cancel"],
        );
    }

    /// A dialog button was chosen (or Enter / Esc / a letter).
    fn answer(&mut self, ui: &mut Ui, button: usize) {
        let Some(d) = self.dialog.as_mut() else {
            return;
        };
        match &mut d.kind {
            Kind::Message(_) => self.close_dialog(ui),
            Kind::Ask(then) => {
                let then = *then;
                self.close_dialog(ui);
                match button {
                    0 => self.save(ui, then),
                    1 => self.proceed(ui, then),
                    _ => {}
                }
            }
            Kind::Find(f) => {
                if button == 0 {
                    let (what, case) = (f.what.value(), f.case);
                    if !what.is_empty() {
                        self.find_next(ui, &what, case);
                    }
                } else {
                    self.close_dialog(ui);
                }
            }
            Kind::File(f) => {
                if button != 0 {
                    return self.close_dialog(ui);
                }
                // A selected folder in the list opens; otherwise the name.
                if f.focus_list {
                    if let Some((n, true)) = f.list.sel.and_then(|i| f.entries.get(i)).cloned() {
                        f.chdir(Some(&n));
                        return ui.redraw(d.surface);
                    }
                }
                let name = f.name.value();
                if name.is_empty() {
                    return;
                }
                let target = if name.starts_with('/') {
                    PathBuf::from(&name)
                } else if let Some(rest) = name.strip_prefix("~/") {
                    std::env::var_os("HOME")
                        .map(PathBuf::from)
                        .unwrap_or_default()
                        .join(rest)
                } else {
                    f.dir.join(&name)
                };
                if target.is_dir() {
                    f.dir = target;
                    f.chdir(None);
                    f.name.set("");
                    return ui.redraw(d.surface);
                }
                let (save, then) = (f.save, f.then);
                self.close_dialog(ui);
                if save {
                    self.ed.buf.path = Some(target);
                    self.set_title(ui);
                    self.save(ui, then);
                } else {
                    self.load(ui, &target);
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
            } => {
                let t = text.as_deref();
                match sym {
                    ESCAPE => return self.close_dialog(ui),
                    RETURN | KP_ENTER => return self.answer(ui, 0),
                    _ => {}
                }
                match &mut d.kind {
                    Kind::Ask(_) => {
                        let c = t
                            .and_then(|t| t.chars().next())
                            .map(|c| c.to_ascii_lowercase());
                        match c {
                            Some('y') => return self.answer(ui, 0),
                            Some('n') => return self.answer(ui, 1),
                            _ => {}
                        }
                    }
                    Kind::Find(f) => {
                        // Tab ticks Match case (the only other control).
                        if sym == TAB {
                            f.case = !f.case;
                        } else {
                            f.what.key(sym, t, ctrl, shift);
                        }
                    }
                    Kind::File(f) => {
                        if sym == TAB || sym == ISO_LEFT_TAB {
                            f.focus_list = !f.focus_list;
                            if f.focus_list && f.list.sel.is_none() && !f.entries.is_empty() {
                                f.list.sel = Some(0);
                            }
                        } else if f.focus_list {
                            f.list.key(sym, FILE_LIST.h);
                            if let Some((n, false)) = f.list.sel.and_then(|i| f.entries.get(i)) {
                                f.name.set(n);
                            }
                        } else {
                            f.name.key(sym, t, ctrl, shift);
                        }
                    }
                    Kind::Message(_) => {}
                }
                ui.redraw(id);
            }
            Event::Scroll { steps } => {
                if let Kind::File(f) = &mut d.kind {
                    f.list.scroll((steps * 3.0).round() as isize, FILE_LIST.h);
                    ui.redraw(id);
                }
            }
            Event::Button {
                button: BTN_LEFT,
                pressed: true,
                x,
                y,
            } => {
                d.pressed = d.buttons.iter().position(|b| b.rect.contains(x, y));
                match &mut d.kind {
                    Kind::Find(f) => {
                        if Rect::new(10, 42, 140, 14).contains(x, y) {
                            f.case = !f.case;
                        } else if Rect::new(80, 10, 140, FIELD_H).contains(x, y) {
                            f.what.click(x - 80);
                        }
                    }
                    Kind::File(f) => {
                        if Rect::new(8, 30, 214, FIELD_H).contains(x, y) {
                            f.focus_list = false;
                            f.name.click(x - 8);
                        } else if FILE_LIST.contains(x, y) {
                            f.focus_list = true;
                            if let Some(i) = f.list.click(y - FILE_LIST.y) {
                                let now = Instant::now();
                                let double = f.last_click.is_some_and(|(li, t)| {
                                    li == i && now.duration_since(t) < DOUBLE_CLICK
                                });
                                f.last_click = Some((i, now));
                                let (n, is_dir) = f.entries[i].clone();
                                if !is_dir {
                                    f.name.set(&n);
                                }
                                if double {
                                    f.last_click = None;
                                    if is_dir {
                                        f.chdir(Some(&n));
                                    } else {
                                        return self.answer(ui, 0);
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
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
                if let Some(b) = p.filter(|&b| d.buttons[b].rect.contains(x, y)) {
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
            Kind::Ask(_) => {
                let name = title_of(&self.ed.buf).replace(" - Jot", "");
                sf.text(
                    f,
                    20,
                    14,
                    1,
                    t.text,
                    None,
                    &format!("The text in the {name} file has changed."),
                );
                sf.text(
                    f,
                    20,
                    32,
                    1,
                    t.text,
                    None,
                    "Do you want to save the changes?",
                );
            }
            Kind::Find(fd) => {
                sf.text(f, 10, 13, 1, t.text, None, "Find what:");
                fd.what.draw(&mut sf, 80, 10, 140, true, t);
                check_box(&mut sf, 10, 42, "Match case", fd.case, t);
            }
            Kind::File(fd) => {
                sf.text(f, 8, 12, 1, t.text, None, "File name:");
                fd.name.draw(&mut sf, 8, 30, 214, !fd.focus_list, t);
                let dir = ellipsize(&fd.dir.display().to_string(), 214);
                sf.text(f, 8, 58, 1, t.text, None, "Directory:");
                sf.text(f, 8, 74, 1, t.text, None, &dir);
                fd.list.draw(&mut sf, FILE_LIST, fd.focus_list, t);
            }
        }
        for (i, b) in d.buttons.iter().enumerate() {
            let r = b.rect;
            push_button(
                &mut sf,
                r.x,
                r.y,
                r.w,
                r.h,
                1,
                b.label,
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
        if alt || sym == F10 {
            let c = text.as_deref().and_then(|t| t.chars().next());
            let t = c.and_then(|c| self.bar.by_letter(c));
            match t {
                Some(t) => self.open_menu(ui, t),
                None if sym == F10 => self.open_menu(ui, 0),
                None => {}
            }
            return;
        }
        let letter = char::from_u32(sym).map(|c| c.to_ascii_lowercase());
        let e = &mut self.ed;
        let mv = |e: &mut Editor, m| e.go(m, shift);
        match sym {
            LEFT if ctrl => mv(e, Move::WordLeft),
            RIGHT if ctrl => mv(e, Move::WordRight),
            HOME if ctrl => mv(e, Move::DocStart),
            END if ctrl => mv(e, Move::DocEnd),
            LEFT => mv(e, Move::Left),
            RIGHT => mv(e, Move::Right),
            UP => mv(e, Move::Up),
            DOWN => mv(e, Move::Down),
            HOME => mv(e, Move::Home),
            END => mv(e, Move::End),
            PAGE_UP => mv(e, Move::PageUp),
            PAGE_DOWN => mv(e, Move::PageDown),
            BACKSPACE => e.backspace(),
            DELETE if shift => return self.run(ui, Cmd::Cut),
            DELETE => e.delete(),
            INSERT if ctrl => return self.run(ui, Cmd::Copy),
            INSERT if shift => return self.run(ui, Cmd::Paste),
            RETURN | KP_ENTER => e.insert("\n"),
            TAB => e.insert("\t"),
            F3 => return self.run(ui, Cmd::FindNext),
            _ if ctrl => {
                let cmd = match letter {
                    Some('z') => Cmd::Undo,
                    Some('x') => Cmd::Cut,
                    Some('c') => Cmd::Copy,
                    Some('v') => Cmd::Paste,
                    Some('a') => Cmd::SelectAll,
                    Some('s') => Cmd::Save,
                    Some('o') => Cmd::Open,
                    Some('n') => Cmd::New,
                    Some('f') => Cmd::Find,
                    _ => return,
                };
                return self.run(ui, cmd);
            }
            _ => match text {
                Some(t) if !t.is_empty() && !t.chars().any(char::is_control) => e.insert(&t),
                _ => return,
            },
        }
        self.caret_on = true;
        self.redraw(ui);
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

    /// The text cell under window point (x, y), clamped into the view.
    fn cell(&self, x: i32, y: i32) -> (usize, usize) {
        let (text, _, _) = areas(self.size);
        let row = ((y - text.y - MARGIN).max(0) / CH) as usize;
        let col = ((x - text.x - MARGIN + CW / 2).max(0) / CW) as usize;
        (row.min(self.ed.rows.saturating_sub(1)), col)
    }

    fn press(&mut self, ui: &mut Ui, x: i32, y: i32, shift: bool) {
        if y < MENU_BAR_H {
            match self.bar.hit(x, y) {
                Some(t) if self.menu.as_ref().map(|m| m.title) == Some(t) => self.close_menu(ui),
                Some(t) => self.open_menu(ui, t),
                None => self.close_menu(ui),
            }
            return;
        }
        let (text, v, hz) = areas(self.size);
        if v.contains(x, y) {
            let off = y - v.y;
            let th = thumb(v.h, self.ed.top, self.max_top());
            let page = self.ed.rows.max(2) as isize - 1;
            if off < SB {
                self.ed.scroll_lines(-1);
            } else if off >= v.h - SB {
                self.ed.scroll_lines(1);
            } else if off < th {
                self.ed.scroll_lines(-page);
            } else if off >= th + SB {
                self.ed.scroll_lines(page);
            } else {
                self.drag = Some(Drag::VThumb(off - th));
            }
            self.ed.top = self.ed.top.min(self.max_top());
        } else if hz.contains(x, y) {
            let off = x - hz.x;
            let th = thumb(hz.w, self.ed.left, self.max_left());
            let page = self.ed.cols.max(2) as isize - 1;
            if off < SB {
                self.ed.scroll_cols(-4);
            } else if off >= hz.w - SB {
                self.ed.scroll_cols(4);
            } else if off < th {
                self.ed.scroll_cols(-page);
            } else if off >= th + SB {
                self.ed.scroll_cols(page);
            } else {
                self.drag = Some(Drag::HThumb(off - th));
            }
        } else if text.contains(x, y) {
            let (row, col) = self.cell(x, y);
            self.ed.click(row, col, shift);
            self.drag = Some(Drag::Text);
            self.caret_on = true;
        }
        self.redraw(ui);
    }

    fn motion(&mut self, ui: &mut Ui, x: i32, y: i32) {
        let (text, v, hz) = areas(self.size);
        match self.drag {
            None => return,
            Some(Drag::Text) => {
                // Above or below the text: scroll a line at a time.
                if y < text.y {
                    self.ed.scroll_lines(-1);
                } else if y >= text.y + text.h {
                    self.ed.scroll_lines(1);
                }
                let (row, col) = self.cell(x, y);
                self.ed.click(row, col, true);
            }
            Some(Drag::VThumb(grab)) => {
                let track = (v.h - 3 * SB).max(1);
                let pos = (y - v.y - SB - grab).clamp(0, track);
                self.ed.top = (pos as i64 * self.max_top() as i64 / track as i64) as usize;
            }
            Some(Drag::HThumb(grab)) => {
                let track = (hz.w - 3 * SB).max(1);
                let pos = (x - hz.x - SB - grab).clamp(0, track);
                self.ed.left = (pos as i64 * self.max_left() as i64 / track as i64) as usize;
            }
        }
        self.redraw(ui);
    }

    fn draw_window(&self, c: &mut Canvas, t: &Theme) {
        let (w, _) = (c.width as i32, c.height as i32);
        let mut sf = c.surface();
        let (text, v, hz) = areas(self.size);
        sf.fill_rect(text.x, text.y, text.w as u32, text.h as u32, t.well);
        self.draw_text(&mut sf, text, t);
        let open = self.menu.as_ref().map(|m| m.title);
        self.bar.draw(&mut sf, w, open, t);
        scroll_bar(&mut sf, v, true, thumb(v.h, self.ed.top, self.max_top()), t);
        scroll_bar(
            &mut sf,
            hz,
            false,
            thumb(hz.w, self.ed.left, self.max_left()),
            t,
        );
        sf.fill_rect(v.x, hz.y, SB as u32, SB as u32, t.face);
    }

    fn draw_text(&self, sf: &mut Surface, area: Rect, t: &Theme) {
        let font = phosphor_mono();
        let e = &self.ed;
        let sel = e.selection();
        let in_sel = |line: usize, col: usize| {
            sel.is_some_and(|(a, b)| {
                let p = retro_edit::buffer::Pos::new(line, col);
                p >= a && p < b
            })
        };
        let x0 = area.x + MARGIN;
        let cols = e.cols;
        for row in 0..e.rows {
            let line = e.top + row;
            let Some(chars) = e.buf.lines.get(line) else {
                break;
            };
            let y = area.y + MARGIN + row as i32 * CH;
            let mut d = 0usize;
            for (i, ch) in chars.iter().enumerate() {
                let w = if *ch == '\t' { (d / 8 + 1) * 8 - d } else { 1 };
                for k in 0..w {
                    let dc = d + k;
                    if dc < e.left || dc >= e.left + cols {
                        continue;
                    }
                    let x = x0 + (dc - e.left) as i32 * CW;
                    let on = in_sel(line, i);
                    if on {
                        sf.fill_rect(x, y, CW as u32, CH as u32, t.sel);
                    }
                    if k == 0 && *ch != '\t' {
                        let g = font.glyph_or_replacement(*ch);
                        sf.glyph(g, x, y, 1, if on { t.sel_text } else { t.text }, None);
                    }
                }
                d += w;
            }
            // A selected line end shows as one selected cell.
            if in_sel(line, chars.len())
                && line + 1 < e.buf.lines.len()
                && d >= e.left
                && d < e.left + cols
            {
                let x = x0 + (d - e.left) as i32 * CW;
                sf.fill_rect(x, y, CW as u32, CH as u32, t.sel);
            }
        }
        if self.caret_on
            && self
                .dialog
                .as_ref()
                .is_none_or(|d| matches!(d.kind, Kind::Find(_)))
        {
            let d = e.buf.display_col(e.cur.line, e.cur.col);
            if e.cur.line >= e.top
                && e.cur.line < e.top + e.rows
                && d >= e.left
                && d <= e.left + cols
            {
                let x = x0 + (d - e.left) as i32 * CW;
                let y = area.y + MARGIN + (e.cur.line - e.top) as i32 * CH;
                sf.fill_rect(x.max(area.x), y, 1, CH as u32, t.text);
            }
        }
    }
}

/// A Windows 3.1 scroll bar: arrow buttons at both ends, a dithered track,
/// a square thumb at `thumb` pixels along it.
fn scroll_bar(sf: &mut Surface, r: Rect, vertical: bool, thumb: i32, t: &Theme) {
    for y in r.y..r.y + r.h {
        for x in r.x..r.x + r.w {
            sf.put(x, y, if (x + y) % 2 == 0 { t.face } else { t.light });
        }
    }
    sf.frame(r.x, r.y, r.w as u32, r.h as u32, 1, t.line);
    let len = if vertical { r.h } else { r.w };
    let at = |o: i32| {
        if vertical {
            (r.x, r.y + o)
        } else {
            (r.x + o, r.y)
        }
    };
    for (o, dir) in [(0, -1), (len - SB, 1)] {
        let (x, y) = at(o);
        bevel(sf, x, y, SB, SB, 1, false, t);
        sf.frame(x, y, SB as u32, SB as u32, 1, t.line);
        // A small solid triangle pointing along the bar.
        for i in 0..4 {
            let span = 1 + 2 * i;
            let k = if dir < 0 { i } else { 3 - i };
            if vertical {
                sf.fill_rect(x + 8 - i - 1 + 1, y + 6 + k, span as u32, 1, t.text);
            } else {
                sf.fill_rect(x + 6 + k, y + 8 - i - 1 + 1, 1, span as u32, t.text);
            }
        }
    }
    if len >= 3 * SB {
        let (x, y) = at(thumb);
        bevel(sf, x, y, SB, SB, 1, false, t);
        sf.frame(x, y, SB as u32, SB as u32, 1, t.line);
    }
}

impl Program for Jot {
    fn start(&mut self, ui: &mut Ui) {
        let title = title_of(&self.ed.buf);
        self.win = Some(ui.window(&title, "retroos.jot", self.size, false));
        self.fit_view();
        ui.timer(BLINK, Duration::from_millis(530));
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
        if std::env::var_os("RETRO_JOT_DEBUG").is_some() {
            eprintln!("jot: {id:?} {ev:?}");
        }
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
        // A dialog other than Find is modal.
        let modal = self
            .dialog
            .as_ref()
            .is_some_and(|d| !matches!(d.kind, Kind::Find(_)));
        match ev {
            Event::Configure { w, h } => {
                self.size = (w, h);
                self.fit_view();
                self.ed.reveal();
            }
            Event::Close if !modal => self.confirm(ui, Then::Exit),
            _ if modal => {}
            Event::Button {
                button: BTN_LEFT,
                pressed: true,
                x,
                y,
            } => {
                let shift = ui.shift();
                self.press(ui, x, y, shift);
            }
            Event::Button {
                button: BTN_LEFT,
                pressed: false,
                ..
            } => self.drag = None,
            Event::PointerMove { x, y } => self.motion(ui, x, y),
            Event::Scroll { steps } => {
                self.ed.scroll_lines((steps * 3.0).round() as isize);
                self.ed.top = self.ed.top.min(self.max_top());
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

    fn timer(&mut self, ui: &mut Ui, token: u32) {
        if token == BLINK {
            self.caret_on = !self.caret_on;
            self.redraw(ui);
            ui.timer(BLINK, Duration::from_millis(530));
        }
    }
}

fn main() {
    let buf = match std::env::args_os().nth(1) {
        Some(p) => Buffer::open(Path::new(&p)).unwrap_or_else(|e| {
            eprintln!("jot: {}: {e}", Path::new(&p).display());
            Buffer::default()
        }),
        None => Buffer::default(),
    };
    let jot = Jot {
        win: None,
        size: (560, 380),
        ed: Editor::new(buf),
        bar: MenuBar::new(&["File", "Edit", "Search", "Help"]),
        menu: None,
        dialog: None,
        caret_on: true,
        drag: None,
        last_find: None,
    };
    if let Err(e) = retro_ui::run(Box::new(jot)) {
        eprintln!("jot: {e}");
        std::process::exit(1);
    }
}

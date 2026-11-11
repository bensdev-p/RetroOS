//! Software Library — the Desktop's installer (DESIGN.md §9.2, §10.5,
//! D-055). Categories on the left (then Installed, Updates, Search), the
//! chosen category's programs on the right with their icons (the
//! category's picture until a program is installed, then its own,
//! retro-fied), the chosen program's description and buttons underneath.
//! The same catalog and operations as LIBRARY (retro_sys::catalog,
//! retro_sys::software); apt runs on a thread and a progress bar shows here.
//!
//!   software-library [category | updates]

use retro_gfx::font::lantern_12;
use retro_gfx::iconset::{self, Icon};
use retro_gfx::widgets::{ellipsize_in, push_button, text_width};
use retro_gfx::{Canvas, Theme};
use retro_sys::admin::AdminError;
use retro_sys::catalog::{catalog, App as Entry};
use retro_sys::software::{self, Job, Op, Update};
use retro_ui::controls::{TextField, FIELD_H};
use retro_ui::keys::*;
use retro_ui::{station, Event, Program, Rect, SurfaceId, Ui};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

const TITLE: &str = "Software Library";
const W: i32 = 640;
const H: i32 = 440;
const LEFT_BUTTON: u32 = 0x110;
const TIMER_JOB: u32 = 1;

const INSTALLED: &str = "Installed";
const UPDATES: &str = "Updates";
const SEARCH: &str = "Search";

const CATS: Rect = Rect {
    x: 8,
    y: 36,
    w: 150,
    h: H - 36 - 118,
};
const CAT_ROW: i32 = 22;
const FIND: Rect = Rect {
    x: 166,
    y: 8,
    w: W - 166 - 8 - 80,
    h: FIELD_H,
};
const FIND_BTN: Rect = Rect {
    x: W - 8 - 72,
    y: 7,
    w: 72,
    h: 22,
};
const LIST: Rect = Rect {
    x: 166,
    y: 36,
    w: W - 166 - 8,
    h: H - 36 - 118,
};
const ROW: i32 = 40;
const DETAIL_Y: i32 = H - 112;
const BTN_W: i32 = 96;
const BTN_H: i32 = 24;

#[derive(Clone, Debug)]
enum Row {
    App(Entry),
    Package(String, String),
    Update(Update),
}

/// The password box.
struct Ask {
    surface: SurfaceId,
    field: TextField,
    op: Op,
    done: Vec<String>,
    wrong: bool,
    pressed: Option<usize>,
}

const ASK_W: i32 = 320;
const ASK_H: i32 = 120;

struct Message {
    surface: SurfaceId,
    lines: Vec<String>,
    pressed: bool,
}

struct Lib {
    win: Option<SurfaceId>,
    cats: Vec<&'static str>,
    cat: usize,
    rows: Vec<Row>,
    sel: usize,
    top: usize,
    installed: HashSet<String>,
    find: TextField,
    /// Keyboard focus: 0 categories, 1 programs, 2 the search field.
    focus: usize,
    pressed: Option<usize>,
    job: Option<(Job, Vec<String>)>,
    /// A job another window (or INSTALL, or the daily check) started: shown
    /// here, as GNOME Software shows PackageKit's transactions.
    other: Option<software::Running>,
    /// Asked for while `other` ran: starts when it has finished (a queue,
    /// as PackageKit's).
    queued: Option<(Op, Vec<String>)>,
    ask: Option<Ask>,
    message: Option<Message>,
    icons: HashMap<String, Option<Icon>>,
    start: Option<String>,
    /// `--install PKG...` (Arcade's Install Emulator): install these, say
    /// so, and close.
    direct: Option<Vec<String>>,
}

fn button(i: usize) -> Rect {
    Rect::new(
        W - 8 - (3 - i as i32) * (BTN_W + 8) + 8,
        H - BTN_H - 10,
        BTN_W,
        BTN_H,
    )
}

impl Lib {
    /// Installing or removing (here or elsewhere), or asking for the
    /// password: the buttons are greyed and do nothing.
    fn busy(&self) -> bool {
        self.job.is_some() || self.other.is_some() || self.ask.is_some()
    }

    fn is_installed(&self, e: &Entry) -> bool {
        self.installed.contains(&e.packages[0])
    }

    fn load(&mut self) {
        let c = catalog();
        let q = self.find.value();
        self.rows = match self.cats[self.cat] {
            INSTALLED => c
                .apps
                .iter()
                .filter(|a| self.is_installed(a))
                .cloned()
                .map(Row::App)
                .collect(),
            UPDATES => software::updates().into_iter().map(Row::Update).collect(),
            SEARCH => {
                let mut v: Vec<Row> = if q.trim().is_empty() {
                    Vec::new()
                } else {
                    c.search(&q).into_iter().cloned().map(Row::App).collect()
                };
                if !q.trim().is_empty() {
                    let known: HashSet<&String> = c.apps.iter().map(|a| &a.packages[0]).collect();
                    v.extend(
                        software::search_all(q.trim())
                            .into_iter()
                            .filter(|(p, _)| !known.contains(p))
                            .map(|(p, s)| Row::Package(p, s)),
                    );
                }
                v
            }
            name => c
                .in_category(name)
                .into_iter()
                .cloned()
                .map(Row::App)
                .collect(),
        };
        self.sel = self.sel.min(self.rows.len().saturating_sub(1));
        self.reveal();
    }

    fn rows_shown() -> usize {
        ((LIST.h - 4) / ROW) as usize
    }

    fn reveal(&mut self) {
        let n = Lib::rows_shown();
        if self.sel < self.top {
            self.top = self.sel;
        } else if self.sel >= self.top + n {
            self.top = self.sel + 1 - n;
        }
        self.top = self.top.min(self.rows.len().saturating_sub(n));
    }

    /// The picture for a catalog program: its own (retro-fied) once
    /// installed, else its category's.
    fn icon_for(&mut self, e: &Entry) -> Option<Icon> {
        if self.is_installed(e) {
            let key = e.packages[0].clone();
            let own = self
                .icons
                .entry(key)
                .or_insert_with(|| {
                    retro_apps::find_app(&e.run)
                        .and_then(|d| d.icon)
                        .and_then(|i| iconset::for_app(&i))
                })
                .clone();
            if own.is_some() {
                return own;
            }
        }
        let cat = catalog().categories.iter().find(|c| c.name == e.category)?;
        iconset::builtin(cat.icon).cloned()
    }

    fn draw_window(&mut self, c: &mut Canvas, t: &Theme) {
        let mut sf = c.surface();
        sf.clear(t.face);
        let font = lantern_12();
        // Search.
        sf.text(font, CATS.x, FIND.y + 3, 1, t.text, None, "Find:");
        self.find
            .draw(&mut sf, FIND.x, FIND.y, FIND.w, self.focus == 2, t);
        push_button(
            &mut sf,
            FIND_BTN.x,
            FIND_BTN.y,
            FIND_BTN.w,
            FIND_BTN.h,
            1,
            "Search",
            false,
            self.pressed == Some(10),
            t,
        );
        // Categories.
        let r = CATS;
        sf.fill_rect(r.x, r.y, r.w as u32, r.h as u32, t.well);
        sf.frame(r.x, r.y, r.w as u32, r.h as u32, 1, t.line);
        let n_updates = software::status().count;
        for (i, name) in self.cats.iter().enumerate() {
            let y = r.y + 2 + i as i32 * CAT_ROW + if i >= self.cats.len() - 3 { 8 } else { 0 };
            if i == self.cats.len() - 3 {
                sf.hline(r.x + 4, y - 5, (r.w - 8) as u32, t.disabled);
            }
            let fg = if i == self.cat {
                let bg = if self.focus == 0 { t.sel } else { t.dark };
                sf.fill_rect(r.x + 2, y, (r.w - 4) as u32, CAT_ROW as u32, bg);
                t.sel_text
            } else {
                t.text
            };
            let label = if *name == UPDATES && n_updates > 0 {
                format!("{name} ({n_updates})")
            } else {
                name.to_string()
            };
            sf.text(font, r.x + 6, y + 5, 1, fg, None, &label);
        }
        // Programs.
        let r = LIST;
        sf.fill_rect(r.x, r.y, r.w as u32, r.h as u32, t.well);
        sf.frame(r.x, r.y, r.w as u32, r.h as u32, 1, t.line);
        if self.rows.is_empty() {
            let msg = match self.cats[self.cat] {
                UPDATES => "No updates are waiting. Update All looks for new ones.",
                SEARCH => "Type a word in Find and press Enter.",
                INSTALLED => "None of the Library's programs are installed yet.",
                _ => "",
            };
            sf.text(font, r.x + 8, r.y + 8, 1, t.disabled, None, msg);
        }
        let rows: Vec<(usize, Row)> = self
            .rows
            .iter()
            .cloned()
            .enumerate()
            .skip(self.top)
            .take(Lib::rows_shown())
            .collect();
        for (k, (i, row)) in rows.into_iter().enumerate() {
            let y = r.y + 2 + k as i32 * ROW;
            let on = i == self.sel;
            let (fg, dim) = if on {
                let bg = if self.focus == 1 { t.sel } else { t.dark };
                sf.fill_rect(r.x + 2, y, (r.w - 4) as u32, ROW as u32, bg);
                (t.sel_text, t.sel_text)
            } else {
                (t.text, t.disabled)
            };
            let (name, blurb, tag, icon) = match &row {
                Row::App(a) => {
                    let inst = self.is_installed(a);
                    (
                        a.name.clone(),
                        a.blurb.clone(),
                        if inst { "Installed" } else { "" },
                        self.icon_for(a),
                    )
                }
                Row::Package(p, s) => (p.clone(), s.clone(), "Debian", None),
                Row::Update(u) => (
                    u.package.clone(),
                    match &u.from {
                        Some(f) => format!("{f} to {}", u.to),
                        None => format!("new: {}", u.to),
                    },
                    if u.security { "Security" } else { "Update" },
                    None,
                ),
            };
            match icon {
                Some(ic) => iconset::draw(&mut sf, &ic, r.x + 6, y + 4, 1, t),
                // A package without a picture of its own (HW 4.42).
                None => retro_gfx::icons::icon_picture(
                    &mut sf,
                    retro_gfx::icons::IconArt::Program,
                    r.x + 6,
                    y + 4,
                    1,
                    t,
                ),
            }
            let tw = r.w - 56 - 80;
            sf.text(
                font,
                r.x + 46,
                y + 5,
                1,
                fg,
                None,
                &ellipsize_in(font, &name, tw),
            );
            sf.text(
                font,
                r.x + 46,
                y + 21,
                1,
                dim,
                None,
                &ellipsize_in(font, &blurb, r.w - 56),
            );
            if !tag.is_empty() {
                sf.text(
                    font,
                    r.x + r.w - 8 - text_width(tag),
                    y + 5,
                    1,
                    fg,
                    None,
                    tag,
                );
            }
        }
        // The chosen one, or the job's progress.
        let y = DETAIL_Y;
        let shown = match (&self.job, &self.other) {
            (Some((job, _)), _) => Some(job.progress()),
            (None, Some(o)) => {
                let mut p = o.progress.clone();
                p.text = format!("{}: {}", o.what, p.text);
                Some(p)
            }
            _ => None,
        };
        if let Some(p) = shown {
            sf.text(
                font,
                8,
                y,
                1,
                t.text,
                None,
                &ellipsize_in(font, &p.text, W - 80),
            );
            sf.text(
                font,
                W - 50,
                y,
                1,
                t.text,
                None,
                &format!("{} %", p.percent),
            );
            let bw = W - 16;
            sf.fill_rect(8, y + 20, bw as u32, 16, t.well);
            sf.frame(8, y + 20, bw as u32, 16, 1, t.line);
            let fill = (bw - 4) * p.percent.min(100) as i32 / 100;
            if fill > 0 {
                sf.fill_rect(10, y + 22, fill as u32, 12, t.active);
            }
        } else if let Some(row) = self.rows.get(self.sel).cloned() {
            let lines: Vec<(String, bool)> = match &row {
                Row::App(a) => {
                    let mut v = vec![
                        (a.name.clone(), true),
                        (a.blurb.clone(), false),
                        (format!("Package: {}", a.packages.join(", ")), false),
                    ];
                    if let Some(n) = &a.note {
                        v.push((n.clone(), false));
                    }
                    v
                }
                Row::Package(p, s) => vec![
                    (p.clone(), true),
                    (s.clone(), false),
                    (
                        "From the Debian archive; not checked for RetroOS.".into(),
                        false,
                    ),
                ],
                Row::Update(u) => vec![
                    (u.package.clone(), true),
                    (
                        if u.security {
                            "A security update.".into()
                        } else {
                            "An update.".into()
                        },
                        false,
                    ),
                    ("Update All installs every update.".into(), false),
                ],
            };
            for (i, (l, strong)) in lines.iter().enumerate() {
                let fg = if *strong { t.text } else { t.disabled };
                sf.text(
                    font,
                    10,
                    y + i as i32 * 16,
                    1,
                    fg,
                    None,
                    &ellipsize_in(font, l, W - 20),
                );
            }
        }
        for i in 0..3 {
            let b = button(i);
            let label = self.button_label(i);
            if label.is_empty() {
                continue;
            }
            push_button(
                &mut sf,
                b.x,
                b.y,
                b.w,
                b.h,
                1,
                label,
                false,
                self.pressed == Some(i),
                t,
            );
            if self.busy() {
                // Greyed: the label redrawn in the disabled colour.
                let tw = text_width(label);
                sf.fill_rect(b.x + 4, b.y + 4, (b.w - 8) as u32, (b.h - 8) as u32, t.face);
                sf.text(
                    font,
                    b.x + (b.w - tw) / 2,
                    b.y + 6,
                    1,
                    t.disabled,
                    None,
                    label,
                );
            }
        }
    }

    /// Buttons: the chosen program's action, Run, Update All.
    fn button_label(&self, i: usize) -> &'static str {
        match i {
            0 => match self.rows.get(self.sel) {
                Some(Row::App(a)) if self.is_installed(a) => "Remove",
                Some(Row::App(_) | Row::Package(..)) => "Install",
                _ => "",
            },
            1 => match self.rows.get(self.sel) {
                Some(Row::App(a)) if self.is_installed(a) => "Run",
                _ => "",
            },
            _ => "Update All",
        }
    }

    fn message(&mut self, ui: &mut Ui, lines: Vec<String>) {
        let Some(win) = self.win else { return };
        if let Some(m) = self.message.take() {
            ui.close(m.surface);
        }
        let w = lines
            .iter()
            .map(|l| text_width(l))
            .max()
            .unwrap_or(0)
            .max(160)
            + 40;
        let h = 56 + lines.len() as i32 * 16;
        let surface = ui.dialog(win, TITLE, (w.min(W), h));
        self.message = Some(Message {
            surface,
            lines,
            pressed: false,
        });
    }

    /// Run `op`, asking for the password first while sudo needs it.
    fn begin(&mut self, ui: &mut Ui, op: Op, done: Vec<String>) {
        if self.job.is_some() || self.ask.is_some() {
            return;
        }
        if let Some(o) = self.other.clone().or_else(software::running) {
            // One at a time: wait for the other, then start this one.
            self.message(
                ui,
                vec![
                    format!("{} is still running.", o.what),
                    "This starts when it has finished.".into(),
                ],
            );
            self.other = Some(o);
            self.queued = Some((op, done));
            ui.timer(TIMER_JOB, Duration::from_millis(500));
            return;
        }
        if software::needs_password() {
            let Some(win) = self.win else { return };
            let surface = ui.dialog(win, "Administrator", (ASK_W, ASK_H));
            self.ask = Some(Ask {
                surface,
                field: TextField::password(),
                op,
                done,
                wrong: false,
                pressed: None,
            });
            return;
        }
        self.run(ui, op, done);
    }

    fn run(&mut self, ui: &mut Ui, op: Op, done: Vec<String>) {
        match software::start(op) {
            Ok(j) => {
                self.job = Some((j, done));
                ui.timer(TIMER_JOB, Duration::from_millis(200));
            }
            Err(e) => self.message(ui, vec![e]),
        }
        if let Some(w) = self.win {
            ui.redraw(w);
        }
    }

    fn press(&mut self, ui: &mut Ui, i: usize) {
        if self.busy() {
            return;
        }
        match (i, self.rows.get(self.sel).cloned()) {
            (0, Some(Row::App(a))) if self.is_installed(&a) => {
                let done = vec![format!("{} has been removed.", a.name)];
                self.begin(ui, Op::Remove(vec![a.packages[0].clone()]), done);
            }
            (0, Some(Row::App(a))) => {
                let mut done = vec![format!("{} is installed.", a.name)];
                done.push("Start it with Run, or from Program Manager.".into());
                if let Some(n) = &a.note {
                    done.push(n.clone());
                }
                self.begin(ui, Op::Install(a.packages.clone()), done);
            }
            (0, Some(Row::Package(p, _))) => {
                self.begin(
                    ui,
                    Op::Install(vec![p.clone()]),
                    vec![format!("{p} is installed.")],
                );
            }
            (1, Some(Row::App(a))) if self.is_installed(&a) => {
                let cmd = retro_apps::find_app(&a.run)
                    .map(|d| d.exec)
                    .filter(|e| !e.is_empty())
                    .unwrap_or_else(|| vec![a.run.clone()]);
                if let Err(e) = station::launch(&a.name.to_ascii_uppercase(), &cmd) {
                    self.message(ui, vec![format!("Cannot start {}: {e}", a.name)]);
                }
            }
            (2, _) => {
                self.begin(
                    ui,
                    Op::UpgradeAll,
                    vec!["The updates are installed.".into()],
                );
            }
            _ => {}
        }
    }

    fn choose_cat(&mut self, i: usize) {
        self.cat = i.min(self.cats.len() - 1);
        self.sel = 0;
        self.top = 0;
        self.load();
    }

    fn search(&mut self) {
        self.cat = self.cats.len() - 1;
        self.sel = 0;
        self.top = 0;
        self.focus = 1;
        self.load();
    }

    fn window_event(&mut self, ui: &mut Ui, ev: Event) {
        let n = self.rows.len();
        match ev {
            Event::Close => return ui.quit(),
            Event::Button {
                button: LEFT_BUTTON,
                pressed,
                x,
                y,
            } => {
                if !pressed {
                    match self.pressed.take() {
                        Some(10) if FIND_BTN.contains(x, y) => self.search(),
                        Some(b) if b < 3 && button(b).contains(x, y) => self.press(ui, b),
                        _ => {}
                    }
                } else if FIND_BTN.contains(x, y) {
                    self.pressed = Some(10);
                } else if let Some(b) = (0..3).find(|b| button(*b).contains(x, y)) {
                    if !self.button_label(b).is_empty() {
                        self.pressed = Some(b);
                    }
                } else if FIND.contains(x, y) {
                    self.focus = 2;
                    self.find.click(x - FIND.x);
                } else if CATS.contains(x, y) {
                    self.focus = 0;
                    let i = (0..self.cats.len()).find(|i| {
                        let ry = CATS.y
                            + 2
                            + *i as i32 * CAT_ROW
                            + if *i >= self.cats.len() - 3 { 8 } else { 0 };
                        y >= ry && y < ry + CAT_ROW
                    });
                    if let Some(i) = i {
                        self.choose_cat(i);
                    }
                } else if LIST.contains(x, y) {
                    self.focus = 1;
                    let i = self.top + ((y - LIST.y - 2).max(0) / ROW) as usize;
                    if i < n {
                        let again = i == self.sel;
                        self.sel = i;
                        if again && self.button_label(0) == "Install" {
                            // A second click on a program installs it.
                            self.press(ui, 0);
                        }
                    }
                }
            }
            Event::Scroll { steps } => {
                let max = n.saturating_sub(Lib::rows_shown());
                self.top =
                    (self.top as isize + steps.round() as isize).clamp(0, max as isize) as usize;
            }
            Event::Key {
                sym,
                text,
                ctrl,
                shift,
                ..
            } => match (self.focus, sym) {
                (_, ESCAPE) => return ui.quit(),
                (_, TAB) => self.focus = (self.focus + 1) % 3,
                (_, ISO_LEFT_TAB) => self.focus = (self.focus + 2) % 3,
                (2, RETURN | KP_ENTER) => self.search(),
                (2, _) => {
                    self.find.key(sym, text.as_deref(), ctrl, shift);
                }
                (0, UP) => self.choose_cat(self.cat.saturating_sub(1)),
                (0, DOWN) => self.choose_cat(self.cat + 1),
                (0, RIGHT | RETURN | KP_ENTER) if n > 0 => self.focus = 1,
                (1, UP) => self.sel = self.sel.saturating_sub(1),
                (1, DOWN) => self.sel = (self.sel + 1).min(n.saturating_sub(1)),
                (1, PAGE_UP) => self.sel = self.sel.saturating_sub(Lib::rows_shown()),
                (1, PAGE_DOWN) => {
                    self.sel = (self.sel + Lib::rows_shown()).min(n.saturating_sub(1))
                }
                (1, LEFT) => self.focus = 0,
                (1, RETURN | KP_ENTER) => {
                    if self.button_label(0).is_empty() {
                        self.press(ui, 2);
                    } else {
                        self.press(ui, 0);
                    }
                }
                (1, DELETE) if self.button_label(0) == "Remove" => self.press(ui, 0),
                _ if text.as_deref() == Some("/") => self.focus = 2,
                _ => {}
            },
            _ => {}
        }
        self.reveal();
        if let Some(w) = self.win {
            ui.redraw(w);
        }
    }

    fn ask_event(&mut self, ui: &mut Ui, ev: Event) {
        let Some(a) = self.ask.as_mut() else { return };
        let ok = Rect::new(ASK_W - 2 * 84 - 4, ASK_H - BTN_H - 10, 76, BTN_H);
        let cancel = Rect::new(ASK_W - 84, ASK_H - BTN_H - 10, 76, BTN_H);
        let mut submit = false;
        let mut close = false;
        match ev {
            Event::Close => close = true,
            Event::Key {
                sym,
                text,
                ctrl,
                shift,
                ..
            } => match sym {
                ESCAPE => close = true,
                RETURN | KP_ENTER => submit = true,
                _ => {
                    a.field.key(sym, text.as_deref(), ctrl, shift);
                }
            },
            Event::Button {
                button: LEFT_BUTTON,
                pressed,
                x,
                y,
            } => {
                if pressed {
                    a.pressed = if ok.contains(x, y) {
                        Some(0)
                    } else if cancel.contains(x, y) {
                        Some(1)
                    } else {
                        None
                    };
                } else {
                    match a.pressed.take() {
                        Some(0) if ok.contains(x, y) => submit = true,
                        Some(1) if cancel.contains(x, y) => close = true,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        if submit {
            match software::authorize(&a.field.value()) {
                Ok(()) => {
                    let a = self.ask.take().unwrap();
                    ui.close(a.surface);
                    return self.run(ui, a.op, a.done);
                }
                Err(AdminError::WrongPassword) => {
                    a.wrong = true;
                    a.field = TextField::password();
                }
                Err(e) => {
                    let a = self.ask.take().unwrap();
                    ui.close(a.surface);
                    let m = e
                        .to_string()
                        .replace("change this setting", "install software");
                    return self.message(ui, vec![m]);
                }
            }
        }
        if close {
            let a = self.ask.take().unwrap();
            ui.close(a.surface);
            if self.direct.is_some() {
                ui.quit();
            }
            return;
        }
        ui.redraw(a.surface);
    }

    fn draw_ask(&self, c: &mut Canvas, t: &Theme) {
        let Some(a) = &self.ask else { return };
        let mut sf = c.surface();
        sf.clear(t.face);
        let font = lantern_12();
        let what = match a.op {
            Op::Install(_) => "Installing software needs your password.",
            Op::Remove(_) => "Removing software needs your password.",
            _ => "Installing updates needs your password.",
        };
        let first = if a.wrong {
            "That password is not right. Try again."
        } else {
            what
        };
        sf.text(font, 14, 14, 1, t.text, None, first);
        sf.text(font, 14, 44, 1, t.text, None, "Password:");
        a.field.draw(&mut sf, 90, 40, ASK_W - 104, true, t);
        for (i, l) in ["OK", "Cancel"].iter().enumerate() {
            let x = ASK_W - (2 - i as i32) * 84 - if i == 0 { 4 } else { 0 };
            push_button(
                &mut sf,
                x,
                ASK_H - BTN_H - 10,
                76,
                BTN_H,
                1,
                l,
                i == 0,
                a.pressed == Some(i),
                t,
            );
        }
    }

    fn draw_message(&self, c: &mut Canvas, t: &Theme) {
        let Some(m) = &self.message else { return };
        let (w, h) = (c.width as i32, c.height as i32);
        let mut sf = c.surface();
        sf.clear(t.face);
        for (i, l) in m.lines.iter().enumerate() {
            sf.text(lantern_12(), 20, 14 + i as i32 * 16, 1, t.text, None, l);
        }
        push_button(
            &mut sf,
            (w - 76) / 2,
            h - BTN_H - 10,
            76,
            BTN_H,
            1,
            "OK",
            true,
            m.pressed,
            t,
        );
    }
}

impl Program for Lib {
    fn start(&mut self, ui: &mut Ui) {
        if let Some(pkgs) = self.direct.clone() {
            // Installing for another program: our own small session.
            self.win = Some(ui.window(TITLE, "retroos.software-library", (W, H), true));
            self.cat = self.cats.len() - 1;
            self.rows = pkgs
                .iter()
                .map(|p| Row::Package(p.clone(), "Asked for by another program".into()))
                .collect();
            let done = vec![format!("Installed: {}.", pkgs.join(", "))];
            self.begin(ui, Op::Install(pkgs), done);
            return;
        }
        if let Some(w) = station::windows().iter().find(|w| w.title == TITLE) {
            let _ = station::request(&["SWITCH", &w.id.to_string()]);
            ui.quit();
            return;
        }
        self.win = Some(ui.window(TITLE, "retroos.software-library", (W, H), true));
        // An installation started from another window: show it.
        self.other = software::running();
        if self.other.is_some() {
            ui.timer(TIMER_JOB, Duration::from_millis(500));
        }
        if let Some(s) = self.start.take() {
            if let Some(i) = self.cats.iter().position(|c| c.eq_ignore_ascii_case(&s)) {
                self.choose_cat(i);
                self.focus = 1;
            }
        }
    }

    fn draw(&mut self, ui: &Ui, id: SurfaceId, c: &mut Canvas) {
        let t = ui.theme;
        if Some(id) == self.win {
            self.draw_window(c, &t);
        } else if self.ask.as_ref().is_some_and(|a| a.surface == id) {
            self.draw_ask(c, &t);
        } else if self.message.as_ref().is_some_and(|m| m.surface == id) {
            self.draw_message(c, &t);
        }
    }

    fn event(&mut self, ui: &mut Ui, id: SurfaceId, ev: Event) {
        if let Some(m) = self.message.as_mut().filter(|m| m.surface == id) {
            let close = match ev {
                Event::Close => true,
                Event::Key { sym, .. } => matches!(sym, RETURN | KP_ENTER | ESCAPE | SPACE),
                Event::Button {
                    button: LEFT_BUTTON,
                    pressed,
                    ..
                } => {
                    m.pressed = pressed;
                    ui.redraw(m.surface);
                    !pressed
                }
                _ => false,
            };
            if close {
                if let Some(m) = self.message.take() {
                    ui.close(m.surface);
                }
                if self.direct.is_some() && !self.busy() && self.queued.is_none() {
                    ui.quit();
                }
            }
            return;
        }
        if self.ask.as_ref().is_some_and(|a| a.surface == id) {
            return self.ask_event(ui, ev);
        }
        if Some(id) == self.win {
            self.window_event(ui, ev);
        }
    }

    fn timer(&mut self, ui: &mut Ui, token: u32) {
        if token != TIMER_JOB {
            return;
        }
        if self.job.is_none() && self.other.is_some() {
            self.other = software::running();
            if self.other.is_some() {
                ui.timer(TIMER_JOB, Duration::from_millis(500));
            } else {
                // It has finished: what is installed now, then ours.
                self.installed = software::installed_catalog();
                self.icons.clear();
                self.load();
                if let Some((op, done)) = self.queued.take() {
                    self.begin(ui, op, done);
                }
            }
            if let Some(w) = self.win {
                ui.redraw(w);
            }
            return;
        }
        let finished = self.job.as_ref().and_then(|(j, _)| j.finished());
        match finished {
            None if self.job.is_some() => ui.timer(TIMER_JOB, Duration::from_millis(200)),
            None => {}
            Some(r) => {
                let (_, done) = self.job.take().unwrap();
                self.installed = software::installed_catalog();
                self.icons.clear();
                self.load();
                match r {
                    Ok(()) => self.message(ui, done),
                    Err(e) => self.message(ui, vec![e]),
                }
            }
        }
        if let Some(w) = self.win {
            ui.redraw(w);
        }
    }
}

fn main() {
    let mut cats: Vec<&'static str> = catalog().categories.iter().map(|c| c.name).collect();
    cats.extend([INSTALLED, UPDATES, SEARCH]);
    let lib = Lib {
        win: None,
        cats,
        cat: 0,
        rows: Vec::new(),
        sel: 0,
        top: 0,
        installed: software::installed_catalog(),
        find: TextField::default(),
        focus: 0,
        pressed: None,
        job: None,
        other: None,
        queued: None,
        ask: None,
        message: None,
        icons: HashMap::new(),
        start: std::env::args().nth(1).filter(|a| a != "--install"),
        direct: (std::env::args().nth(1).as_deref() == Some("--install"))
            .then(|| std::env::args().skip(2).collect()),
    };
    let mut lib = lib;
    lib.load();
    if let Err(e) = retro_ui::run(Box::new(lib)) {
        eprintln!("software-library: {e}");
        std::process::exit(1);
    }
}

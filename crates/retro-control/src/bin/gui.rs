//! Control Panel on the Desktop (docs/DESIGN.md §9.2): a window of applet
//! icons in the manner of Windows 3.1's, with a status line describing the
//! selected one. Double-click or Enter opens the applet as a dialog of the
//! same settings CONTROL shows (retro_control::load / apply).
//!
//!   control-panel [applet]     open that applet at once (DISPLAY, TIME, …)

use retro_control::{applet_by_letter, applet_named, apply, load, Applet, Page, Value, APPLETS};
use retro_gfx::font::lantern_12;
use retro_gfx::iconset;
use retro_gfx::widgets::{bevel, ellipsize, ellipsize_in, push_button, text_width};
use retro_gfx::{Canvas, Surface, Theme};
use retro_ui::controls::{check_box, ListBox, TextField, FIELD_H, ROW_H};
use retro_ui::keys::*;
use retro_ui::{station, Event, Program, Rect, SurfaceId, Ui};
use std::time::{Duration, Instant};

const TITLE: &str = "Control Panel";
const CELL_W: i32 = 84;
const CELL_H: i32 = 64;
const GRID_Y: i32 = 8;
const STATUS_H: i32 = 22;
const LEFT_BUTTON: u32 = 0x110;

const DLG_W: i32 = 400;
const CTRL_X: i32 = 150;
const ROW: i32 = 28;
const LIST_H: i32 = 4 + 8 * ROW_H;
const BTN_W: i32 = 76;
const BTN_H: i32 = 24;

/// The editor of one field in a dialog.
enum Ctl {
    Plain,
    Text(TextField),
    List(ListBox),
}

struct Dialog {
    surface: SurfaceId,
    page: Page,
    ctls: Vec<Ctl>,
    /// Where each field's control is.
    rects: Vec<Rect>,
    /// Field index, or fields.len() = OK, + 1 = Cancel.
    focus: usize,
    pressed: Option<usize>,
    h: i32,
}

struct Message {
    surface: SurfaceId,
    lines: Vec<String>,
    pressed: bool,
}

struct Panel {
    win: Option<SurfaceId>,
    sel: usize,
    last_click: Option<(usize, Instant)>,
    dialog: Option<Dialog>,
    message: Option<Message>,
    start: Option<Applet>,
}

fn cell(i: usize) -> Rect {
    Rect::new(8 + i as i32 * CELL_W, GRID_Y, CELL_W, CELL_H)
}

fn win_size() -> (i32, i32) {
    (
        16 + APPLETS.len() as i32 * CELL_W,
        GRID_Y + CELL_H + 8 + STATUS_H,
    )
}

impl Dialog {
    fn new(ui: &mut Ui, parent: SurfaceId, page: Page) -> Dialog {
        let top = if page.notice.is_some() { 38 } else { 14 };
        let mut y = top;
        let mut rects = Vec::new();
        let mut ctls = Vec::new();
        for f in &page.fields {
            let (h, ctl) = match &f.value {
                Value::List(items, sel) => {
                    let mut l = ListBox::new(items.iter().map(|(_, l)| l.clone()).collect());
                    l.sel = Some(*sel);
                    (LIST_H, Ctl::List(l))
                }
                Value::Text(s) => (FIELD_H, Ctl::Text(TextField::new(s))),
                _ => (FIELD_H, Ctl::Plain),
            };
            // A long list goes under its label, the dialog's full width.
            if matches!(ctl, Ctl::List(_)) {
                y += FIELD_H;
                rects.push(Rect::new(14, y, DLG_W - 28, h));
            } else {
                rects.push(Rect::new(CTRL_X, y, DLG_W - CTRL_X - 14, h));
            }
            ctls.push(ctl);
            y += h + (ROW - FIELD_H);
        }
        let h = y + 8 + BTN_H + 12;
        let surface = ui.dialog(parent, page.applet.name(), (DLG_W, h));
        let mut d = Dialog {
            surface,
            page,
            ctls,
            rects,
            focus: 0,
            pressed: None,
            h,
        };
        if !d.usable(0) {
            d.next_focus(1);
        }
        // A list wants to show its selection.
        for c in d.ctls.iter_mut() {
            if let Ctl::List(l) = c {
                if let Some(s) = l.sel {
                    l.top = s.saturating_sub(3);
                    l.scroll(0, LIST_H);
                }
            }
        }
        d
    }

    fn n(&self) -> usize {
        self.page.fields.len()
    }

    fn usable(&self, i: usize) -> bool {
        i >= self.n() || self.page.fields[i].enabled
    }

    fn next_focus(&mut self, d: isize) {
        let total = self.n() + 2;
        for _ in 0..total {
            self.focus = (self.focus as isize + d).rem_euclid(total as isize) as usize;
            if self.usable(self.focus) {
                return;
            }
        }
    }

    fn button(&self, i: usize) -> Rect {
        let x = DLG_W - 2 * (BTN_W + 10) - 4 + i as i32 * (BTN_W + 10);
        Rect::new(x, self.h - BTN_H - 12, BTN_W, BTN_H)
    }

    /// Copy the editors' state into the page.
    fn sync(&mut self, i: usize) {
        let f = &mut self.page.fields[i];
        match (&mut f.value, &self.ctls[i]) {
            (Value::Text(s), Ctl::Text(t)) => *s = t.value(),
            (Value::List(_, sel), Ctl::List(l)) => *sel = l.sel.unwrap_or(*sel),
            _ => {}
        }
    }

    fn draw(&self, c: &mut Canvas, t: &Theme) {
        let mut sf = c.surface();
        sf.clear(t.face);
        let font = lantern_12();
        if let Some(n) = &self.page.notice {
            sf.text(font, 14, 12, 1, t.text, None, &ellipsize(n, DLG_W - 28));
        }
        for (i, f) in self.page.fields.iter().enumerate() {
            let r = self.rects[i];
            let fg = if f.enabled { t.text } else { t.disabled };
            let ly = if r.x == 14 {
                r.y - FIELD_H + 2
            } else {
                r.y + 4
            };
            sf.text(font, 14, ly, 1, fg, None, &format!("{}:", f.label));
            let focused = self.focus == i;
            match (&f.value, &self.ctls[i]) {
                (_, Ctl::Text(tf)) => {
                    if f.enabled {
                        tf.draw(&mut sf, r.x, r.y, 110, focused, t);
                    } else {
                        sf.fill_rect(r.x, r.y, 110, FIELD_H as u32, t.face);
                        sf.frame(r.x, r.y, 110, FIELD_H as u32, 1, t.disabled);
                        sf.text(font, r.x + 4, r.y + 4, 1, t.disabled, None, &f.text());
                    }
                }
                (_, Ctl::List(l)) if f.enabled => l.draw(&mut sf, r, focused, t),
                (_, Ctl::List(_)) => {
                    sf.frame(r.x, r.y, r.w as u32, r.h as u32, 1, t.disabled);
                    sf.text(font, r.x + 5, r.y + 4, 1, t.disabled, None, &f.shown());
                }
                (Value::Choice(..), _) => {
                    draw_choice(&mut sf, r, &f.shown(), focused, f.enabled, t)
                }
                (Value::Check(_), _) if !f.enabled => {
                    sf.frame(r.x, r.y + 3, 13, 13, 1, t.disabled);
                }
                (Value::Check(b), _) => {
                    check_box(&mut sf, r.x, r.y + 3, "", *b, t);
                    if focused {
                        dotted(&mut sf, r.x - 2, r.y + 1, 17, 17, t);
                    }
                }
                (Value::Level(n), _) => draw_level(&mut sf, r, *n, focused, f.enabled, t),
                _ => {}
            }
        }
        for (i, label) in ["OK", "Cancel"].iter().enumerate() {
            let b = self.button(i);
            let focus = self.focus == self.n() + i || (i == 0 && self.focus < self.n());
            push_button(
                &mut sf,
                b.x,
                b.y,
                b.w,
                b.h,
                1,
                label,
                focus,
                self.pressed == Some(i),
                t,
            );
        }
    }
}

/// A drop-down-looking box: the value and an arrow button; a click steps.
fn draw_choice(sf: &mut Surface, r: Rect, text: &str, focused: bool, enabled: bool, t: &Theme) {
    let w = r.w.min(190);
    sf.fill_rect(r.x, r.y, w as u32, FIELD_H as u32, t.light);
    sf.frame(r.x, r.y, w as u32, FIELD_H as u32, 1, t.line);
    let font = lantern_12();
    let label = ellipsize_in(font, text, w - 30);
    let fg = if !enabled {
        t.disabled
    } else if focused {
        sf.fill_rect(
            r.x + 2,
            r.y + 2,
            (font.text_width(&label) as i32 + 4) as u32,
            16,
            t.sel,
        );
        t.sel_text
    } else {
        t.text
    };
    sf.text(font, r.x + 4, r.y + 4, 1, fg, None, &label);
    let bx = r.x + w - 18;
    bevel(sf, bx, r.y + 1, 17, FIELD_H - 2, 1, false, t);
    sf.vline(bx - 1, r.y + 1, (FIELD_H - 2) as u32, t.line);
    // ▼
    for i in 0..4 {
        sf.hline(bx + 5 + i, r.y + 8 + i, (7 - 2 * i) as u32, t.text);
    }
}

fn draw_level(sf: &mut Surface, r: Rect, n: u32, focused: bool, enabled: bool, t: &Theme) {
    let w = 150;
    let y = r.y + 4;
    sf.fill_rect(r.x, y, w as u32, 12, t.light);
    sf.frame(r.x, y, w as u32, 12, 1, t.line);
    let fill = ((w - 2) as u32 * n) / 100;
    if enabled && fill > 0 {
        sf.fill_rect(r.x + 1, y + 1, fill, 10, t.active);
    }
    let fg = if enabled { t.text } else { t.disabled };
    let label = if enabled {
        format!("{n} %")
    } else {
        "No backlight".into()
    };
    sf.text(lantern_12(), r.x + w + 8, r.y + 4, 1, fg, None, &label);
    if focused {
        dotted(sf, r.x - 2, y - 2, (w + 4) as u32, 16, t);
    }
}

/// The dotted focus rectangle.
fn dotted(sf: &mut Surface, x: i32, y: i32, w: u32, h: u32, t: &Theme) {
    for i in (0..w as i32).step_by(2) {
        sf.put(x + i, y, t.text);
        sf.put(x + i, y + h as i32 - 1, t.text);
    }
    for i in (0..h as i32).step_by(2) {
        sf.put(x, y + i, t.text);
        sf.put(x + w as i32 - 1, y + i, t.text);
    }
}

impl Panel {
    fn redraw(&self, ui: &mut Ui) {
        if let Some(w) = self.win {
            ui.redraw(w);
        }
    }

    fn draw_window(&self, c: &mut Canvas, t: &Theme) {
        let (w, h) = (c.width as i32, c.height as i32);
        let mut sf = c.surface();
        sf.clear(t.light);
        let font = lantern_12();
        for (i, a) in APPLETS.iter().enumerate() {
            let r = cell(i);
            if let Some(icon) = iconset::builtin(a.icon()) {
                iconset::draw(&mut sf, icon, r.x + (r.w - 32) / 2, r.y + 4, 1, t);
            }
            let label = ellipsize_in(font, a.name(), r.w - 6);
            let tw = font.text_width(&label) as i32;
            let (lx, ly) = (r.x + (r.w - tw) / 2, r.y + 42);
            let fg = if i == self.sel {
                sf.fill_rect(lx - 2, ly - 1, (tw + 3) as u32, 13, t.sel);
                t.sel_text
            } else {
                t.text
            };
            sf.text(font, lx, ly, 1, fg, None, &label);
        }
        // Status line.
        let sy = h - STATUS_H;
        sf.fill_rect(0, sy, w as u32, STATUS_H as u32, t.face);
        sf.hline(0, sy, w as u32, t.line);
        let blurb = APPLETS[self.sel].blurb();
        sf.text(font, 6, sy + 5, 1, t.text, None, &ellipsize(blurb, w - 12));
    }

    fn open(&mut self, ui: &mut Ui, a: Applet) {
        let Some(win) = self.win else { return };
        if let Some(p) = a.program() {
            // Network: NET in a window of its own.
            let argv: Vec<String> = ["retro-console", "--exec", p]
                .iter()
                .map(|s| s.to_string())
                .collect();
            if let Err(e) = station::launch("NET", &argv) {
                self.message(ui, &[&format!("Cannot start Network: {e}")]);
            }
            return;
        }
        self.close_dialog(ui);
        self.dialog = Some(Dialog::new(ui, win, load(a)));
    }

    fn close_dialog(&mut self, ui: &mut Ui) {
        if let Some(d) = self.dialog.take() {
            ui.close(d.surface);
        }
    }

    fn message(&mut self, ui: &mut Ui, lines: &[&str]) {
        let Some(win) = self.win else { return };
        if let Some(m) = self.message.take() {
            ui.close(m.surface);
        }
        let parent = self.dialog.as_ref().map_or(win, |d| d.surface);
        let w = lines
            .iter()
            .map(|l| text_width(l))
            .max()
            .unwrap_or(0)
            .max(160)
            + 40;
        let h = 56 + lines.len() as i32 * 16;
        let surface = ui.dialog(parent, TITLE, (w, h));
        self.message = Some(Message {
            surface,
            lines: lines.iter().map(|s| s.to_string()).collect(),
            pressed: false,
        });
    }

    fn ok(&mut self, ui: &mut Ui) {
        let Some(d) = self.dialog.as_mut() else {
            return;
        };
        for i in 0..d.n() {
            d.sync(i);
        }
        if !d.page.any_changed() {
            return self.close_dialog(ui);
        }
        match apply(&d.page) {
            Ok(done) => {
                self.close_dialog(ui);
                if let Some(s) = done.scheme {
                    let _ = station::request(&["SCHEME", &s]);
                }
                if done.keymap {
                    let _ = station::request(&["KEYMAP"]);
                }
                if done.restart {
                    self.message(
                        ui,
                        &[
                            "The new screen settings take effect the",
                            "next time the Desktop or RetroDOS starts.",
                        ],
                    );
                }
            }
            Err(e) => self.message(ui, &[&e]),
        }
    }

    fn dialog_event(&mut self, ui: &mut Ui, ev: Event) {
        let Some(d) = self.dialog.as_mut() else {
            return;
        };
        let n = d.n();
        match ev {
            Event::Close => self.close_dialog(ui),
            Event::Button {
                button: LEFT_BUTTON,
                pressed,
                x,
                y,
            } => {
                if !pressed {
                    let hit = d.pressed.take().filter(|b| d.button(*b).contains(x, y));
                    ui.redraw(d.surface);
                    match hit {
                        Some(0) => self.ok(ui),
                        Some(_) => self.close_dialog(ui),
                        None => {}
                    }
                    return;
                }
                if let Some(b) = (0..2).find(|b| d.button(*b).contains(x, y)) {
                    d.pressed = Some(b);
                    d.focus = n + b;
                } else if let Some(i) =
                    (0..n).find(|i| d.rects[*i].contains(x, y) || label_hit(d, *i, x, y))
                {
                    if !d.page.fields[i].enabled {
                        return;
                    }
                    d.focus = i;
                    let r = d.rects[i];
                    let shift = ui.shift();
                    let f = &mut d.page.fields[i];
                    match (&mut d.ctls[i], &mut f.value) {
                        (Ctl::Text(t), _) => t.click(x - r.x),
                        (Ctl::List(l), _) => {
                            l.click(y - r.y);
                        }
                        (_, Value::Level(v)) => {
                            *v = (((x - r.x).clamp(0, 150) * 100 / 150) as u32).clamp(5, 100);
                        }
                        _ if shift => f.step(-1),
                        _ => f.cycle(),
                    }
                    d.sync(i);
                }
                ui.redraw(d.surface);
            }
            Event::Scroll { steps } => {
                if let Some(Ctl::List(l)) = d.ctls.get_mut(d.focus) {
                    l.scroll(steps.round() as isize * 3, LIST_H);
                    ui.redraw(d.surface);
                }
            }
            Event::Key {
                sym,
                text,
                ctrl,
                shift,
                ..
            } => {
                let i = d.focus;
                match sym {
                    ESCAPE => return self.close_dialog(ui),
                    TAB => d.next_focus(1),
                    ISO_LEFT_TAB => d.next_focus(-1),
                    RETURN | KP_ENTER if i == n + 1 => return self.close_dialog(ui),
                    RETURN | KP_ENTER => return self.ok(ui),
                    SPACE if i == n + 1 => return self.close_dialog(ui),
                    SPACE if i == n => return self.ok(ui),
                    _ if i >= n => {
                        if matches!(sym, LEFT | RIGHT) {
                            d.focus = if sym == LEFT { n } else { n + 1 };
                        }
                    }
                    _ => {
                        let f = &mut d.page.fields[i];
                        match &mut d.ctls[i] {
                            Ctl::Text(t) => {
                                t.key(sym, text.as_deref(), ctrl, shift);
                            }
                            Ctl::List(l) => {
                                if !l.key(sym, LIST_H) {
                                    // A letter jumps to the next item starting with it.
                                    if let Some(c) = text.as_deref().and_then(|t| t.chars().next())
                                    {
                                        jump(l, c);
                                    }
                                }
                            }
                            Ctl::Plain => match sym {
                                LEFT | UP => f.step(-1),
                                RIGHT | DOWN => f.step(1),
                                SPACE => f.cycle(),
                                _ => {}
                            },
                        }
                        d.sync(i);
                    }
                }
                ui.redraw(d.surface);
            }
            _ => {}
        }
    }
}

/// A click on a field's label focuses (and for a check box, toggles) it.
fn label_hit(d: &Dialog, i: usize, x: i32, y: i32) -> bool {
    let r = d.rects[i];
    matches!(d.page.fields[i].value, Value::Check(_)) && x < r.x && y >= r.y && y < r.y + FIELD_H
}

fn jump(l: &mut ListBox, c: char) {
    let c = c.to_lowercase().next().unwrap_or(c);
    let n = l.items.len();
    let from = l.sel.map_or(0, |s| s + 1);
    if let Some(i) = (0..n)
        .map(|k| (from + k) % n)
        .find(|i| l.items[*i].to_lowercase().starts_with(c))
    {
        l.sel = Some(i);
        l.top = i.saturating_sub(3);
        l.scroll(0, LIST_H);
    }
}

impl Program for Panel {
    fn start(&mut self, ui: &mut Ui) {
        if let Some(w) = station::windows().iter().find(|w| w.title == TITLE) {
            let _ = station::request(&["SWITCH", &w.id.to_string()]);
            ui.quit();
            return;
        }
        let win = ui.window(TITLE, "retroos.control", win_size(), true);
        self.win = Some(win);
        if let Some(a) = self.start.take() {
            self.sel = APPLETS.iter().position(|x| *x == a).unwrap_or(0);
            self.open(ui, a);
        }
    }

    fn draw(&mut self, ui: &Ui, id: SurfaceId, c: &mut Canvas) {
        let t = &ui.theme;
        if Some(id) == self.win {
            self.draw_window(c, t);
        } else if let Some(d) = self.dialog.as_ref().filter(|d| d.surface == id) {
            d.draw(c, t);
        } else if let Some(m) = self.message.as_ref().filter(|m| m.surface == id) {
            let (w, h) = (c.width as i32, c.height as i32);
            let mut sf = c.surface();
            sf.clear(t.face);
            for (i, l) in m.lines.iter().enumerate() {
                sf.text(lantern_12(), 20, 14 + i as i32 * 16, 1, t.text, None, l);
            }
            push_button(
                &mut sf,
                (w - BTN_W) / 2,
                h - BTN_H - 10,
                BTN_W,
                BTN_H,
                1,
                "OK",
                true,
                m.pressed,
                t,
            );
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
            }
            return;
        }
        if self.dialog.as_ref().is_some_and(|d| d.surface == id) {
            return self.dialog_event(ui, ev);
        }
        if Some(id) != self.win {
            return;
        }
        match ev {
            Event::Close => ui.quit(),
            Event::Button {
                button: LEFT_BUTTON,
                pressed: true,
                x,
                y,
            } => {
                if let Some(i) = (0..APPLETS.len()).find(|i| cell(*i).contains(x, y)) {
                    self.sel = i;
                    let double = self
                        .last_click
                        .is_some_and(|(j, t)| j == i && t.elapsed() < Duration::from_millis(500));
                    self.last_click = Some((i, Instant::now()));
                    if double {
                        self.last_click = None;
                        self.open(ui, APPLETS[i]);
                    }
                    self.redraw(ui);
                }
            }
            Event::Key { sym, text, .. } => {
                let n = APPLETS.len();
                match sym {
                    LEFT | UP => self.sel = self.sel.saturating_sub(1),
                    RIGHT | DOWN => self.sel = (self.sel + 1).min(n - 1),
                    HOME => self.sel = 0,
                    END => self.sel = n - 1,
                    RETURN | KP_ENTER => return self.open(ui, APPLETS[self.sel]),
                    _ => {
                        let c = text.as_deref().and_then(|t| t.chars().next());
                        if let Some(i) = c.and_then(|c| applet_by_letter(self.sel, c)) {
                            self.sel = i;
                        }
                    }
                }
                self.redraw(ui);
            }
            _ => {}
        }
    }
}

fn main() {
    let start = std::env::args().nth(1).and_then(|a| applet_named(&a));
    let panel = Panel {
        win: None,
        sel: 0,
        last_click: None,
        dialog: None,
        message: None,
        start,
    };
    if let Err(e) = retro_ui::run(Box::new(panel)) {
        eprintln!("control-panel: {e}");
        std::process::exit(1);
    }
}

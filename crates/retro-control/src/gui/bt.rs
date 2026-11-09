//! Control Panel → Bluetooth → Devices: the Desktop's counterpart of BT
//! (retro_sys::bt, BlueZ). Devices Bluetooth can see, paired ones first;
//! Pair (Connect, Disconnect: the first button follows the chosen device),
//! Remove, Search. Searching starts when the dialog opens. While a keyboard
//! pairs, a box in the dialog shows the number to type on it; a phone or
//! computer asks whether both numbers match. Pairing runs on a thread
//! (retro_sys::bt), so the dialog keeps drawing.

use super::wifi::Out;
use retro_gfx::font::lantern_12;
use retro_gfx::widgets::{ellipsize_in, push_button, text_width};
use retro_gfx::{Canvas, Surface, Theme};
use retro_sys::bt::{self, Absent, BtApi, Device, Job, Prompt};
use retro_ui::controls::ROW_H;
use retro_ui::keys::*;
use retro_ui::{Event, Rect, SurfaceId, Ui};
use std::time::Duration;

pub const W: i32 = 440;
const LIST: Rect = Rect {
    x: 14,
    y: 34,
    w: W - 28,
    h: 4 + 9 * ROW_H,
};
const H: i32 = LIST.y + LIST.h + 52;
const BTN_W: i32 = 80;
const BTN_H: i32 = 24;
const N_BUTTONS: usize = 4;
const LEFT_BUTTON: u32 = 0x110;

/// Timer token (the Control Panel routes it here).
pub const TIMER_BT: u32 = 22;
const TICK: Duration = Duration::from_millis(500);
/// How many ticks to wait for bluetoothd after it was turned on.
const START_TICKS: u32 = 16;

/// The pairing box over the list.
const PROMPT: Rect = Rect {
    x: 70,
    y: 40,
    w: W - 140,
    h: 140,
};

pub struct BtDlg {
    pub surface: SurfaceId,
    api: Result<Box<dyn BtApi>, Absent>,
    devs: Vec<Device>,
    sel: usize,
    top: usize,
    /// 0 = the list, 1… = the buttons.
    focus: usize,
    pressed: Option<usize>,
    /// Pressed Yes (0) / No (1) in a confirm box.
    pressed_answer: Option<usize>,
    /// Ticks left to wait for bluetoothd to come up.
    starting: u32,
}

fn bars(sf: &mut Surface, x: i32, y: i32, rssi: i16, fg: retro_gfx::Rgb, dim: retro_gfx::Rgb) {
    let n = match rssi {
        d if d >= -55 => 4,
        d if d >= -67 => 3,
        d if d >= -80 => 2,
        _ => 1,
    };
    for i in 0..4 {
        let h = 3 + i * 3;
        let c = if i < n { fg } else { dim };
        sf.fill_rect(x + i * 4, y + 12 - h, 3, h as u32, c);
    }
}

impl BtDlg {
    pub fn new(ui: &mut Ui, parent: SurfaceId) -> BtDlg {
        let api = bt::open_api();
        let starting = match &api {
            Err(Absent::Off | Absent::NoAdapter) if bt::enabled() => START_TICKS,
            _ => 0,
        };
        let surface = ui.dialog(parent, "Bluetooth Devices", (W, H));
        let mut d = BtDlg {
            surface,
            api,
            devs: Vec::new(),
            sel: 0,
            top: 0,
            focus: 0,
            pressed: None,
            pressed_answer: None,
            starting,
        };
        d.opened();
        ui.timer(TIMER_BT, TICK);
        d
    }

    fn opened(&mut self) {
        if let Ok(b) = &self.api {
            if b.powered() {
                let _ = b.discover(true);
            }
        }
        self.refresh();
    }

    fn rows() -> usize {
        ((LIST.h - 4) / ROW_H) as usize
    }

    /// Re-read the devices, keeping the same one chosen. A finished job's
    /// error comes back for a message box.
    fn refresh(&mut self) -> Option<String> {
        let Ok(b) = &self.api else { return None };
        let failed = match b.take_result() {
            Some((_, Err(e))) => Some(e),
            _ => None,
        };
        if let Ok(d) = b.devices() {
            let keep = self.devs.get(self.sel).map(|d| d.path.clone());
            self.devs = d;
            if let Some(i) = keep.and_then(|k| self.devs.iter().position(|d| d.path == k)) {
                self.sel = i;
            }
            self.sel = self.sel.min(self.devs.len().saturating_sub(1));
            self.reveal();
        }
        failed
    }

    fn reveal(&mut self) {
        let rows = BtDlg::rows();
        if self.sel < self.top {
            self.top = self.sel;
        } else if self.sel >= self.top + rows {
            self.top = self.sel + 1 - rows;
        }
    }

    fn chosen(&self) -> Option<Device> {
        self.devs.get(self.sel).cloned()
    }

    fn prompt(&self) -> Option<Prompt> {
        self.api.as_ref().ok().and_then(|b| b.prompt())
    }

    fn busy(&self) -> Option<Job> {
        self.api.as_ref().ok().and_then(|b| b.busy())
    }

    /// The first button's label: what Enter does to the chosen device.
    fn first_label(&self) -> &'static str {
        match self.chosen() {
            Some(d) if d.connected => "Disconnect",
            Some(d) if d.paired => "Connect",
            _ => "Pair",
        }
    }

    fn label(&self, b: usize) -> &'static str {
        match b {
            0 => self.first_label(),
            1 => "Remove",
            2 => {
                if self.api.as_ref().is_ok_and(|b| b.discovering()) {
                    "Stop"
                } else {
                    "Search"
                }
            }
            _ => "Close",
        }
    }

    fn status(&self) -> String {
        if self.starting > 0 {
            return "Starting Bluetooth...".into();
        }
        let b = match &self.api {
            Ok(b) => b,
            Err(a) => return a.text(),
        };
        if let Some(j) = self.busy() {
            return j.busy_text();
        }
        if !b.powered() {
            "The adapter is switched off.".into()
        } else if b.discovering() {
            "Searching... Put the device in pairing mode.".into()
        } else if self.devs.is_empty() {
            "No devices. Press Search.".into()
        } else {
            "Choose a device and press Pair.".into()
        }
    }

    fn button(b: usize) -> Rect {
        let x = LIST.x + b as i32 * (BTN_W + 8);
        Rect::new(x, H - BTN_H - 12, BTN_W, BTN_H)
    }

    /// Yes (0) and No (1) side by side, or Cancel (2) in the middle.
    fn answer_button(i: usize) -> Rect {
        let w = 64;
        let x = if i == 2 {
            PROMPT.x + (PROMPT.w - w) / 2
        } else {
            PROMPT.x + PROMPT.w / 2 - w - 6 + i as i32 * (w + 12)
        };
        Rect::new(x, PROMPT.y + PROMPT.h - BTN_H - 10, w, BTN_H)
    }

    pub fn draw(&self, c: &mut Canvas, t: &Theme) {
        let mut sf = c.surface();
        sf.clear(t.face);
        let font = lantern_12();
        sf.text(
            font,
            LIST.x,
            12,
            1,
            t.text,
            None,
            &ellipsize_in(font, &self.status(), W - 28),
        );
        let r = LIST;
        sf.fill_rect(r.x, r.y, r.w as u32, r.h as u32, t.well);
        sf.frame(r.x, r.y, r.w as u32, r.h as u32, 1, t.line);
        for (row, i) in (self.top..self.devs.len()).take(BtDlg::rows()).enumerate() {
            let d = &self.devs[i];
            let y = r.y + 2 + row as i32 * ROW_H;
            let (fg, dim) = if i == self.sel {
                let bg = if self.focus == 0 { t.sel } else { t.dark };
                sf.fill_rect(r.x + 2, y, (r.w - 4) as u32, ROW_H as u32, bg);
                (t.sel_text, t.well)
            } else {
                (t.text, t.disabled)
            };
            sf.text(
                font,
                r.x + 6,
                y + 2,
                1,
                fg,
                None,
                &ellipsize_in(font, &d.name, 190),
            );
            sf.text(font, r.x + 204, y + 2, 1, fg, None, d.kind.label());
            sf.text(font, r.x + 284, y + 2, 1, fg, None, d.state());
            if let (false, Some(rssi)) = (d.paired, d.rssi) {
                bars(&mut sf, r.x + r.w - 26, y + 2, rssi, fg, dim);
            }
        }
        for b in 0..N_BUTTONS {
            let r = BtDlg::button(b);
            push_button(
                &mut sf,
                r.x,
                r.y,
                r.w,
                r.h,
                1,
                self.label(b),
                self.focus == b + 1 || (b == 0 && self.focus == 0),
                self.pressed == Some(b),
                t,
            );
        }
        if let Some(p) = self.prompt() {
            self.draw_prompt(&mut sf, &p, t);
        }
    }

    fn draw_prompt(&self, sf: &mut Surface, p: &Prompt, t: &Theme) {
        let r = PROMPT;
        sf.fill_rect(r.x, r.y, r.w as u32, r.h as u32, t.face);
        sf.frame(r.x, r.y, r.w as u32, r.h as u32, 2, t.line);
        let font = lantern_12();
        let name = self
            .devs
            .iter()
            .find(|d| d.path == p.device())
            .map_or("the device".to_string(), |d| d.name.clone());
        let lines = p.lines(&name);
        let center = |sf: &mut Surface, y: i32, s: &str, scale: i32| {
            let s = ellipsize_in(font, s, (r.w - 16) / scale);
            let x = r.x + (r.w - text_width(&s) * scale) / 2;
            sf.text(font, x, y, scale as u32, t.text, None, &s);
        };
        center(sf, r.y + 12, &lines[0], 1);
        center(sf, r.y + 32, &lines[1], 2);
        center(sf, r.y + 66, &lines[2], 1);
        match p {
            Prompt::Type { .. } => {
                let b = BtDlg::answer_button(2);
                push_button(
                    sf,
                    b.x,
                    b.y,
                    b.w,
                    b.h,
                    1,
                    "Cancel",
                    true,
                    self.pressed_answer == Some(2),
                    t,
                );
            }
            Prompt::Confirm { .. } => {
                for (i, l) in ["Yes", "No"].iter().enumerate() {
                    let b = BtDlg::answer_button(i);
                    push_button(
                        sf,
                        b.x,
                        b.y,
                        b.w,
                        b.h,
                        1,
                        l,
                        i == 0,
                        self.pressed_answer == Some(i),
                        t,
                    );
                }
            }
        }
    }

    /// The timer: wait for bluetoothd, then keep the list fresh.
    pub fn timer(&mut self, ui: &mut Ui, token: u32) -> Out {
        if token != TIMER_BT {
            return Out::None;
        }
        ui.timer(TIMER_BT, TICK);
        if self.starting > 0 {
            self.starting -= 1;
            self.api = bt::open_api();
            if self.api.is_ok() {
                self.starting = 0;
                self.opened();
            }
            ui.redraw(self.surface);
            return Out::None;
        }
        let failed = self.refresh();
        ui.redraw(self.surface);
        match failed {
            Some(e) => Out::Message(vec![e]),
            None => Out::None,
        }
    }

    fn press(&mut self, b: usize) -> Out {
        if b == 3 {
            return Out::Close;
        }
        let api = match &self.api {
            Ok(a) => a,
            Err(_) if self.starting > 0 => return Out::None,
            Err(a) => return Out::Message(vec![a.text()]),
        };
        if api.busy().is_some() {
            return Out::None;
        }
        match b {
            0 => {
                let Some(d) = self.chosen() else {
                    return Out::None;
                };
                api.start(match (d.paired, d.connected) {
                    (_, true) => Job::Disconnect(d),
                    (true, false) => Job::Connect(d),
                    (false, _) => Job::Pair(d),
                });
            }
            1 => match self.chosen() {
                Some(d) if d.paired => api.start(Job::Remove(d)),
                Some(d) => return Out::Message(vec![format!("{} is not paired.", d.name)]),
                None => {}
            },
            _ => {
                let on = !api.discovering();
                if let Err(e) = api.discover(on) {
                    return Out::Message(vec![e]);
                }
            }
        }
        Out::None
    }

    fn answer(&mut self, yes: bool) {
        if let Ok(b) = &self.api {
            b.answer(yes);
        }
    }

    fn cancel(&mut self) {
        if let Ok(b) = &self.api {
            b.cancel();
        }
    }

    pub fn event(&mut self, ui: &mut Ui, ev: Event) -> Out {
        let out = self.handle(ui, ev);
        ui.redraw(self.surface);
        out
    }

    fn handle(&mut self, ui: &mut Ui, ev: Event) -> Out {
        if let Some(p) = self.prompt() {
            let confirm = matches!(p, Prompt::Confirm { .. });
            let buttons: &[usize] = if confirm { &[0, 1] } else { &[2] };
            match ev {
                Event::Close => return Out::Close,
                Event::Key { sym, text, .. } => {
                    let t = text.as_deref().unwrap_or("").to_ascii_lowercase();
                    if sym == ESCAPE {
                        self.cancel();
                    } else if confirm && (matches!(sym, RETURN | KP_ENTER) || t == "y") {
                        self.answer(true);
                    } else if confirm && t == "n" {
                        self.answer(false);
                    } else if !confirm && matches!(sym, RETURN | KP_ENTER | SPACE) {
                        self.cancel();
                    }
                }
                Event::Button {
                    button: LEFT_BUTTON,
                    pressed,
                    x,
                    y,
                } => {
                    let hit = buttons
                        .iter()
                        .copied()
                        .find(|i| BtDlg::answer_button(*i).contains(x, y));
                    if pressed {
                        self.pressed_answer = hit;
                    } else if let Some(i) = self.pressed_answer.take().filter(|i| hit == Some(*i)) {
                        match i {
                            0 => self.answer(true),
                            1 => self.answer(false),
                            _ => self.cancel(),
                        }
                    }
                }
                _ => {}
            }
            return Out::None;
        }
        match ev {
            Event::Close => return Out::Close,
            Event::Button {
                button: LEFT_BUTTON,
                pressed,
                x,
                y,
            } => {
                if !pressed {
                    let hit = self
                        .pressed
                        .take()
                        .filter(|b| BtDlg::button(*b).contains(x, y));
                    return match hit {
                        Some(b) => self.press(b),
                        None => Out::None,
                    };
                }
                if let Some(b) = (0..N_BUTTONS).find(|b| BtDlg::button(*b).contains(x, y)) {
                    self.pressed = Some(b);
                    self.focus = b + 1;
                } else if LIST.contains(x, y) {
                    let i = self.top + ((y - LIST.y - 2).max(0) / ROW_H) as usize;
                    self.focus = 0;
                    if i < self.devs.len() {
                        let again = i == self.sel;
                        self.sel = i;
                        if again {
                            // A second click on the same device does Pair.
                            return self.press(0);
                        }
                    }
                }
            }
            Event::Scroll { steps } => {
                let max = self.devs.len().saturating_sub(BtDlg::rows());
                self.top =
                    (self.top as isize + steps.round() as isize).clamp(0, max as isize) as usize;
            }
            Event::Key { sym, .. } => {
                let nb = N_BUTTONS;
                match sym {
                    ESCAPE => return Out::Close,
                    TAB => self.focus = (self.focus + 1) % (nb + 1),
                    ISO_LEFT_TAB => self.focus = (self.focus + nb) % (nb + 1),
                    UP if !self.devs.is_empty() => {
                        self.focus = 0;
                        self.sel = self.sel.saturating_sub(1);
                        self.reveal();
                    }
                    DOWN if !self.devs.is_empty() => {
                        self.focus = 0;
                        self.sel = (self.sel + 1).min(self.devs.len() - 1);
                        self.reveal();
                    }
                    LEFT if self.focus > 1 => self.focus -= 1,
                    RIGHT if self.focus >= 1 && self.focus < nb => self.focus += 1,
                    RETURN | KP_ENTER | SPACE => {
                        let b = self.focus.saturating_sub(1);
                        return self.press(b);
                    }
                    DELETE => return self.press(1),
                    _ => {}
                }
            }
            _ => {}
        }
        let _ = ui;
        Out::None
    }
}

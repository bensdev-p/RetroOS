//! Volume — the Desktop's mixer window (DESIGN.md §9.2 "Volume desk
//! accessory", D-053): MASTER (the output in use) and every program
//! playing, each a vertical slider with its level and a Mute box, and the
//! output's name at the top (a click switches to the next output). The
//! same channels as MIXER, through retro_sys::sound. Refreshed every
//! second; Esc or the close box end it.

use retro_gfx::font::lantern_12;
use retro_gfx::widgets::{bevel, ellipsize, text_width};
use retro_gfx::{Canvas, Theme};
use retro_sys::sound::{self, Channel, Mixer};
use retro_ui::controls::check_box;
use retro_ui::keys::*;
use retro_ui::{station, Event, Program, Rect, SurfaceId, Ui};
use std::time::Duration;

const TITLE: &str = "Volume";
const COLS: usize = 5;
const COL_W: i32 = 72;
const TOP: i32 = 34;
const TRACK_H: i32 = 140;
const W: i32 = 16 + COLS as i32 * COL_W;
const H: i32 = TOP + 18 + TRACK_H + 48;
const REFRESH: u32 = 1;
const LEFT_BUTTON: u32 = 0x110;

struct Vol {
    win: Option<SurfaceId>,
    mixer: Result<Mixer, String>,
    sel: usize,
    /// The channel being dragged.
    drag: Option<usize>,
}

fn track(i: usize) -> Rect {
    Rect::new(
        8 + i as i32 * COL_W + (COL_W - 16) / 2,
        TOP + 18,
        16,
        TRACK_H,
    )
}

fn mute_box(i: usize) -> Rect {
    Rect::new(8 + i as i32 * COL_W + 12, TOP + 18 + TRACK_H + 22, 13, 13)
}

const OUTPUT: Rect = Rect {
    x: 8,
    y: 8,
    w: W - 16,
    h: 18,
};

impl Vol {
    fn channels(&self) -> Vec<(String, Channel)> {
        let Ok(m) = &self.mixer else {
            return Vec::new();
        };
        let mut v = Vec::new();
        if let Some(o) = m.output() {
            v.push(("MASTER".into(), o.clone()));
        }
        v.extend(m.streams.iter().map(|s| (s.name.clone(), s.clone())));
        v.truncate(COLS);
        v
    }

    fn refresh(&mut self) {
        if self.drag.is_none() {
            self.mixer = sound::mixer();
            let n = self.channels().len();
            self.sel = self.sel.min(n.saturating_sub(1));
        }
    }

    fn set_local(&mut self, i: usize, f: impl FnOnce(&mut Channel)) {
        let id = self.channels().get(i).map(|(_, c)| c.id);
        let Ok(m) = &mut self.mixer else { return };
        if let Some(c) = m
            .outputs
            .iter_mut()
            .chain(m.streams.iter_mut())
            .find(|c| Some(c.id) == id)
        {
            f(c);
        }
    }

    fn set_from_y(&mut self, i: usize, y: i32) {
        let r = track(i);
        let v = ((r.y + r.h - 1 - y) as f32 / (r.h - 2) as f32).clamp(0.0, 1.0);
        self.set_local(i, |c| c.volume = v);
    }

    fn commit(&mut self, i: usize) {
        if let Some((_, c)) = self.channels().get(i) {
            let _ = sound::set_volume(c.id, c.volume);
        }
    }

    fn step(&mut self, d: f32) {
        let i = self.sel;
        self.set_local(i, |c| c.volume = (c.volume + d).clamp(0.0, 1.0));
        self.commit(i);
    }

    fn toggle_mute(&mut self, i: usize) {
        if let Some((_, c)) = self.channels().get(i) {
            let (id, m) = (c.id, !c.muted);
            if sound::set_mute(id, m).is_ok() {
                self.set_local(i, |c| c.muted = m);
            }
        }
    }

    fn next_output(&mut self) {
        let Ok(m) = &self.mixer else { return };
        if m.outputs.len() < 2 {
            return;
        }
        let i = m.outputs.iter().position(|o| o.default).unwrap_or(0);
        let id = m.outputs[(i + 1) % m.outputs.len()].id;
        if sound::set_default(id).is_ok() {
            self.mixer = sound::mixer();
        }
    }

    fn draw_window(&self, c: &mut Canvas, t: &Theme) {
        let mut sf = c.surface();
        sf.clear(t.face);
        let font = lantern_12();
        let m = match &self.mixer {
            Ok(m) => m,
            Err(e) => {
                sf.text(font, 12, 14, 1, t.text, None, &ellipsize(e, W - 24));
                sf.text(
                    font,
                    12,
                    32,
                    1,
                    t.disabled,
                    None,
                    "Sound starts with the first program that plays it.",
                );
                return;
            }
        };
        let out = m
            .output()
            .map(|o| o.name.clone())
            .unwrap_or_else(|| "(no output)".into());
        let label = if m.outputs.len() > 1 {
            format!("Output: {out}  (click to change)")
        } else {
            format!("Output: {out}")
        };
        sf.text(
            font,
            OUTPUT.x + 4,
            OUTPUT.y + 2,
            1,
            t.text,
            None,
            &ellipsize(&label, OUTPUT.w - 8),
        );
        for (i, (name, ch)) in self.channels().iter().enumerate() {
            let x0 = 8 + i as i32 * COL_W;
            let name = ellipsize(name, COL_W - 6);
            let fg = if i == self.sel {
                let tw = text_width(&name);
                sf.fill_rect(
                    x0 + (COL_W - tw) / 2 - 2,
                    TOP - 1,
                    (tw + 4) as u32,
                    14,
                    t.sel,
                );
                t.sel_text
            } else {
                t.text
            };
            sf.text(
                font,
                x0 + (COL_W - text_width(&name)) / 2,
                TOP,
                1,
                fg,
                None,
                &name,
            );
            let r = track(i);
            sf.fill_rect(r.x + 5, r.y, 6, r.h as u32, t.well);
            sf.frame(r.x + 5, r.y, 6, r.h as u32, 1, t.line);
            let pct = ch.percent().min(100) as i32;
            let fill = (r.h - 2) * pct / 100;
            if fill > 0 {
                let col = if ch.muted { t.disabled } else { t.active };
                sf.fill_rect(r.x + 6, r.y + r.h - 1 - fill, 4, fill as u32, col);
            }
            let ty = (r.y + r.h - 1 - fill - 4).clamp(r.y - 4, r.y + r.h - 5);
            bevel(&mut sf, r.x, ty, r.w, 9, 1, false, t);
            sf.frame(r.x, ty, r.w as u32, 9, 1, t.line);
            let p = format!("{pct} %");
            sf.text(
                font,
                x0 + (COL_W - text_width(&p)) / 2,
                r.y + r.h + 4,
                1,
                t.text,
                None,
                &p,
            );
            let mb = mute_box(i);
            check_box(&mut sf, mb.x, mb.y, "Mute", ch.muted, t);
        }
    }
}

impl Program for Vol {
    fn start(&mut self, ui: &mut Ui) {
        if let Some(w) = station::windows().iter().find(|w| w.title == TITLE) {
            let _ = station::request(&["SWITCH", &w.id.to_string()]);
            ui.quit();
            return;
        }
        self.win = Some(ui.window(TITLE, "retroos.volume", (W, H), true));
        ui.timer(REFRESH, Duration::from_secs(1));
    }

    fn draw(&mut self, ui: &Ui, id: SurfaceId, c: &mut Canvas) {
        if Some(id) == self.win {
            self.draw_window(c, &ui.theme);
        }
    }

    fn event(&mut self, ui: &mut Ui, id: SurfaceId, ev: Event) {
        let n = self.channels().len();
        match ev {
            Event::Close | Event::Key { sym: ESCAPE, .. } => return ui.quit(),
            Event::Button {
                button: LEFT_BUTTON,
                pressed: true,
                x,
                y,
            } => {
                if OUTPUT.contains(x, y) {
                    self.next_output();
                } else if let Some(i) = (0..n).find(|i| {
                    let r = track(*i);
                    Rect::new(r.x - 8, r.y - 6, r.w + 16, r.h + 12).contains(x, y)
                }) {
                    self.sel = i;
                    self.drag = Some(i);
                    self.set_from_y(i, y);
                } else if let Some(i) = (0..n).find(|i| {
                    let b = mute_box(*i);
                    Rect::new(b.x, b.y, 60, b.h).contains(x, y)
                }) {
                    self.sel = i;
                    self.toggle_mute(i);
                }
            }
            Event::Button {
                button: LEFT_BUTTON,
                pressed: false,
                y,
                ..
            } => {
                if let Some(i) = self.drag.take() {
                    self.set_from_y(i, y);
                    self.commit(i);
                }
            }
            Event::PointerMove { y, .. } => {
                let Some(i) = self.drag else { return };
                self.set_from_y(i, y);
            }
            Event::Scroll { steps } => self.step(-steps as f32 * 0.05),
            Event::Key { sym, text, .. } => match sym {
                LEFT => self.sel = self.sel.saturating_sub(1),
                RIGHT => self.sel = (self.sel + 1).min(n.saturating_sub(1)),
                UP => self.step(0.05),
                DOWN => self.step(-0.05),
                PAGE_UP => self.step(0.2),
                PAGE_DOWN => self.step(-0.2),
                _ if text.as_deref().is_some_and(|t| t.eq_ignore_ascii_case("m")) => {
                    let i = self.sel;
                    self.toggle_mute(i);
                }
                _ if text.as_deref().is_some_and(|t| t.eq_ignore_ascii_case("o")) => {
                    self.next_output()
                }
                _ => return,
            },
            _ => return,
        }
        ui.redraw(id);
    }

    fn timer(&mut self, ui: &mut Ui, token: u32) {
        if token == REFRESH {
            let before = self.mixer.clone();
            self.refresh();
            if self.mixer != before {
                if let Some(w) = self.win {
                    ui.redraw(w);
                }
            }
            ui.timer(REFRESH, Duration::from_secs(1));
        }
    }
}

fn main() {
    let v = Vol {
        win: None,
        mixer: sound::mixer(),
        sel: 0,
        drag: None,
    };
    if let Err(e) = retro_ui::run(Box::new(v)) {
        eprintln!("volume: {e}");
        std::process::exit(1);
    }
}

//! A form behind an action button (Control Panel → Users) as a Desktop
//! dialog: the intro lines, one text field per form field, the confirming
//! button and Cancel. The work runs from a timer after "Please wait..." has
//! been drawn (sudo takes a moment, and longer after a wrong password).

use retro_control::{forms, Form};
use retro_gfx::font::lantern_12;
use retro_gfx::widgets::{ellipsize_in, push_button};
use retro_gfx::{Canvas, Theme};
use retro_ui::controls::{TextField, FIELD_H};
use retro_ui::keys::*;
use retro_ui::{Event, Rect, SurfaceId, Ui};
use std::time::Duration;

const W: i32 = 360;
const FIELD_X: i32 = 130;
const BTN_W: i32 = 88;
const BTN_H: i32 = 24;
const LEFT_BUTTON: u32 = 0x110;
pub const TIMER_RUN: u32 = 30;

pub struct FormDlg {
    pub surface: SurfaceId,
    pub form: Form,
    inputs: Vec<TextField>,
    /// The fields, then the confirming button, then Cancel.
    focus: usize,
    pressed: Option<usize>,
    busy: bool,
    h: i32,
}

/// What the Control Panel does after an event.
pub enum Out {
    None,
    Close,
    /// Show this message over the form; the form stays.
    Message(String),
    /// It worked: close the form, reload the page choosing this user, say so.
    Done {
        select: String,
        message: String,
    },
}

impl FormDlg {
    pub fn new(ui: &mut Ui, parent: SurfaceId, form: Form) -> FormDlg {
        let inputs = form
            .fields
            .iter()
            .map(|f| TextField {
                secret: f.secret,
                ..TextField::default()
            })
            .collect::<Vec<_>>();
        let h = FormDlg::top(&form) + inputs.len() as i32 * 28 + 12 + BTN_H + 12;
        let surface = ui.dialog(parent, &form.title, (W, h));
        FormDlg {
            surface,
            form,
            inputs,
            focus: 0,
            pressed: None,
            busy: false,
            h,
        }
    }

    fn top(form: &Form) -> i32 {
        14 + form.intro.len() as i32 * 16 + 10
    }

    fn row_y(&self, i: usize) -> i32 {
        FormDlg::top(&self.form) + i as i32 * 28
    }

    fn n(&self) -> usize {
        self.inputs.len()
    }

    fn button(&self, b: usize) -> Rect {
        Rect::new(
            W - 2 * (BTN_W + 10) - 4 + b as i32 * (BTN_W + 10),
            self.h - BTN_H - 12,
            BTN_W,
            BTN_H,
        )
    }

    pub fn draw(&self, c: &mut Canvas, t: &Theme) {
        let mut sf = c.surface();
        sf.clear(t.face);
        let font = lantern_12();
        for (i, l) in self.form.intro.iter().enumerate() {
            let s = ellipsize_in(font, l, W - 28);
            sf.text(font, 14, 14 + i as i32 * 16, 1, t.text, None, &s);
        }
        for (i, (f, input)) in self.form.fields.iter().zip(&self.inputs).enumerate() {
            let y = self.row_y(i);
            sf.text(font, 14, y + 4, 1, t.text, None, &format!("{}:", f.label));
            input.draw(&mut sf, FIELD_X, y, W - FIELD_X - 14, self.focus == i, t);
        }
        if self.busy {
            let b = self.button(0);
            sf.text(font, 14, b.y + 6, 1, t.text, None, "Please wait...");
        }
        for (b, label) in [self.form.button, "Cancel"].iter().enumerate() {
            let r = self.button(b);
            let default = self.focus == self.n() + b || (b == 0 && self.focus < self.n());
            push_button(
                &mut sf,
                r.x,
                r.y,
                r.w,
                r.h,
                1,
                label,
                default,
                self.pressed == Some(b),
                t,
            );
        }
    }

    /// Check, then run from the timer.
    fn submit(&mut self, ui: &mut Ui) -> Out {
        self.copy_back();
        if let Err(e) = forms::check(&self.form) {
            return Out::Message(e);
        }
        self.busy = true;
        ui.redraw(self.surface);
        ui.timer(TIMER_RUN, Duration::from_millis(40));
        Out::None
    }

    fn copy_back(&mut self) {
        for (f, i) in self.form.fields.iter_mut().zip(&self.inputs) {
            f.value = i.value();
        }
    }

    pub fn timer(&mut self, ui: &mut Ui, token: u32) -> Out {
        if token != TIMER_RUN || !self.busy {
            return Out::None;
        }
        self.busy = false;
        ui.redraw(self.surface);
        match forms::run(&self.form) {
            Ok(message) => {
                let select = if self.form.target.is_empty() {
                    self.form.value("name").trim().to_string()
                } else {
                    self.form.target.clone()
                };
                Out::Done { select, message }
            }
            Err(e) => {
                // The administrator's password is asked again.
                if let Some(i) = self.form.fields.iter().position(|f| f.key == "auth") {
                    self.inputs[i].set("");
                    self.focus = i;
                }
                Out::Message(e)
            }
        }
    }

    pub fn event(&mut self, ui: &mut Ui, ev: Event) -> Out {
        if self.busy {
            return Out::None;
        }
        let n = self.n();
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
                        .filter(|b| self.button(*b).contains(x, y));
                    ui.redraw(self.surface);
                    return match hit {
                        Some(0) => self.submit(ui),
                        Some(_) => Out::Close,
                        None => Out::None,
                    };
                }
                if let Some(b) = (0..2).find(|b| self.button(*b).contains(x, y)) {
                    self.pressed = Some(b);
                } else if let Some(i) =
                    (0..n).find(|i| y >= self.row_y(*i) && y < self.row_y(*i) + FIELD_H)
                {
                    self.focus = i;
                    self.inputs[i].click(x - FIELD_X);
                }
            }
            Event::Key {
                sym,
                text,
                ctrl,
                shift,
                ..
            } => match sym {
                ESCAPE => return Out::Close,
                TAB | DOWN => self.focus = (self.focus + 1) % (n + 2),
                ISO_LEFT_TAB | UP => self.focus = (self.focus + n + 1) % (n + 2),
                RETURN | KP_ENTER | SPACE if self.focus == n + 1 => return Out::Close,
                SPACE if self.focus == n => return self.submit(ui),
                RETURN | KP_ENTER => return self.submit(ui),
                LEFT if self.focus == n + 1 => self.focus = n,
                RIGHT if self.focus == n => self.focus = n + 1,
                _ if self.focus < n => {
                    self.inputs[self.focus].key(sym, text.as_deref(), ctrl, shift);
                }
                _ => {}
            },
            _ => {}
        }
        ui.redraw(self.surface);
        Out::None
    }
}

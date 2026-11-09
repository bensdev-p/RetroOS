//! The System Bar's volume item (DESIGN.md §9.1, D-053): a speaker left of
//! the clock, crossed out when the sound is muted. A click opens a small
//! popup with a vertical slider, Mute and "Mixer..." (the Volume window);
//! the wheel over the speaker changes the volume without opening it.
//!
//! The bar only asks PipeWire for the volume while PipeWire runs (some
//! program has played sound): asking would start it, and the Desktop at
//! rest should not carry a sound server. Opening the popup does ask.

use retro_gfx::font::lantern_12;
use retro_gfx::widgets::{bevel, push_button, text_width};
use retro_gfx::{Canvas, Rgb, Surface, Theme};
use retro_sys::sound::{self, Channel};
use retro_ui::{Rect, SurfaceId};

/// The speaker's size on the bar, art pixels.
pub const ICON_W: i32 = 14;
pub const POP_W: i32 = 92;
pub const POP_H: i32 = 196;
const TRACK: Rect = Rect {
    x: 38,
    y: 30,
    w: 16,
    h: 104,
};
const MUTE: Rect = Rect {
    x: 14,
    y: 150,
    w: 64,
    h: 14,
};
const MIXER: Rect = Rect {
    x: 10,
    y: 168,
    w: 72,
    h: 22,
};

/// Is a PipeWire daemon running (for this user)? Looks for its process.
pub fn pipewire_running() -> bool {
    if std::env::var_os("RETROOS_TEST_SOUND").is_some() {
        return true;
    }
    let me = unsafe { libc::getuid() };
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| {
            let p = e.path();
            std::fs::read_to_string(p.join("comm")).is_ok_and(|c| c.trim() == "pipewire")
                && std::fs::metadata(&p).is_ok_and(|m| {
                    use std::os::unix::fs::MetadataExt;
                    m.uid() == me
                })
        })
}

/// The output in use, or None (no sound service, no output).
pub fn read() -> Option<Channel> {
    sound::mixer().ok()?.output().cloned()
}

/// The speaker: a box, a cone, and waves (or a cross when muted / unknown
/// loudness shows as no waves).
pub fn draw_icon(sf: &mut Surface, x: i32, y: i32, vol: Option<&Channel>, c: Rgb, bg: Rgb) {
    sf.fill_rect(x, y + 4, 3, 5, c);
    for i in 0..4 {
        sf.fill_rect(x + 3 + i, y + 3 - i, 1, (7 + 2 * i) as u32, c);
    }
    match vol {
        Some(v) if v.muted || v.percent() == 0 => {
            for i in 0..5 {
                sf.put(x + 9 + i, y + 4 + i, c);
                sf.put(x + 13 - i, y + 4 + i, c);
            }
        }
        Some(v) => {
            let waves = 1 + (v.percent() as i32 * 2 / 101);
            for w in 0..waves {
                let wx = x + 9 + w * 2;
                sf.fill_rect(wx, y + 3 - w, 1, (7 + 2 * w) as u32, c);
                sf.put(wx, y + 3 - w, bg);
                sf.put(wx, y + 9 + w, bg);
            }
        }
        None => {}
    }
}

pub struct Popup {
    pub surface: SurfaceId,
    pub vol: Option<Channel>,
    /// The slider is being dragged.
    pub dragging: bool,
    pub pressed_mixer: bool,
    pub error: Option<String>,
}

/// What a popup event asks the bar to do.
pub enum Out {
    None,
    Close,
    Mixer,
}

impl Popup {
    pub fn new(surface: SurfaceId) -> Popup {
        let (vol, error) = match sound::mixer() {
            Ok(m) => match m.output() {
                Some(o) => (Some(o.clone()), None),
                None => (None, Some("No sound output".to_string())),
            },
            Err(e) => (None, Some(e)),
        };
        Popup {
            surface,
            vol,
            dragging: false,
            pressed_mixer: false,
            error,
        }
    }

    pub fn draw(&self, c: &mut Canvas, t: &Theme) {
        let (w, h) = (c.width as i32, c.height as i32);
        let mut sf = c.surface();
        sf.fill_rect(0, 0, w as u32, h as u32, t.face);
        sf.frame(0, 0, w as u32, h as u32, 1, t.line);
        let font = lantern_12();
        let title = "Volume";
        sf.text(font, (w - text_width(title)) / 2, 8, 1, t.text, None, title);
        let r = TRACK;
        sf.fill_rect(r.x + 5, r.y, 6, r.h as u32, t.well);
        sf.frame(r.x + 5, r.y, 6, r.h as u32, 1, t.line);
        match &self.vol {
            Some(v) => {
                let pct = v.percent().min(100) as i32;
                let fill = (r.h - 2) * pct / 100;
                if fill > 0 {
                    sf.fill_rect(r.x + 6, r.y + r.h - 1 - fill, 4, fill as u32, t.active);
                }
                let ty = r.y + r.h - 1 - fill - 4;
                bevel(
                    &mut sf,
                    r.x,
                    ty.clamp(r.y - 4, r.y + r.h - 5),
                    r.w,
                    9,
                    1,
                    false,
                    t,
                );
                sf.frame(
                    r.x,
                    ty.clamp(r.y - 4, r.y + r.h - 5),
                    r.w as u32,
                    9,
                    1,
                    t.line,
                );
                let label = format!("{pct} %");
                sf.text(
                    font,
                    (w - text_width(&label)) / 2,
                    r.y + r.h + 4,
                    1,
                    t.text,
                    None,
                    &label,
                );
                retro_ui::controls::check_box(&mut sf, MUTE.x, MUTE.y, "Mute", v.muted, t);
            }
            None => {
                let msg = self.error.as_deref().unwrap_or("No sound");
                for (i, line) in wrap(msg, w - 12).iter().enumerate() {
                    sf.text(font, 6, r.y + 10 + i as i32 * 14, 1, t.disabled, None, line);
                }
            }
        }
        push_button(
            &mut sf,
            MIXER.x,
            MIXER.y,
            MIXER.w,
            MIXER.h,
            1,
            "Mixer...",
            false,
            self.pressed_mixer,
            t,
        );
    }

    fn set_from_y(&mut self, y: i32) {
        let r = TRACK;
        if let Some(v) = self.vol.as_mut() {
            let pct = ((r.y + r.h - 1 - y) * 100 / (r.h - 2)).clamp(0, 100);
            v.volume = pct as f32 / 100.0;
        }
    }

    fn commit(&mut self) {
        if let Some(v) = &self.vol {
            if let Err(e) = sound::set_volume(v.id, v.volume) {
                self.error = Some(e);
            }
        }
    }

    pub fn step(&mut self, d: f32) {
        if let Some(v) = self.vol.as_mut() {
            v.volume = (v.volume + d).clamp(0.0, 1.0);
        }
        self.commit();
    }

    pub fn toggle_mute(&mut self) {
        if let Some(v) = self.vol.as_mut() {
            v.muted = !v.muted;
            if let Err(e) = sound::set_mute(v.id, v.muted) {
                self.error = Some(e);
            }
        }
    }

    pub fn button(&mut self, pressed: bool, x: i32, y: i32) -> Out {
        if pressed {
            if TRACK.contains(x, y)
                || Rect::new(TRACK.x - 6, TRACK.y - 6, TRACK.w + 12, TRACK.h + 12).contains(x, y)
            {
                self.dragging = true;
                self.set_from_y(y);
            } else if MUTE.contains(x, y) {
                self.toggle_mute();
            } else if MIXER.contains(x, y) {
                self.pressed_mixer = true;
            }
            return Out::None;
        }
        if self.dragging {
            self.dragging = false;
            self.set_from_y(y);
            self.commit();
        }
        if std::mem::take(&mut self.pressed_mixer) && MIXER.contains(x, y) {
            return Out::Mixer;
        }
        Out::None
    }

    pub fn motion(&mut self, y: i32) -> bool {
        if self.dragging {
            self.set_from_y(y);
            return true;
        }
        false
    }

    pub fn key(&mut self, sym: u32, text: Option<&str>) -> Out {
        use retro_ui::keys::*;
        match sym {
            UP | RIGHT => self.step(0.05),
            DOWN | LEFT => self.step(-0.05),
            PAGE_UP => self.step(0.2),
            PAGE_DOWN => self.step(-0.2),
            ESCAPE | RETURN | KP_ENTER => return Out::Close,
            _ if text.is_some_and(|t| t.eq_ignore_ascii_case("m")) => self.toggle_mute(),
            _ => {}
        }
        Out::None
    }
}

fn wrap(s: &str, w: i32) -> Vec<String> {
    let mut lines = vec![String::new()];
    for word in s.split_whitespace() {
        let cur = lines.last_mut().unwrap();
        let next = if cur.is_empty() {
            word.to_string()
        } else {
            format!("{cur} {word}")
        };
        if text_width(&next) > w && !cur.is_empty() {
            lines.push(word.to_string());
        } else {
            *cur = next;
        }
    }
    lines
}

//! The mixer screen: one vertical bar per channel, in the manner of a
//! Sound Blaster mixer — MASTER (the output in use) first, then every
//! program playing. ←→ choose a channel, ↑↓ change it, M mutes, O picks the
//! output.

use retro_sys::sound::{Channel, Mixer};
use retro_tui::{Key, Screen, BLACK, COBALT, CORAL, LEMON, LIME, SILVER, SLATE, TEAL, WHITE};

const COL_W: u16 = 13;

/// A change for the main loop to make (then it re-reads the state).
#[derive(Debug, PartialEq)]
pub enum Change {
    Volume(u32, f32),
    Mute(u32, bool),
    Output(u32),
}

pub struct App {
    pub mixer: Result<Mixer, String>,
    pub sel: usize,
    pub quit: bool,
    /// The last error from a change.
    pub error: Option<String>,
}

fn clip(s: &str, w: usize) -> String {
    s.chars().take(w).collect()
}

impl App {
    pub fn new(mixer: Result<Mixer, String>) -> App {
        App {
            mixer,
            sel: 0,
            quit: false,
            error: None,
        }
    }

    /// MASTER, then the programs.
    pub fn channels(&self) -> Vec<(String, Channel)> {
        let Ok(m) = &self.mixer else {
            return Vec::new();
        };
        let mut v = Vec::new();
        if let Some(o) = m.output() {
            v.push(("MASTER".to_string(), o.clone()));
        }
        for s in &m.streams {
            v.push((s.name.clone(), s.clone()));
        }
        v
    }

    pub fn refresh(&mut self, mixer: Result<Mixer, String>) {
        self.mixer = mixer;
        let n = self.channels().len();
        self.sel = self.sel.min(n.saturating_sub(1));
    }

    pub fn draw(&self, s: &mut Screen) {
        s.clear(SILVER, COBALT);
        s.cursor = None;
        s.fill(0, 0, s.cols, 1, ' ', BLACK, TEAL);
        s.text(1, 0, "RetroOS Mixer", BLACK, TEAL);
        let y = s.rows - 1;
        s.fill(0, y, s.cols, 1, ' ', BLACK, TEAL);
        s.text(
            1,
            y,
            "\u{2190}\u{2192} Channel  \u{2191}\u{2193} Volume  M Mute  O Output  Esc Close",
            BLACK,
            TEAL,
        );
        let w = s.cols.saturating_sub(4).min(96);
        let h = s.rows.saturating_sub(4);
        let x = (s.cols - w) / 2;
        s.dialog(x, 1, w, h, "Mixer", WHITE, COBALT);
        let inner = (w - 6) as usize;
        let m = match &self.mixer {
            Ok(m) => m,
            Err(e) => {
                s.text(x + 3, 3, &clip(e, inner), LEMON, COBALT);
                s.text(
                    x + 3,
                    5,
                    "Sound starts with the first program that plays it.",
                    SILVER,
                    COBALT,
                );
                return;
            }
        };
        let out = m
            .output()
            .map(|o| o.name.clone())
            .unwrap_or_else(|| "(none)".into());
        let arrows = if m.outputs.len() > 1 {
            format!("\u{25c4} {out} \u{25ba}  (O)")
        } else {
            out
        };
        s.text(x + 3, 3, "Output:", LEMON, COBALT);
        s.text(x + 11, 3, &clip(&arrows, inner - 8), WHITE, COBALT);
        if let Some(e) = &self.error {
            s.text(x + 3, 4, &clip(e, inner), CORAL, COBALT);
        }
        let chans = self.channels();
        if chans.is_empty() {
            s.text(x + 3, 6, "No sound outputs were found.", SILVER, COBALT);
            return;
        }
        // Bars from row 7 to the label rows at the bottom.
        let top = 7u16;
        let bottom = h.saturating_sub(4).max(top + 4); // last bar row
        let rows = (bottom - top + 1) as u32;
        let per_page = ((w - 4) / COL_W).max(1) as usize;
        let first = self.sel.saturating_sub(per_page - 1);
        for (col, (i, (label, c))) in chans
            .iter()
            .enumerate()
            .skip(first)
            .take(per_page)
            .enumerate()
        {
            let cx = x + 3 + col as u16 * COL_W;
            let on = i == self.sel;
            let filled = ((c.volume.clamp(0.0, 1.0) * rows as f32).round()) as u32;
            for r in 0..rows {
                let yy = bottom - r as u16;
                let level = (r + 1) * 100 / rows;
                let (ch, fg) = if r < filled {
                    let fg = if c.muted {
                        SLATE
                    } else if level > 85 {
                        CORAL
                    } else if level > 60 {
                        LEMON
                    } else {
                        LIME
                    };
                    ('\u{2588}', fg)
                } else {
                    ('\u{2591}', SLATE)
                };
                s.fill(cx + 4, yy, 3, 1, ch, fg, COBALT);
            }
            let (fg, bg) = if on { (BLACK, SILVER) } else { (WHITE, COBALT) };
            let name = clip(label, (COL_W - 2) as usize);
            s.fill(cx, bottom + 1, COL_W - 2, 1, ' ', fg, bg);
            let pad = (COL_W - 2).saturating_sub(name.chars().count() as u16) / 2;
            s.text(cx + pad, bottom + 1, &name, fg, bg);
            let pct = format!("{:>3} %", c.percent());
            s.text(cx + 3, bottom + 2, &pct, WHITE, COBALT);
            let mute = if c.muted { "[MUTE]" } else { "      " };
            s.text(cx + 2, bottom + 3, mute, CORAL, COBALT);
        }
    }

    pub fn key(&mut self, k: Key) -> Option<Change> {
        let chans = self.channels();
        let n = chans.len();
        let cur = chans.get(self.sel).map(|(_, c)| c.clone());
        let step = |c: &Channel, d: f32| Change::Volume(c.id, (c.volume + d).clamp(0.0, 1.0));
        match k {
            Key::Esc | Key::F(10) | Key::Char('q') | Key::Char('Q') => self.quit = true,
            Key::Left | Key::BackTab => self.sel = self.sel.saturating_sub(1),
            Key::Right | Key::Tab => self.sel = (self.sel + 1).min(n.saturating_sub(1)),
            Key::Up => return cur.map(|c| step(&c, 0.05)),
            Key::Down => return cur.map(|c| step(&c, -0.05)),
            Key::PgUp => return cur.map(|c| step(&c, 0.20)),
            Key::PgDn => return cur.map(|c| step(&c, -0.20)),
            Key::Char('m') | Key::Char('M') | Key::Char(' ') => {
                return cur.map(|c| Change::Mute(c.id, !c.muted))
            }
            Key::Char('o') | Key::Char('O') => {
                let Ok(m) = &self.mixer else { return None };
                if m.outputs.len() > 1 {
                    let i = m.outputs.iter().position(|o| o.default).unwrap_or(0);
                    return Some(Change::Output(m.outputs[(i + 1) % m.outputs.len()].id));
                }
            }
            _ => {}
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mixer() -> Mixer {
        let c = |id, name: &str, volume, default| Channel {
            id,
            name: name.into(),
            volume,
            muted: false,
            default,
        };
        Mixer {
            outputs: vec![c(45, "HDMI", 0.7, true), c(52, "USB", 0.4, false)],
            streams: vec![c(80, "Firefox", 1.0, false)],
        }
    }

    #[test]
    fn master_first_then_programs() {
        let mut a = App::new(Ok(mixer()));
        let mut s = Screen::with_size(80, 25, None);
        a.draw(&mut s);
        let t = s.dump();
        assert!(
            t.contains("MASTER") && t.contains("Firefox") && t.contains(" 70 %"),
            "{t}"
        );
        assert!(t.contains("\u{25c4} HDMI \u{25ba}"));
        assert_eq!(a.key(Key::Up), Some(Change::Volume(45, 0.75)));
        a.key(Key::Right);
        assert_eq!(a.key(Key::Char('m')), Some(Change::Mute(80, true)));
        assert_eq!(a.key(Key::Char('o')), Some(Change::Output(52)));
        a.key(Key::Esc);
        assert!(a.quit);
    }

    #[test]
    fn without_a_sound_server() {
        let a = App::new(Err("The sound service is not running.".into()));
        let mut s = Screen::with_size(80, 25, None);
        a.draw(&mut s);
        assert!(s.dump().contains("not running"));
    }
}

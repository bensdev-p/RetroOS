//! The BT screen: the devices Bluetooth can see, paired ones first. Enter
//! pairs with the chosen one (or connects / disconnects it), R removes a
//! pairing, S searches again, O turns Bluetooth off. While a keyboard pairs,
//! a box shows the number to type on it; a phone or computer asks whether
//! the numbers on both screens match.

use retro_sys::bt::{Absent, BtApi, Device, Job, Prompt};
use retro_tui::{Key, Screen, BLACK, COBALT, CORAL, LEMON, LIME, SILVER, TEAL, WHITE};

/// Something slow for the main loop to do after drawing "please wait".
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    TurnOn,
    TurnOff,
}

pub struct App {
    pub api: Result<Box<dyn BtApi>, Absent>,
    pub devs: Vec<Device>,
    pub sel: usize,
    pub quit: bool,
    /// The last outcome or error, under the list.
    pub note: Option<(String, bool)>,
    /// Shown while an [`Action`] runs.
    pub waiting: Option<String>,
}

fn clip(s: &str, w: usize) -> String {
    s.chars().take(w).collect()
}

/// Signal as bars, like NET.
fn bars(rssi: Option<i16>) -> &'static str {
    match rssi {
        Some(d) if d >= -55 => "\u{2582}\u{2584}\u{2586}\u{2588}",
        Some(d) if d >= -67 => "\u{2582}\u{2584}\u{2586} ",
        Some(d) if d >= -80 => "\u{2582}\u{2584}  ",
        Some(_) => "\u{2582}   ",
        None => "",
    }
}

impl App {
    pub fn new(api: Result<Box<dyn BtApi>, Absent>) -> App {
        let mut a = App {
            api,
            devs: Vec::new(),
            sel: 0,
            quit: false,
            note: None,
            waiting: None,
        };
        a.opened();
        a
    }

    /// Bluetooth is there: search at once (pairing is why BT was run).
    fn opened(&mut self) {
        if let Ok(b) = &self.api {
            if b.powered() {
                if let Err(e) = b.discover(true) {
                    self.note = Some((e, true));
                }
            }
        }
        self.refresh();
    }

    pub fn reopen(&mut self, api: Result<Box<dyn BtApi>, Absent>) {
        self.api = api;
        self.opened();
    }

    pub fn refresh(&mut self) {
        let Ok(b) = &self.api else {
            self.devs.clear();
            return;
        };
        if let Some((job, r)) = b.take_result() {
            self.note = Some(match r {
                Ok(()) => (job.done_text(), false),
                Err(e) => (e, true),
            });
        }
        match b.devices() {
            Ok(d) => {
                // Keep the same device chosen when the order changes.
                let cur = self.devs.get(self.sel).map(|d| d.path.clone());
                self.devs = d;
                if let Some(i) = cur.and_then(|p| self.devs.iter().position(|d| d.path == p)) {
                    self.sel = i;
                }
            }
            Err(e) => self.note = Some((e, true)),
        }
        self.sel = self.sel.min(self.devs.len().saturating_sub(1));
    }

    fn name_of(&self, path: &str) -> String {
        self.devs
            .iter()
            .find(|d| d.path == path)
            .map(|d| d.name.clone())
            .unwrap_or_else(|| "the device".into())
    }

    pub fn draw(&self, s: &mut Screen) {
        s.clear(SILVER, COBALT);
        s.cursor = None;
        s.fill(0, 0, s.cols, 1, ' ', BLACK, TEAL);
        s.text(1, 0, "RetroOS Bluetooth", BLACK, TEAL);
        let y = s.rows - 1;
        s.fill(0, y, s.cols, 1, ' ', BLACK, TEAL);
        let help = match &self.api {
            Ok(_) => {
                "\u{2191}\u{2193} Choose  Enter Pair/Connect  R Remove  S Search  O Off  Esc Close"
            }
            Err(Absent::Off) => "O Turn Bluetooth on  Esc Close",
            Err(_) => "Esc Close",
        };
        s.text(1, y, help, BLACK, TEAL);
        let w = s.cols.saturating_sub(4).min(76);
        let h = s.rows.saturating_sub(4);
        let x = (s.cols - w) / 2;
        s.dialog(x, 1, w, h, "Bluetooth", WHITE, COBALT);
        let inner = (w - 6) as usize;
        if let Some(t) = &self.waiting {
            s.text(x + 3, 3, &clip(t, inner), LEMON, COBALT);
            return;
        }
        let b = match &self.api {
            Ok(b) => b,
            Err(a) => {
                s.text(x + 3, 3, &clip(&a.text(), inner), LEMON, COBALT);
                if *a == Absent::Off {
                    s.text(
                        x + 3,
                        5,
                        "Press O to turn it on. It stays on until you",
                        SILVER,
                        COBALT,
                    );
                    s.text(
                        x + 3,
                        6,
                        "turn it off; paired devices then reconnect by",
                        SILVER,
                        COBALT,
                    );
                    s.text(x + 3, 7, "themselves.", SILVER, COBALT);
                }
                return;
            }
        };
        let busy = b.busy();
        let head = if let Some(j) = &busy {
            j.busy_text()
        } else if !b.powered() {
            "The adapter is switched off. Press O to switch it on.".into()
        } else if b.discovering() {
            "Searching... Put the device in pairing mode.".into()
        } else {
            "Choose a device and press Enter.".into()
        };
        s.text(x + 3, 3, &clip(&head, inner), WHITE, COBALT);
        s.text(
            x + 3,
            4,
            &clip(&format!("Adapter: {}", b.adapter()), inner),
            SILVER,
            COBALT,
        );
        let top = 6u16;
        let rows = h.saturating_sub(top + 2) as usize;
        let lw = w - 6;
        s.text(x + 3, top - 1, "Device", LEMON, COBALT);
        s.text(x + 3 + 34, top - 1, "Kind", LEMON, COBALT);
        s.text(x + 3 + 45, top - 1, "State", LEMON, COBALT);
        if self.devs.is_empty() {
            s.text(x + 3, top, "No devices found yet.", SILVER, COBALT);
        }
        let first = self.sel.saturating_sub(rows.saturating_sub(1));
        for (row, (i, d)) in self
            .devs
            .iter()
            .enumerate()
            .skip(first)
            .take(rows)
            .enumerate()
        {
            let yy = top + row as u16;
            let (fg, bg) = if i == self.sel {
                (BLACK, SILVER)
            } else {
                (WHITE, COBALT)
            };
            s.fill(x + 3, yy, lw, 1, ' ', fg, bg);
            s.text(x + 3, yy, &clip(&d.name, 32), fg, bg);
            s.text(x + 3 + 34, yy, d.kind.label(), fg, bg);
            let state_fg = if i == self.sel {
                BLACK
            } else if d.connected {
                LIME
            } else {
                WHITE
            };
            s.text(x + 3 + 45, yy, d.state(), state_fg, bg);
            if !d.paired && lw > 62 {
                s.text(x + 3 + 57, yy, bars(d.rssi), fg, bg);
            }
        }
        if let Some((n, bad)) = &self.note {
            let fg = if *bad { CORAL } else { LIME };
            s.text(x + 3, 1 + h - 2, &clip(n, inner), fg, COBALT);
        }
        if let Some(p) = b.prompt() {
            self.draw_prompt(s, &p);
        }
    }

    fn draw_prompt(&self, s: &mut Screen, p: &Prompt) {
        let lines = p.lines(&self.name_of(p.device()));
        let w = 44u16.min(s.cols.saturating_sub(4));
        let h = 9u16;
        let x = (s.cols - w) / 2;
        let y = (s.rows.saturating_sub(h)) / 2;
        let title = match p {
            Prompt::Type { .. } => "Pairing",
            Prompt::Confirm { .. } => "Confirm Pairing",
        };
        s.dialog(x, y, w, h, title, BLACK, SILVER);
        for (i, l) in lines.iter().enumerate() {
            let big = i == 1;
            let t = if big {
                // Spaced out, so it reads easily across the room.
                l.chars()
                    .map(|c| format!("{c} "))
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            } else {
                l.clone()
            };
            let tx = x + (w.saturating_sub(t.chars().count() as u16)) / 2;
            s.text(
                tx,
                y + 2 + i as u16,
                &t,
                if big { COBALT } else { BLACK },
                SILVER,
            );
        }
        let foot = match p {
            Prompt::Type { .. } => "Esc cancels",
            Prompt::Confirm { .. } => "Y Yes    N No    Esc Cancel",
        };
        let fx = x + (w.saturating_sub(foot.len() as u16)) / 2;
        s.text(fx, y + h - 2, foot, BLACK, SILVER);
    }

    fn chosen(&self) -> Option<Device> {
        self.devs.get(self.sel).cloned()
    }

    pub fn key(&mut self, k: Key) -> Option<Action> {
        let b = match &self.api {
            Ok(b) => b,
            Err(a) => {
                match k {
                    Key::Esc | Key::F(10) | Key::Char('q') | Key::Char('Q') => self.quit = true,
                    Key::Char('o') | Key::Char('O') if *a == Absent::Off => {
                        return Some(Action::TurnOn)
                    }
                    _ => {}
                }
                return None;
            }
        };
        if let Some(p) = b.prompt() {
            match (p, k) {
                (Prompt::Confirm { .. }, Key::Char('y' | 'Y')) => b.answer(true),
                (Prompt::Confirm { .. }, Key::Char('n' | 'N')) => b.answer(false),
                // A keyboard pairing ends on the keyboard, times out, or
                // is given up.
                (_, Key::Esc) => b.cancel(),
                _ => {}
            }
            self.refresh();
            return None;
        }
        let busy = b.busy().is_some();
        match k {
            Key::Esc | Key::F(10) | Key::Char('q') | Key::Char('Q') => self.quit = true,
            Key::Up => self.sel = self.sel.saturating_sub(1),
            Key::Down => self.sel = (self.sel + 1).min(self.devs.len().saturating_sub(1)),
            Key::PgUp | Key::Home => self.sel = 0,
            Key::PgDn | Key::End => self.sel = self.devs.len().saturating_sub(1),
            _ if busy => {}
            Key::Enter => {
                if let Some(d) = self.chosen() {
                    self.note = None;
                    b.start(match (d.paired, d.connected) {
                        (_, true) => Job::Disconnect(d),
                        (true, false) => Job::Connect(d),
                        (false, _) => Job::Pair(d),
                    });
                }
            }
            Key::Char('d' | 'D') => {
                if let Some(d) = self.chosen().filter(|d| d.connected) {
                    b.start(Job::Disconnect(d));
                }
            }
            Key::Char('r' | 'R') | Key::Delete => match self.chosen() {
                Some(d) if d.paired => {
                    self.note = None;
                    b.start(Job::Remove(d));
                }
                Some(d) => self.note = Some((format!("{} is not paired.", d.name), true)),
                None => {}
            },
            Key::Char('s' | 'S') => {
                let on = !b.discovering();
                if let Err(e) = b.discover(on) {
                    self.note = Some((e, true));
                }
            }
            Key::Char('o' | 'O') => return Some(Action::TurnOff),
            _ => {}
        }
        self.refresh();
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use retro_sys::bt::fake::Fake;

    #[test]
    fn lists_and_pairs_a_keyboard() {
        let mut a = App::new(Ok(Box::new(Fake::default())));
        let mut s = Screen::with_size(80, 25, None);
        a.draw(&mut s);
        let t = s.dump();
        assert!(
            t.contains("Studio Headphones") && t.contains("Connected"),
            "{t}"
        );
        assert!(t.contains("Searching"), "searches at once: {t}");
        a.key(Key::Down);
        assert_eq!(a.chosen().unwrap().name, "Compact Keyboard");
        a.key(Key::Enter);
        a.draw(&mut s);
        let t = s.dump();
        assert!(t.contains("Pairing with Compact Keyboard"), "{t}");
        assert!(t.contains("3 1 4 1 5 9"), "the number to type: {t}");
        a.key(Key::Esc);
        a.draw(&mut s);
        assert!(s.dump().contains("Pairing was cancelled."));
    }

    #[test]
    fn bluetooth_off_offers_to_turn_it_on() {
        let mut a = App::new(Err(Absent::Off));
        let mut s = Screen::with_size(80, 25, None);
        a.draw(&mut s);
        assert!(s.dump().contains("Bluetooth is off."));
        assert_eq!(a.key(Key::Char('o')), Some(Action::TurnOn));
        a.key(Key::Esc);
        assert!(a.quit);
    }
}

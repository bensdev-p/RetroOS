//! CONTROL: the Control Panel in text mode, drawn with retro-tui. The list
//! of applets on the left of a dialog; Enter opens one as a page of
//! settings (arrows change them, a long list opens a chooser), OK saves.

use crate::{apply, load, Applet, Applied, Field, Page, Value, APPLETS};
use retro_tui::*;

/// What the main loop must do for the screen.
#[derive(Debug, PartialEq)]
pub enum Action {
    /// Leave the screen, run this program, come back.
    Run(&'static str),
    /// Save the page (the loop draws "Saving..." first).
    Apply,
    Quit,
}

/// A long list being chosen from: typing filters it.
pub struct Chooser {
    pub field: usize,
    pub filter: String,
    pub sel: usize,
}

pub struct Open {
    pub page: Page,
    /// The field with the cursor; `fields.len()` = OK, +1 = Cancel.
    pub sel: usize,
    pub chooser: Option<Chooser>,
}

pub struct App {
    pub sel: usize,
    pub open: Option<Open>,
    /// A message box over everything; any key closes it.
    pub message: Option<Vec<String>>,
    pub busy: Option<String>,
    pub quit: bool,
}

const LABEL_W: usize = 22;

fn clip(s: &str, w: usize) -> String {
    s.chars().take(w).collect()
}

impl Chooser {
    /// The list's (index, label) that match the filter.
    fn matches<'a>(&self, items: &'a [(String, String)]) -> Vec<(usize, &'a str)> {
        let f = self.filter.to_lowercase();
        items
            .iter()
            .enumerate()
            .filter(|(_, (v, l))| {
                f.is_empty() || l.to_lowercase().contains(&f) || v.to_lowercase().contains(&f)
            })
            .map(|(i, (_, l))| (i, l.as_str()))
            .collect()
    }
}

impl App {
    pub fn new(start: Option<Applet>) -> App {
        let mut app = App {
            sel: 0,
            open: None,
            message: None,
            busy: None,
            quit: false,
        };
        if let Some(a) = start {
            app.sel = APPLETS.iter().position(|x| *x == a).unwrap_or(0);
            if a.program().is_none() {
                app.open = Some(Open {
                    page: load(a),
                    sel: 0,
                    chooser: None,
                });
            }
        }
        app
    }

    pub fn draw(&self, s: &mut Screen) {
        s.clear(SILVER, COBALT);
        s.cursor = None;
        s.fill(0, 0, s.cols, 1, ' ', BLACK, TEAL);
        s.text(1, 0, "RetroOS Control Panel", BLACK, TEAL);
        let y = s.rows - 1;
        s.fill(0, y, s.cols, 1, ' ', BLACK, TEAL);
        let help = match &self.open {
            None => "Enter Open  \u{2191}\u{2193} Choose  Esc Close",
            Some(o) if o.chooser.is_some() => "Type to search  Enter Choose  Esc Back",
            Some(_) => "\u{2191}\u{2193} Setting  \u{2190}\u{2192} Change  Enter OK  Esc Cancel",
        };
        s.text(1, y, help, BLACK, TEAL);

        // The applets.
        let w = 74.min(s.cols.saturating_sub(4));
        let h = (APPLETS.len() as u16 + 5).min(s.rows.saturating_sub(3));
        let x = (s.cols - w) / 2;
        s.dialog(x, 2, w, h, "Control Panel", WHITE, COBALT);
        for (i, a) in APPLETS.iter().enumerate() {
            let row = 4 + i as u16;
            let on = i == self.sel;
            let (fg, bg) = if on { (BLACK, SILVER) } else { (WHITE, COBALT) };
            s.fill(x + 2, row, 16, 1, ' ', fg, bg);
            s.text(x + 3, row, a.name(), fg, bg);
            s.text(
                x + 20,
                row,
                &clip(a.blurb(), (w - 22) as usize),
                SILVER,
                COBALT,
            );
        }

        if let Some(o) = &self.open {
            self.draw_page(s, o);
        }
        if let Some(b) = &self.busy {
            let w = b.chars().count() as u16 + 8;
            s.dialog((s.cols - w) / 2, s.rows / 2 - 2, w, 3, "", BLACK, SILVER);
            s.center(s.rows / 2 - 1, b, BLACK, SILVER);
        }
        if let Some(m) = &self.message {
            let w = m
                .iter()
                .map(|l| l.chars().count())
                .max()
                .unwrap_or(0)
                .max(20) as u16
                + 6;
            let h = m.len() as u16 + 4;
            let (mx, my) = ((s.cols - w) / 2, (s.rows - h) / 2);
            s.dialog(mx, my, w, h, "Control Panel", BLACK, SILVER);
            for (i, l) in m.iter().enumerate() {
                s.text(mx + 3, my + 1 + i as u16, l, BLACK, SILVER);
            }
            s.center(my + h - 2, "< OK >", WHITE, BLACK);
        }
    }

    fn draw_page(&self, s: &mut Screen, o: &Open) {
        let p = &o.page;
        let extra = if p.notice.is_some() { 2 } else { 0 };
        let w = 66.min(s.cols.saturating_sub(4));
        let h = p.fields.len() as u16 + 6 + extra;
        // Cascaded over the applet list, which stays readable above it.
        let x = (s.cols - w) / 2 + 2;
        let y = (APPLETS.len() as u16 + 4).min(s.rows.saturating_sub(h + 2));
        s.dialog(x, y, w, h, p.applet.name(), WHITE, COBALT);
        let mut row = y + 2;
        if let Some(n) = &p.notice {
            s.text(x + 3, row, &clip(n, (w - 6) as usize), LEMON, COBALT);
            row += 2;
        }
        let vw = (w as usize).saturating_sub(LABEL_W + 10);
        for (i, f) in p.fields.iter().enumerate() {
            let on = o.sel == i;
            let lf = if f.enabled { WHITE } else { SLATE };
            let dots = ".".repeat(LABEL_W.saturating_sub(f.label.chars().count() + 1));
            s.text(x + 3, row, &format!("{} {dots}", f.label), lf, COBALT);
            let (fg, bg) = match (on, f.enabled) {
                (true, _) => (BLACK, SILVER),
                (false, true) => (WHITE, BLACK),
                (false, false) => (SLATE, COBALT),
            };
            let vx = x + 4 + LABEL_W as u16;
            let shown = match &f.value {
                Value::Choice(..) => format!("\u{25c4} {} \u{25ba}", f.shown()),
                Value::List(..) => format!("{} \u{25bc}", f.shown()),
                Value::Check(b) => format!("[{}]", if *b { 'X' } else { ' ' }),
                Value::Level(n) => {
                    let bars = (*n as usize * 20) / 100;
                    format!(
                        "{}{} {n:>3} %",
                        "\u{2588}".repeat(bars),
                        "\u{2591}".repeat(20 - bars)
                    )
                }
                Value::Text(t) => format!("{t:<12}"),
            };
            let end = s.text(vx, row, &clip(&shown, vw), fg, bg);
            if on && matches!(f.value, Value::Text(_)) && f.enabled {
                s.cursor = Some((end.min(vx + 11), row));
            }
            row += 1;
        }
        row += 1;
        let n = p.fields.len();
        let ok = if o.sel == n {
            (WHITE, BLACK)
        } else {
            (BLACK, SILVER)
        };
        let cancel = if o.sel == n + 1 {
            (WHITE, BLACK)
        } else {
            (BLACK, SILVER)
        };
        let bx = x + w / 2 - 12;
        s.text(bx, row, "<  OK  >", ok.0, ok.1);
        s.text(bx + 12, row, "< Cancel >", cancel.0, cancel.1);
        if let Some(c) = &o.chooser {
            if let Value::List(items, cur) = &p.fields[c.field].value {
                self.draw_chooser(s, p.fields[c.field].label, items, *cur, c);
            }
        }
    }

    fn draw_chooser(
        &self,
        s: &mut Screen,
        label: &str,
        items: &[(String, String)],
        cur: usize,
        c: &Chooser,
    ) {
        let w = 50.min(s.cols.saturating_sub(6));
        let h = s.rows.saturating_sub(6).min(20);
        let (x, y) = ((s.cols - w) / 2, 3);
        s.dialog(x, y, w, h, label, BLACK, SILVER);
        s.text(x + 2, y + 1, "Search:", BLACK, SILVER);
        let end = s.text(
            x + 10,
            y + 1,
            &format!("{:<width$}", c.filter, width = (w - 13) as usize),
            WHITE,
            BLACK,
        );
        s.cursor = Some(((x + 10 + c.filter.chars().count() as u16).min(end), y + 1));
        let m = c.matches(items);
        let rows = (h - 4) as usize;
        let top = c.sel.saturating_sub(rows - 1);
        for (r, (i, l)) in m.iter().enumerate().skip(top).take(rows) {
            let on = r == c.sel;
            let (fg, bg) = if on { (WHITE, COBALT) } else { (BLACK, SILVER) };
            let mark = if *i == cur { '\u{221a}' } else { ' ' };
            let line = format!("{mark} {}", clip(l, (w - 7) as usize));
            s.fill(x + 2, y + 3 + (r - top) as u16, w - 4, 1, ' ', fg, bg);
            s.text(x + 2, y + 3 + (r - top) as u16, &line, fg, bg);
        }
        if m.is_empty() {
            s.text(x + 3, y + 3, "Nothing matches.", SLATE, SILVER);
        }
    }

    pub fn key(&mut self, k: Key) -> Option<Action> {
        if self.message.is_some() {
            self.message = None;
            return None;
        }
        let (_, base) = k.split();
        let base = base.clone();
        let Some(o) = self.open.as_mut() else {
            match base {
                Key::Up => self.sel = self.sel.saturating_sub(1),
                Key::Down => self.sel = (self.sel + 1).min(APPLETS.len() - 1),
                Key::Home => self.sel = 0,
                Key::End => self.sel = APPLETS.len() - 1,
                Key::Enter | Key::Char(' ') => return self.open_applet(),
                Key::Esc | Key::F(10) => return Some(Action::Quit),
                Key::Char(c) => {
                    if let Some(i) = crate::applet_by_letter(self.sel, c) {
                        self.sel = i;
                    }
                }
                Key::Mouse(m) if m.pressed && m.button == 0 => {
                    // Rows 4.. of the applet dialog.
                    let i = m.y.wrapping_sub(4) as usize;
                    if i < APPLETS.len() {
                        if i == self.sel {
                            return self.open_applet();
                        }
                        self.sel = i;
                    }
                }
                _ => {}
            }
            return None;
        };
        if let Some(c) = o.chooser.as_mut() {
            let Value::List(items, cur) = &mut o.page.fields[c.field].value else {
                o.chooser = None;
                return None;
            };
            let m: Vec<usize> = c.matches(items).iter().map(|(i, _)| *i).collect();
            match base {
                Key::Esc => o.chooser = None,
                Key::Up => c.sel = c.sel.saturating_sub(1),
                Key::Down => c.sel = (c.sel + 1).min(m.len().saturating_sub(1)),
                Key::PgUp => c.sel = c.sel.saturating_sub(10),
                Key::PgDn => c.sel = (c.sel + 10).min(m.len().saturating_sub(1)),
                Key::Enter => {
                    if let Some(i) = m.get(c.sel) {
                        *cur = *i;
                    }
                    o.chooser = None;
                }
                Key::Backspace => {
                    c.filter.pop();
                    c.sel = 0;
                }
                Key::Char(ch) if !ch.is_control() => {
                    c.filter.push(ch);
                    c.sel = 0;
                }
                _ => {}
            }
            return None;
        }
        let n = o.page.fields.len();
        let on_field = o.sel < n;
        match base {
            Key::Esc => self.open = None,
            Key::Up | Key::BackTab => o.sel = o.sel.saturating_sub(1),
            Key::Down | Key::Tab => o.sel = (o.sel + 1).min(n + 1),
            Key::Left if on_field => o.page.fields[o.sel].step(-1),
            Key::Right if on_field => o.page.fields[o.sel].step(1),
            Key::Left if o.sel == n + 1 => o.sel = n,
            Key::Right if o.sel == n => o.sel = n + 1,
            Key::F(10) => return self.finish(),
            Key::Enter => {
                if o.sel == n + 1 {
                    self.open = None;
                } else if on_field && matches!(o.page.fields[o.sel].value, Value::List(..)) {
                    return self.choose();
                } else {
                    return self.finish();
                }
            }
            Key::Char(' ') if on_field => {
                let f = &mut o.page.fields[o.sel];
                match &mut f.value {
                    Value::List(..) => return self.choose(),
                    Value::Text(t) if f.enabled => t.push(' '),
                    _ => f.cycle(),
                }
            }
            Key::Backspace if on_field => {
                let f = &mut o.page.fields[o.sel];
                if let (Value::Text(t), true) = (&mut f.value, f.enabled) {
                    t.pop();
                }
            }
            Key::Char(ch) if on_field => {
                let f = &mut o.page.fields[o.sel];
                match &mut f.value {
                    Value::Text(t) if f.enabled && t.chars().count() < 12 && !ch.is_control() => {
                        t.push(ch)
                    }
                    Value::List(..) => {
                        // Typing opens the chooser with that letter.
                        self.choose();
                        return self.key(Key::Char(ch));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        None
    }

    fn choose(&mut self) -> Option<Action> {
        let o = self.open.as_mut()?;
        let f: &Field = o.page.fields.get(o.sel)?;
        if let (Value::List(_, cur), true) = (&f.value, f.enabled) {
            o.chooser = Some(Chooser {
                field: o.sel,
                filter: String::new(),
                sel: *cur,
            });
        }
        None
    }

    fn open_applet(&mut self) -> Option<Action> {
        let a = APPLETS[self.sel];
        if let Some(p) = a.program() {
            return Some(Action::Run(p));
        }
        self.open = Some(Open {
            page: load(a),
            sel: 0,
            chooser: None,
        });
        None
    }

    fn finish(&mut self) -> Option<Action> {
        match &self.open {
            Some(o) if o.page.any_changed() => Some(Action::Apply),
            _ => {
                self.open = None;
                None
            }
        }
    }

    /// Save the open page (after [`Action::Apply`]).
    pub fn apply(&mut self) -> Option<Applied> {
        let o = self.open.as_ref()?;
        match apply(&o.page) {
            Ok(done) => {
                let mut msg = Vec::new();
                if done.restart {
                    msg.push("The new screen settings take effect the".to_string());
                    msg.push("next time the Desktop or RetroDOS starts.".to_string());
                }
                if !msg.is_empty() {
                    self.message = Some(msg);
                }
                self.open = None;
                Some(done)
            }
            Err(e) => {
                self.message = Some(vec![e]);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_the_applets_and_a_page() {
        let mut a = App::new(None);
        let mut s = Screen::with_size(80, 25, None);
        a.draw(&mut s);
        let t = s.dump();
        assert!(
            t.contains("Control Panel") && t.contains("Date & Time") && t.contains("Keyboard"),
            "{t}"
        );
        a.key(Key::Char('d'));
        assert_eq!(APPLETS[a.sel], Applet::DateTime, "D from Display goes on");
        a.key(Key::Char('s'));
        assert_eq!(APPLETS[a.sel], Applet::Startup);
        assert_eq!(a.key(Key::Enter), None);
        a.draw(&mut s);
        let t = s.dump();
        assert!(
            t.contains("Start in") && t.contains("Startup chime") && t.contains("<  OK  >"),
            "{t}"
        );
        // Esc leaves the page without saving.
        a.key(Key::Esc);
        assert!(a.open.is_none());
        a.key(Key::Char('n'));
        assert_eq!(a.key(Key::Enter), Some(Action::Run("retro-net")));
    }

    #[test]
    fn a_list_is_chosen_by_typing() {
        let mut a = App::new(Some(Applet::Keyboard));
        let before = a.open.as_ref().unwrap().page.fields[0].text();
        a.key(Key::Char('g'));
        a.key(Key::Char('e'));
        a.key(Key::Char('r'));
        let mut s = Screen::with_size(80, 25, None);
        a.draw(&mut s);
        assert!(s.dump().contains("Search:"));
        a.key(Key::Enter);
        let o = a.open.as_ref().unwrap();
        assert!(o.chooser.is_none());
        let now = o.page.fields[0].shown();
        assert!(now.to_lowercase().contains("ger"), "{now} (was {before})");
    }
}

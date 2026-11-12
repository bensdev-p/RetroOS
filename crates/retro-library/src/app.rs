//! The LIBRARY screen: categories on the left (and Installed, Updates,
//! Search everything), the chosen category's programs on the right, the
//! chosen program's description underneath. Enter installs (or removes),
//! U installs updates, / searches. Installing asks for the administrator's
//! password, then shows a progress bar while apt works.

use retro_sys::catalog::{catalog, App as Entry};
use retro_sys::software::{self, Job, Op, Update};
use retro_tui::{
    Field, Key, Screen, BLACK, COBALT, CORAL, LEMON, LIME, SILVER, SLATE, TEAL, WHITE,
};
use std::collections::HashSet;

/// The left-hand list: the catalog's categories, then these.
pub const INSTALLED: &str = "Installed";
pub const UPDATES: &str = "Updates";
pub const SEARCH: &str = "Search everything";

/// One row on the right.
#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    App(Entry),
    /// A package from the whole index (Search everything): name, summary.
    Package(String, String),
    Update(Update),
}

impl Row {
    fn name(&self) -> String {
        match self {
            Row::App(a) => a.name.clone(),
            Row::Package(p, _) => p.clone(),
            Row::Update(u) => u.package.clone(),
        }
    }
}

/// A modal box over the screen.
pub enum Modal {
    None,
    /// The administrator's password for this operation: the field, the
    /// operation, what to ask, what to say when it is done.
    Password(Field, Op, String, String),
    /// Yes/No before removing or updating.
    Confirm(Vec<String>, Op, String),
    /// Typing the search word.
    Search(Field),
    /// apt at work: the job, what to say when it is done, and what Esc
    /// (Cancel) has done so far.
    Busy(Job, String, Option<software::Cancel>),
    /// "Force Stop?" (Y/N) over a running job (D-058).
    ForceAsk(Job, String, software::Cancel),
    /// A message; any key closes it.
    Message(Vec<String>),
}

pub struct App {
    pub cats: Vec<&'static str>,
    pub cat: usize,
    pub rows: Vec<Row>,
    pub sel: usize,
    /// Keys go to the right-hand list.
    pub right: bool,
    pub installed: HashSet<String>,
    pub modal: Modal,
    pub search: String,
    pub quit: bool,
}

fn clip(s: &str, w: usize) -> String {
    s.chars().take(w).collect()
}

impl App {
    pub fn new(start: Option<&str>) -> App {
        let mut cats: Vec<&'static str> = catalog().categories.iter().map(|c| c.name).collect();
        cats.extend([INSTALLED, UPDATES, SEARCH]);
        let cat = start
            .and_then(|s| cats.iter().position(|c| c.eq_ignore_ascii_case(s)))
            .unwrap_or(0);
        let mut a = App {
            cats,
            cat,
            rows: Vec::new(),
            sel: 0,
            right: start.is_some(),
            installed: software::installed_catalog(),
            modal: Modal::None,
            search: String::new(),
            quit: false,
        };
        a.load();
        a
    }

    fn is_installed(&self, e: &Entry) -> bool {
        self.installed.contains(&e.packages[0])
    }

    /// Fill the right-hand list for the chosen category.
    pub fn load(&mut self) {
        let c = catalog();
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
                if self.search.is_empty() {
                    Vec::new()
                } else {
                    let mut v: Vec<Row> = c
                        .search(&self.search)
                        .into_iter()
                        .cloned()
                        .map(Row::App)
                        .collect();
                    let known: HashSet<String> =
                        c.apps.iter().map(|a| a.packages[0].clone()).collect();
                    v.extend(
                        software::search_all(&self.search)
                            .into_iter()
                            .filter(|(p, _)| !known.contains(p))
                            .map(|(p, s)| Row::Package(p, s)),
                    );
                    v
                }
            }
            name => c
                .in_category(name)
                .into_iter()
                .cloned()
                .map(Row::App)
                .collect(),
        };
        self.sel = self.sel.min(self.rows.len().saturating_sub(1));
    }

    fn refresh_installed(&mut self) {
        self.installed = software::installed_catalog();
        let keep = self.sel;
        self.load();
        self.sel = keep.min(self.rows.len().saturating_sub(1));
    }

    pub fn draw(&self, s: &mut Screen) {
        s.clear(SILVER, COBALT);
        s.cursor = None;
        s.fill(0, 0, s.cols, 1, ' ', BLACK, TEAL);
        s.text(1, 0, "RetroOS Software Library", BLACK, TEAL);
        let y = s.rows - 1;
        s.fill(0, y, s.cols, 1, ' ', BLACK, TEAL);
        s.text(
            1,
            y,
            "\u{2191}\u{2193} Choose  Tab Switch  Enter Install/Remove  U Updates  / Search  Esc Close",
            BLACK,
            TEAL,
        );
        let w = s.cols.saturating_sub(2).min(98);
        let h = s.rows.saturating_sub(3);
        let x = (s.cols - w) / 2;
        s.dialog(x, 1, w, h, "Software Library", WHITE, COBALT);
        // Left: categories.
        let lw = 20u16;
        for (i, c) in self.cats.iter().enumerate() {
            let yy = 3 + i as u16 + u16::from(i >= self.cats.len() - 3);
            let on = i == self.cat;
            let (fg, bg) = match (on, self.right) {
                (true, false) => (BLACK, SILVER),
                (true, true) => (WHITE, SLATE),
                _ => (WHITE, COBALT),
            };
            s.fill(x + 2, yy, lw, 1, ' ', fg, bg);
            let label = if *c == UPDATES {
                let n = software::status().count;
                if n > 0 {
                    format!("{c} ({n})")
                } else {
                    c.to_string()
                }
            } else {
                c.to_string()
            };
            s.text(x + 3, yy, &clip(&label, lw as usize - 2), fg, bg);
        }
        // Right: programs.
        let rx = x + lw + 4;
        let rw = w.saturating_sub(lw + 7);
        let list_h = h.saturating_sub(9) as usize;
        if self.rows.is_empty() {
            let msg = match self.cats[self.cat] {
                UPDATES => "No updates are waiting. Press U to look for new ones.",
                SEARCH if self.search.is_empty() => "Press / and type a word to search.",
                SEARCH => "Nothing found.",
                INSTALLED => "None of the Library's programs are installed yet.",
                _ => "",
            };
            s.text(rx, 3, &clip(msg, rw as usize), SILVER, COBALT);
        }
        let first = self.sel.saturating_sub(list_h.saturating_sub(1));
        for (row, (i, r)) in self
            .rows
            .iter()
            .enumerate()
            .skip(first)
            .take(list_h)
            .enumerate()
        {
            let yy = 3 + row as u16;
            let on = i == self.sel;
            let (fg, bg) = match (on, self.right) {
                (true, true) => (BLACK, SILVER),
                (true, false) => (WHITE, SLATE),
                _ => (WHITE, COBALT),
            };
            s.fill(rx, yy, rw, 1, ' ', fg, bg);
            s.text(rx + 1, yy, &clip(&r.name(), 30), fg, bg);
            let (tag, tfg) = match r {
                Row::App(a) if self.is_installed(a) => ("Installed", if on { BLACK } else { LIME }),
                Row::Package(..) => ("Debian", if on { BLACK } else { SILVER }),
                Row::Update(u) if u.security => ("Security", if on { BLACK } else { CORAL }),
                Row::Update(_) => ("Update", if on { BLACK } else { SILVER }),
                _ => ("", fg),
            };
            if rw > 44 {
                s.text(rx + rw - 12, yy, tag, tfg, bg);
            }
        }
        // Bottom: the chosen program.
        let by = 1 + h - 6;
        s.fill(x + 2, by - 1, w - 4, 1, '\u{2500}', WHITE, COBALT);
        if let Some(r) = self.rows.get(self.sel) {
            let iw = (w - 6) as usize;
            match r {
                Row::App(a) => {
                    s.text(x + 3, by, &clip(&a.name, iw), LEMON, COBALT);
                    s.text(x + 3, by + 1, &clip(&a.blurb, iw), WHITE, COBALT);
                    let mut more = format!("Package: {}", a.packages.join(", "));
                    if !a.alias.is_empty() {
                        more.push_str(&format!("   Type: {}", a.alias.join(", ")));
                    }
                    s.text(x + 3, by + 2, &clip(&more, iw), SILVER, COBALT);
                    let what = if self.is_installed(a) {
                        "Installed. Enter removes it."
                    } else {
                        "Enter installs it."
                    };
                    s.text(x + 3, by + 3, what, SILVER, COBALT);
                }
                Row::Package(p, sum) => {
                    s.text(x + 3, by, &clip(p, iw), LEMON, COBALT);
                    s.text(x + 3, by + 1, &clip(sum, iw), WHITE, COBALT);
                    s.text(
                        x + 3,
                        by + 2,
                        "From the Debian archive (not checked for RetroOS). Enter installs it.",
                        SILVER,
                        COBALT,
                    );
                }
                Row::Update(u) => {
                    s.text(x + 3, by, &clip(&u.package, iw), LEMON, COBALT);
                    let v = format!("{} \u{2192} {}", u.from.as_deref().unwrap_or("new"), u.to);
                    s.text(x + 3, by + 1, &clip(&v, iw), WHITE, COBALT);
                    s.text(x + 3, by + 2, "U installs every update.", SILVER, COBALT);
                }
            }
        }
        self.draw_modal(s);
    }

    fn draw_modal(&self, s: &mut Screen) {
        let box_at = |s: &mut Screen, h: u16, title: &str| {
            let w = 56u16.min(s.cols.saturating_sub(4));
            let x = (s.cols - w) / 2;
            let y = (s.rows.saturating_sub(h)) / 2;
            s.dialog(x, y, w, h, title, BLACK, SILVER);
            (x, y, w)
        };
        match &self.modal {
            Modal::None => {}
            Modal::Password(f, _, what, _) => {
                let (x, y, w) = box_at(s, 7, "Administrator");
                s.text(x + 3, y + 2, &clip(what, w as usize - 6), BLACK, SILVER);
                s.text(x + 3, y + 4, "Password:", BLACK, SILVER);
                let cx = f.draw(s, x + 14, y + 4, w - 17, WHITE, BLACK);
                s.cursor = Some((cx, y + 4));
            }
            Modal::Confirm(lines, ..) => {
                let (x, y, w) = box_at(s, lines.len() as u16 + 5, "Software Library");
                for (i, l) in lines.iter().enumerate() {
                    s.text(
                        x + 3,
                        y + 2 + i as u16,
                        &clip(l, w as usize - 6),
                        BLACK,
                        SILVER,
                    );
                }
                s.text(
                    x + 3,
                    y + 3 + lines.len() as u16,
                    "Y Yes    N No",
                    BLACK,
                    SILVER,
                );
            }
            Modal::Search(f) => {
                let (x, y, w) = box_at(s, 5, "Search");
                s.text(x + 3, y + 2, "Find:", BLACK, SILVER);
                let cx = f.draw(s, x + 9, y + 2, w - 12, WHITE, BLACK);
                s.cursor = Some((cx, y + 2));
            }
            Modal::ForceAsk(..) => {
                let lines = [
                    "Force Stop ends the installation now.",
                    "A program being set up may be left half installed;",
                    "RetroOS finishes it before the next installation.",
                ];
                let (x, y, w) = box_at(s, 7, "Force Stop");
                for (i, l) in lines.iter().enumerate() {
                    s.text(
                        x + 3,
                        y + 2 + i as u16,
                        &clip(l, w as usize - 6),
                        BLACK,
                        SILVER,
                    );
                }
                s.text(x + 3, y + 5, "Y Force Stop    N Keep going", BLACK, SILVER);
            }
            Modal::Busy(job, _, cancel) => {
                let p = job.progress();
                let (x, y, w) = box_at(s, 7, "Please wait");
                let text = match cancel {
                    Some(c) if !p.text.starts_with("Cancelling") => c.say().to_string(),
                    _ => p.text.clone(),
                };
                s.text(x + 3, y + 2, &clip(&text, w as usize - 6), BLACK, SILVER);
                let hint = if cancel.is_some() {
                    "Esc Force Stop"
                } else {
                    "Esc Cancel"
                };
                s.text(x + 3, y + 5, hint, SLATE, SILVER);
                let bw = (w - 6) as u32;
                let filled = (bw * p.percent.min(100) / 100) as u16;
                s.fill(x + 3, y + 4, bw as u16, 1, '\u{2591}', SLATE, SILVER);
                s.fill(x + 3, y + 4, filled, 1, '\u{2588}', COBALT, SILVER);
                s.text(
                    x + w - 8,
                    y + 2,
                    &format!("{:>3} %", p.percent),
                    BLACK,
                    SILVER,
                );
            }
            Modal::Message(lines) => {
                let (x, y, w) = box_at(s, lines.len() as u16 + 4, "Software Library");
                for (i, l) in lines.iter().enumerate() {
                    s.text(
                        x + 3,
                        y + 2 + i as u16,
                        &clip(l, w as usize - 6),
                        BLACK,
                        SILVER,
                    );
                }
            }
        }
    }

    /// Run `op`, asking for the password first if sudo needs it.
    fn begin(&mut self, op: Op, done: String) {
        if software::needs_password() {
            let what = match &op {
                Op::Install(_) => "Installing software needs your password.",
                Op::Remove(_) => "Removing software needs your password.",
                _ => "Installing updates needs your password.",
            };
            self.modal = Modal::Password(Field::new(64, true), op, what.into(), done);
            return;
        }
        self.run(op, done);
    }

    fn run(&mut self, op: Op, done: String) {
        self.modal = match software::start(op) {
            Ok(j) => Modal::Busy(j, done, None),
            Err(e) => Modal::Message(vec![e]),
        };
    }

    /// The chosen row's action (Enter).
    fn act(&mut self) {
        let Some(r) = self.rows.get(self.sel).cloned() else {
            return;
        };
        match r {
            Row::App(a) if self.is_installed(&a) => {
                self.modal = Modal::Confirm(
                    vec![format!("Remove {}?", a.name)],
                    Op::Remove(vec![a.packages[0].clone()]),
                    format!("{} has been removed.", a.name),
                );
            }
            Row::App(a) => {
                let mut done = format!("{} is installed.", a.name);
                if !a.run.is_empty() {
                    done.push_str(&format!(
                        " Type {} to start it.",
                        a.alias
                            .first()
                            .cloned()
                            .unwrap_or_else(|| a.run.to_ascii_uppercase())
                    ));
                }
                if let Some(n) = &a.note {
                    done.push('\n');
                    done.push_str(n);
                }
                self.begin(Op::Install(a.packages.clone()), done);
            }
            Row::Package(p, _) => {
                self.begin(Op::Install(vec![p.clone()]), format!("{p} is installed."));
            }
            Row::Update(_) => self.update(),
        }
    }

    fn update(&mut self) {
        let n = software::updates().len();
        let first = if n == 0 {
            "Look for updates and install them now?".to_string()
        } else {
            format!("Install {n} update{} now?", if n == 1 { "" } else { "s" })
        };
        self.modal = Modal::Confirm(
            vec![first, "The computer stays usable meanwhile.".into()],
            Op::UpgradeAll,
            "The updates are installed.".into(),
        );
    }

    /// Call often: notices a finished job.
    pub fn tick(&mut self) {
        let (job, done, cancel) = match &self.modal {
            Modal::Busy(j, d, c) => (j, d, *c),
            Modal::ForceAsk(j, d, c) => (j, d, Some(*c)),
            _ => return,
        };
        if let Some(r) = job.finished() {
            let lines = match (cancel, r) {
                (Some(software::Cancel::Stopped), _) => vec![
                    "Stopped.".to_string(),
                    "Anything left half installed is finished before the next installation."
                        .to_string(),
                ],
                (Some(_), Ok(())) => vec!["It had already finished.".to_string()],
                (None, Ok(())) => done.lines().map(String::from).collect(),
                (_, Err(e)) => vec![e],
            };
            self.modal = Modal::Message(lines);
            self.refresh_installed();
        }
    }

    pub fn key(&mut self, k: Key) {
        match std::mem::replace(&mut self.modal, Modal::None) {
            Modal::None => {}
            Modal::Message(_) => return,
            Modal::Busy(j, d, c) => {
                // Esc: Cancel, then Force Stop after asking (D-058).
                self.modal = match (k, c) {
                    (Key::Esc, None) => match software::cancel(false) {
                        Ok(c) => Modal::Busy(j, d, Some(c)),
                        Err(_) => Modal::Busy(j, d, None),
                    },
                    (Key::Esc, Some(software::Cancel::Stopped)) => Modal::Busy(j, d, c),
                    (Key::Esc, Some(c)) => Modal::ForceAsk(j, d, c),
                    _ => Modal::Busy(j, d, c),
                };
                return;
            }
            Modal::ForceAsk(j, d, before) => {
                self.modal = match k {
                    Key::Char('y' | 'Y') => {
                        let c = software::cancel(true).unwrap_or(software::Cancel::Stopped);
                        Modal::Busy(j, d, Some(c))
                    }
                    Key::Char('n' | 'N') | Key::Esc | Key::Enter => Modal::Busy(j, d, Some(before)),
                    _ => Modal::ForceAsk(j, d, before),
                };
                return;
            }
            Modal::Confirm(l, op, done) => {
                match k {
                    Key::Char('y' | 'Y') | Key::Enter => self.begin(op, done),
                    Key::Char('n' | 'N') | Key::Esc => {}
                    _ => self.modal = Modal::Confirm(l, op, done),
                }
                return;
            }
            Modal::Search(mut f) => {
                match k {
                    Key::Enter => {
                        self.search = f.value.trim().to_string();
                        self.cat = self.cats.len() - 1;
                        self.sel = 0;
                        self.right = true;
                        self.load();
                    }
                    Key::Esc => {}
                    k => {
                        f.edit(&k);
                        self.modal = Modal::Search(f);
                    }
                }
                return;
            }
            Modal::Password(mut f, op, what, done) => {
                match k {
                    Key::Enter => match software::authorize(&f.value) {
                        Ok(()) => self.run(op, done),
                        Err(retro_sys::admin::AdminError::WrongPassword) => {
                            f.value.clear();
                            self.modal = Modal::Password(
                                f,
                                op,
                                "That password is not right. Try again.".into(),
                                done,
                            );
                        }
                        Err(e) => {
                            self.modal = Modal::Message(vec![e
                                .to_string()
                                .replace("change this setting", "install software")])
                        }
                    },
                    Key::Esc => {}
                    k => {
                        f.edit(&k);
                        self.modal = Modal::Password(f, op, what, done);
                    }
                }
                return;
            }
        }
        let n = self.rows.len();
        match k {
            Key::Esc | Key::F(10) => self.quit = true,
            Key::Tab | Key::BackTab => self.right = !self.right && n > 0,
            Key::Right if n > 0 => self.right = true,
            Key::Left => self.right = false,
            Key::Up if self.right => self.sel = self.sel.saturating_sub(1),
            Key::Down if self.right => self.sel = (self.sel + 1).min(n.saturating_sub(1)),
            Key::Up | Key::Down => {
                let d: isize = if k == Key::Up { -1 } else { 1 };
                self.cat = (self.cat as isize + d).clamp(0, self.cats.len() as isize - 1) as usize;
                self.sel = 0;
                self.load();
            }
            Key::PgUp => self.sel = self.sel.saturating_sub(10),
            Key::PgDn => self.sel = (self.sel + 10).min(n.saturating_sub(1)),
            Key::Enter if !self.right => self.right = n > 0,
            Key::Enter | Key::Char('i' | 'I') if self.right => self.act(),
            Key::Char('u' | 'U') => self.update(),
            Key::Char('/') | Key::Char('s' | 'S') => {
                let mut f = Field::new(40, false);
                f.value = self.search.clone();
                self.modal = Modal::Search(f);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_programs_and_install_asks_password() {
        std::env::set_var("RETROOS_TEST_SOFTWARE", "1");
        std::env::set_var(
            "XDG_RUNTIME_DIR",
            std::env::temp_dir().join(format!("retro-lib-{}", std::process::id())),
        );
        let _ = std::fs::create_dir_all(std::env::var("XDG_RUNTIME_DIR").unwrap());
        let mut a = App::new(None);
        let mut s = Screen::with_size(100, 30, None);
        a.draw(&mut s);
        let t = s.dump();
        assert!(
            t.contains("Internet") && t.contains("Chromium") && t.contains("Updates (3)"),
            "{t}"
        );
        a.key(Key::Right);
        a.key(Key::Enter);
        a.draw(&mut s);
        assert!(s.dump().contains("needs your password"), "{}", s.dump());
        for c in "retro".chars() {
            a.key(Key::Char(c));
        }
        a.key(Key::Enter);
        assert!(matches!(a.modal, Modal::Busy(..)));
    }
}

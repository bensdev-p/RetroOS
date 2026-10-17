//! FILER's state, keys and drawing (Norton Commander × ProDOS Filer).
//! Like EDIT, every modal state lives in `App` so tests can drive it.

use crate::panel::{self, Panel};
use retro_dosfs::Dos;
use retro_tui::keys::Mouse;
use retro_tui::{Field, Key, Screen, AQUA, BLACK, BRICK, COBALT, LEMON, LIME, SILVER, TEAL, WHITE};
use std::path::{Path, PathBuf};

/// What the main loop must do outside the TUI (the terminal is needed).
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
    Edit(PathBuf),
    Run(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ask {
    Copy,
    Move,
    Mkdir,
    Delete,
    Message,
}

pub struct Dialog {
    pub ask: Ask,
    pub title: String,
    pub text: Vec<String>,
    pub field: Option<Field>,
    pub buttons: Vec<&'static str>,
    /// 0 = field (if any), then buttons.
    pub focus: usize,
    fresh: bool,
}

pub struct Viewer {
    pub title: String,
    pub lines: Vec<String>,
    pub top: usize,
}

pub enum Mode {
    Normal,
    Dialog(Dialog),
    View(Viewer),
    Drives { panel: usize, cursor: usize },
    Help,
}

pub struct App {
    pub dos: Dos,
    pub panels: [Panel; 2],
    pub active: usize,
    pub mode: Mode,
    /// Entry rows per panel and panel width, from the last draw.
    rows: usize,
    panels_width: usize,
}

const FKEYS: &[(&str, &str)] = &[
    ("1", "Help"),
    ("2", "Hidden"),
    ("3", "View"),
    ("4", "Edit"),
    ("5", "Copy"),
    ("6", "Move"),
    ("7", "MkDir"),
    ("8", "Delete"),
    ("9", "Drive"),
    ("10", "Quit"),
];

fn date_time(t: i64) -> (String, String) {
    unsafe {
        let tt = t as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&tt, &mut tm);
        (
            format!(
                "{:02}-{:02}-{:02}",
                tm.tm_mon + 1,
                tm.tm_mday,
                tm.tm_year % 100
            ),
            format!("{:02}:{:02}", tm.tm_hour, tm.tm_min),
        )
    }
}

fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn clip(s: &str, w: usize) -> String {
    if s.chars().count() <= w {
        format!("{s:<w$}")
    } else if w > 1 {
        let mut t: String = s.chars().take(w - 1).collect();
        t.push('»');
        t
    } else {
        s.chars().take(w).collect()
    }
}

impl App {
    pub fn new(dos: Dos, left: &Path, right: &Path) -> App {
        App {
            dos,
            panels: [Panel::new(left), Panel::new(right)],
            active: 0,
            mode: Mode::Normal,
            rows: 20,
            panels_width: 40,
        }
    }

    fn p(&mut self) -> &mut Panel {
        &mut self.panels[self.active]
    }

    fn other_dir(&self) -> PathBuf {
        self.panels[1 - self.active].dir.clone()
    }

    fn reload_all(&mut self) {
        self.panels[0].reload();
        self.panels[1].reload();
    }

    fn message(&mut self, title: &str, text: &[String]) {
        self.mode = Mode::Dialog(Dialog {
            ask: Ask::Message,
            title: title.into(),
            text: text.to_vec(),
            field: None,
            buttons: vec!["OK"],
            focus: 0,
            fresh: true,
        });
    }

    fn ask(
        &mut self,
        ask: Ask,
        title: &str,
        text: String,
        value: Option<String>,
        buttons: &[&'static str],
    ) {
        let field = value.map(|v| {
            let mut f = Field::new(400, false);
            f.value = v;
            f
        });
        self.mode = Mode::Dialog(Dialog {
            ask,
            title: title.into(),
            text: vec![text],
            field,
            buttons: buttons.to_vec(),
            focus: 0,
            fresh: true,
        });
    }

    fn count_text(&self, verb: &str) -> String {
        let sel = self.panels[self.active].selection();
        match sel.as_slice() {
            [one] => format!(
                "{verb} \"{}\" to:",
                one.file_name().unwrap_or_default().to_string_lossy()
            ),
            v => format!("{verb} {} files to:", v.len()),
        }
    }

    /// A destination typed by the user: a DOS path (C:\GAMES) or a POSIX one,
    /// relative to the active panel.
    fn dest_path(&self, s: &str) -> Option<PathBuf> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }
        let dos_like = retro_dosfs::split_drive(s).0.is_some() || s.contains('\\');
        if dos_like {
            return self.dos.resolve_new(s).ok();
        }
        let p = Path::new(s);
        Some(if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.panels[self.active].dir.join(p)
        })
    }

    // --------------------------------------------------------------- keys

    pub fn key(&mut self, k: Key) -> Action {
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Normal => return self.normal_key(k),
            Mode::Dialog(d) => self.dialog_key(d, k),
            Mode::View(v) => self.view_key(v, k),
            Mode::Drives { panel, cursor } => self.drives_key(panel, cursor, k),
            Mode::Help => {}
        }
        Action::None
    }

    fn normal_key(&mut self, k: Key) -> Action {
        let rows = self.rows;
        match k {
            Key::Tab | Key::BackTab => self.active = 1 - self.active,
            Key::Up => self.p().move_by(-1),
            Key::Down => self.p().move_by(1),
            Key::PgUp => self.p().move_by(-(rows as isize - 1)),
            Key::PgDn => self.p().move_by(rows as isize - 1),
            Key::Home => self.p().cursor = 0,
            Key::End => {
                let n = self.p().entries.len();
                self.p().cursor = n.saturating_sub(1);
            }
            Key::Insert | Key::Char(' ') => self.p().toggle_mark(),
            Key::Char('+') => {
                let p = self.p();
                p.marked = p
                    .entries
                    .iter()
                    .filter(|e| !e.dir)
                    .map(|e| e.name.clone())
                    .collect();
            }
            Key::Char('-') => self.p().marked.clear(),
            Key::Char('*') => {
                let p = self.p();
                let all: Vec<String> = p
                    .entries
                    .iter()
                    .filter(|e| !e.dir)
                    .map(|e| e.name.clone())
                    .collect();
                for n in all {
                    if !p.marked.remove(&n) {
                        p.marked.insert(n);
                    }
                }
            }
            Key::Backspace => {
                if let Err(e) = self.p().up() {
                    self.message("Error", &[e]);
                }
            }
            Key::Enter => return self.enter(),
            Key::Ctrl('r') => self.reload_all(),
            Key::F(1) => self.mode = Mode::Help,
            Key::F(2) | Key::Ctrl('h') => {
                let p = self.p();
                p.show_hidden = !p.show_hidden;
                p.reload();
            }
            Key::F(3) => self.view(),
            Key::F(4) => {
                if let Some(e) = self.panels[self.active].current() {
                    if !e.dir {
                        return Action::Edit(self.panels[self.active].current_path().unwrap());
                    }
                }
            }
            Key::F(5) => {
                if !self.panels[self.active].selection().is_empty() {
                    let dest = self.dos.display(&self.other_dir());
                    let text = self.count_text("Copy");
                    self.ask(Ask::Copy, "Copy", text, Some(dest), &["Copy", "Cancel"]);
                }
            }
            Key::F(6) => {
                if !self.panels[self.active].selection().is_empty() {
                    let dest = self.dos.display(&self.other_dir());
                    let text = self.count_text("Move or rename");
                    self.ask(Ask::Move, "Move", text, Some(dest), &["Move", "Cancel"]);
                }
            }
            Key::F(7) => self.ask(
                Ask::Mkdir,
                "Make Directory",
                "Name of the new directory:".into(),
                Some(String::new()),
                &["OK", "Cancel"],
            ),
            Key::F(8) | Key::Delete => {
                let n = self.panels[self.active].selection().len();
                if n > 0 {
                    let what = if n == 1 {
                        let p = &self.panels[self.active].selection()[0];
                        format!(
                            "Delete \"{}\"?",
                            p.file_name().unwrap_or_default().to_string_lossy()
                        )
                    } else {
                        format!("Delete {n} files?")
                    };
                    self.ask(Ask::Delete, "Delete", what, None, &["Delete", "Cancel"]);
                }
            }
            Key::F(9) | Key::Alt('1') => self.drives(0),
            Key::Alt('2') => self.drives(1),
            Key::F(10) | Key::Ctrl('q') => return Action::Quit,
            Key::Mouse(m) => self.mouse(m),
            Key::Char(c) if c.is_alphanumeric() || c == '.' || c == '_' => self.p().jump(c),
            _ => {}
        }
        Action::None
    }

    fn enter(&mut self) -> Action {
        let Some(e) = self.panels[self.active].current().cloned() else {
            return Action::None;
        };
        let path = self.panels[self.active].current_path().unwrap();
        if e.name == ".." {
            if let Err(err) = self.p().up() {
                self.message("Error", &[err]);
            }
        } else if e.dir {
            if let Err(err) = self.p().cd(&path) {
                self.message("Error", &[err]);
            }
        } else if e.exec {
            return Action::Run(path);
        } else {
            self.view();
        }
        Action::None
    }

    fn mouse(&mut self, m: Mouse) {
        let half = self.half();
        let side = usize::from(m.x as usize >= half);
        match m.button {
            64 => self.panels[side].move_by(-3),
            65 => self.panels[side].move_by(3),
            0 if m.pressed => {
                self.active = side;
                let y = m.y as usize;
                if y >= 3 && y < 3 + self.rows {
                    let p = &mut self.panels[side];
                    let i = p.top + y - 3;
                    if i < p.entries.len() {
                        p.cursor = i;
                    }
                }
            }
            _ => {}
        }
    }

    fn half(&self) -> usize {
        self.panels_width
    }

    fn view(&mut self) {
        let p = &self.panels[self.active];
        let Some(e) = p.current() else { return };
        if e.dir {
            return;
        }
        let path = p.current_path().unwrap();
        let mut f = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(err) => {
                self.message("Error", &[err.to_string()]);
                return;
            }
        };
        use std::io::Read;
        let mut data = Vec::new();
        let _ = (&mut f).take(4 << 20).read_to_end(&mut data);
        let text: String = String::from_utf8_lossy(&data)
            .chars()
            .map(|c| {
                if c.is_control() && c != '\n' && c != '\t' {
                    '.'
                } else {
                    c
                }
            })
            .collect();
        let lines = text.lines().map(|l| l.replace('\t', "        ")).collect();
        self.mode = Mode::View(Viewer {
            title: self.dos.display(&path),
            lines,
            top: 0,
        });
    }

    fn view_key(&mut self, mut v: Viewer, k: Key) {
        let page = self.rows + 3;
        let max = v.lines.len().saturating_sub(page);
        match k {
            Key::Esc | Key::F(3) | Key::F(10) | Key::Char('q') => return,
            Key::Up => v.top = v.top.saturating_sub(1),
            Key::Down | Key::Enter => v.top = (v.top + 1).min(max),
            Key::PgUp => v.top = v.top.saturating_sub(page),
            Key::PgDn | Key::Char(' ') => v.top = (v.top + page).min(max),
            Key::Home => v.top = 0,
            Key::End => v.top = max,
            Key::Mouse(m) if m.button == 64 => v.top = v.top.saturating_sub(3),
            Key::Mouse(m) if m.button == 65 => v.top = (v.top + 3).min(max),
            _ => {}
        }
        self.mode = Mode::View(v);
    }

    fn drives(&mut self, panel: usize) {
        let cur = self.dos.to_dos(&self.panels[panel].dir).0;
        let cursor = self.dos.drives().position(|d| d.letter == cur).unwrap_or(0);
        self.mode = Mode::Drives { panel, cursor };
    }

    fn drives_key(&mut self, panel: usize, cursor: usize, k: Key) {
        let drives: Vec<_> = self.dos.drives().cloned().collect();
        let n = drives.len();
        let pick = match k {
            Key::Esc => return,
            Key::Up | Key::Left => {
                self.mode = Mode::Drives {
                    panel,
                    cursor: (cursor + n - 1) % n,
                };
                return;
            }
            Key::Down | Key::Right => {
                self.mode = Mode::Drives {
                    panel,
                    cursor: (cursor + 1) % n,
                };
                return;
            }
            Key::Enter => Some(cursor),
            Key::Char(c) => drives
                .iter()
                .position(|d| d.letter.eq_ignore_ascii_case(&c)),
            _ => None,
        };
        match pick {
            Some(i) => {
                if let Err(e) = self.panels[panel].cd(&drives[i].root) {
                    self.message(
                        "Error",
                        &[format!("Drive {}: is not ready", drives[i].letter), e],
                    );
                }
                self.active = panel;
            }
            None => self.mode = Mode::Drives { panel, cursor },
        }
    }

    fn dialog_key(&mut self, mut d: Dialog, k: Key) {
        let nf = usize::from(d.field.is_some());
        let total = nf + d.buttons.len();
        match k {
            Key::Esc => return,
            Key::Tab | Key::Down => d.focus = (d.focus + 1) % total,
            Key::BackTab | Key::Up => d.focus = (d.focus + total - 1) % total,
            Key::Left | Key::Right if d.focus >= nf => {
                let nb = d.buttons.len();
                let b = d.focus - nf;
                d.focus = nf
                    + if k == Key::Left {
                        (b + nb - 1) % nb
                    } else {
                        (b + 1) % nb
                    };
            }
            Key::Enter => {
                let b = d.focus.saturating_sub(nf);
                return self.dialog_done(d, b);
            }
            k if d.focus < nf => {
                if let Some(f) = d.field.as_mut() {
                    if d.fresh && matches!(k, Key::Char(_)) {
                        f.value.clear();
                    }
                    f.edit(&k);
                }
            }
            Key::Char(c) => {
                if let Some(i) = d
                    .buttons
                    .iter()
                    .position(|b| b.to_ascii_lowercase().starts_with(c.to_ascii_lowercase()))
                {
                    return self.dialog_done(d, i);
                }
            }
            _ => {}
        }
        d.fresh = false;
        self.mode = Mode::Dialog(d);
    }

    fn dialog_done(&mut self, d: Dialog, b: usize) {
        if d.buttons.get(b).is_none_or(|l| *l == "Cancel") {
            return;
        }
        let value = d
            .field
            .as_ref()
            .map(|f| f.value.clone())
            .unwrap_or_default();
        let sel = self.panels[self.active].selection();
        let mut errors = Vec::new();
        match d.ask {
            Ask::Message => return,
            Ask::Copy | Ask::Move => {
                let Some(dest) = self.dest_path(&value) else {
                    return;
                };
                if sel.len() > 1 && !dest.is_dir() {
                    errors.push(format!("{value} is not a directory"));
                } else {
                    for s in &sel {
                        let r = if d.ask == Ask::Copy {
                            panel::copy_into(s, &dest)
                        } else {
                            panel::move_into(s, &dest)
                        };
                        if let Err(e) = r {
                            errors.push(format!(
                                "{}: {e}",
                                s.file_name().unwrap_or_default().to_string_lossy()
                            ));
                        }
                    }
                }
                self.panels[self.active].marked.clear();
            }
            Ask::Mkdir => {
                if let Some(p) = self.dest_path(&value) {
                    match std::fs::create_dir_all(&p) {
                        Ok(()) => {
                            self.reload_all();
                            let name = p
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned();
                            let panel = self.p();
                            if let Some(i) = panel.entries.iter().position(|e| e.name == name) {
                                panel.cursor = i;
                            }
                            return;
                        }
                        Err(e) => errors.push(e.to_string()),
                    }
                }
            }
            Ask::Delete => {
                for s in &sel {
                    if let Err(e) = panel::delete(s) {
                        errors.push(format!(
                            "{}: {e}",
                            s.file_name().unwrap_or_default().to_string_lossy()
                        ));
                    }
                }
                self.panels[self.active].marked.clear();
            }
        }
        self.reload_all();
        if !errors.is_empty() {
            errors.truncate(6);
            self.message("Error", &errors);
        }
    }

    // ------------------------------------------------------------ drawing

    pub fn draw(&mut self, s: &mut Screen) {
        let (cols, rows) = (s.cols as usize, s.rows as usize);
        s.clear(AQUA, COBALT);
        s.cursor = None;
        if let Mode::View(v) = &self.mode {
            self.draw_viewer(s, v);
            return;
        }
        // Title bar.
        s.fill(0, 0, s.cols, 1, ' ', BLACK, TEAL);
        s.text(1, 0, "RetroOS Filer", BLACK, TEAL);
        let help = "Tab Switch  Ins Mark  Enter Open";
        s.text(
            s.cols.saturating_sub(help.len() as u16 + 1),
            0,
            help,
            BLACK,
            TEAL,
        );

        let half = cols / 2;
        self.panels_width = half;
        // Rows: title bar, frame top, header, entries, separator, info, frame bottom, F-keys.
        let list_rows = rows.saturating_sub(8).max(1);
        self.rows = list_rows;
        for side in 0..2 {
            let x0 = if side == 0 { 0 } else { half };
            let w = if side == 0 { half } else { cols - half };
            self.draw_panel(s, side, x0 as u16, w as u16, list_rows);
        }

        // F-key bar.
        let y = s.rows - 1;
        s.fill(0, y, s.cols, 1, ' ', BLACK, BLACK);
        let slot = (cols / FKEYS.len()).max(6);
        for (i, (n, label)) in FKEYS.iter().enumerate() {
            let x = (i * slot) as u16;
            s.text(x, y, n, WHITE, BLACK);
            s.fill(
                x + n.len() as u16,
                y,
                (slot - n.len()) as u16 - 1,
                1,
                ' ',
                BLACK,
                TEAL,
            );
            s.text(x + n.len() as u16, y, label, BLACK, TEAL);
        }

        match &self.mode {
            Mode::Dialog(d) => draw_dialog(s, d),
            Mode::Drives { panel, cursor } => self.draw_drives(s, *panel, *cursor),
            Mode::Help => draw_help(s),
            _ => {}
        }
    }

    fn draw_panel(&mut self, s: &mut Screen, side: usize, x: u16, w: u16, list_rows: usize) {
        let active = side == self.active;
        let (fr, bg) = (AQUA, COBALT);
        let h = list_rows as u16 + 5;
        let y = 1u16;
        // Frame.
        for xx in x + 1..x + w - 1 {
            s.text(xx, y, "═", fr, bg);
            s.text(xx, y + h, "═", fr, bg);
            s.text(xx, y + h - 2, "─", fr, bg);
        }
        for yy in y + 1..y + h {
            s.text(x, yy, "║", fr, bg);
            s.text(x + w - 1, yy, "║", fr, bg);
        }
        s.text(x, y, "╔", fr, bg);
        s.text(x + w - 1, y, "╗", fr, bg);
        s.text(x, y + h, "╚", fr, bg);
        s.text(x + w - 1, y + h, "╝", fr, bg);
        s.text(x, y + h - 2, "╟", fr, bg);
        s.text(x + w - 1, y + h - 2, "╢", fr, bg);

        let inner = w as usize - 2;
        // Columns: name | size(9) | date(8) | time(5)
        let wide = inner >= 44;
        let name_w = if wide {
            inner - 26
        } else {
            inner.saturating_sub(10)
        };
        let p = &mut self.panels[side];
        p.scroll(list_rows);
        let path = clip(&self.dos.display(&p.dir), inner.saturating_sub(4));
        let title = format!(" {} ", path.trim_end());
        let tx = x + 1 + ((inner - title.chars().count()) / 2) as u16;
        let (tf, tb) = if active {
            (BLACK, TEAL)
        } else {
            (AQUA, COBALT)
        };
        s.text(tx, y, &title, tf, tb);

        // Header.
        let hy = y + 1;
        s.text(
            x + 1 + (name_w / 2).saturating_sub(2) as u16,
            hy,
            "Name",
            LEMON,
            bg,
        );
        let sx = x + 1 + name_w as u16;
        s.text(sx, hy, "│", fr, bg);
        s.text(sx + 3, hy, "Size", LEMON, bg);
        if wide {
            s.text(sx + 10, hy, "│", fr, bg);
            s.text(sx + 13, hy, "Date", LEMON, bg);
            s.text(sx + 19, hy, "│", fr, bg);
            s.text(sx + 20, hy, "Time", LEMON, bg);
        }

        if let Some(err) = &p.error {
            s.text(
                x + 2,
                hy + 1,
                &clip(&format!("Cannot read: {err}"), inner - 2),
                LEMON,
                BRICK,
            );
        }
        for row in 0..list_rows {
            let ry = hy + 1 + row as u16;
            let i = p.top + row;
            s.text(sx, ry, "│", fr, bg);
            if wide {
                s.text(sx + 10, ry, "│", fr, bg);
                s.text(sx + 19, ry, "│", fr, bg);
            }
            let Some(e) = p.entries.get(i) else { continue };
            let marked = p.marked.contains(&e.name);
            let fg = if marked {
                LEMON
            } else if e.dir {
                WHITE
            } else if e.exec {
                LIME
            } else {
                AQUA
            };
            let (fg, rbg) = if active && i == p.cursor {
                (if marked { LEMON } else { BLACK }, TEAL)
            } else {
                (fg, bg)
            };
            let name = if e.dir && e.name != ".." {
                e.name.to_uppercase()
            } else {
                e.name.clone()
            };
            let name = if e.link { format!("{name}@") } else { name };
            let size = if e.name == ".." {
                "UP--DIR".to_string()
            } else if e.dir {
                "<DIR>".into()
            } else if e.size >= 100_000_000 {
                format!("{}M", e.size >> 20)
            } else {
                e.size.to_string()
            };
            s.text(x + 1, ry, &clip(&name, name_w), fg, rbg);
            s.text(sx, ry, "│", fr, rbg);
            s.text(sx + 1, ry, &format!("{size:>9}"), fg, rbg);
            if wide {
                let (d, t) = if e.mtime > 0 {
                    date_time(e.mtime)
                } else {
                    (String::new(), String::new())
                };
                s.text(sx + 10, ry, "│", fr, rbg);
                s.text(sx + 11, ry, &format!("{d:>8}"), fg, rbg);
                s.text(sx + 19, ry, "│", fr, rbg);
                s.text(sx + 20, ry, &format!("{t:>5}"), fg, rbg);
            }
        }

        // Info line.
        let iy = y + h - 1;
        let info = if !p.marked.is_empty() {
            format!(
                "{} bytes in {} marked file(s)",
                thousands(p.marked_bytes()),
                p.marked.len()
            )
        } else {
            match p.current() {
                Some(e) if e.name == ".." => "Up to the parent directory".into(),
                Some(e) if e.dir => format!("{}  directory", e.name),
                Some(e) => format!("{}  {} bytes", e.name, thousands(e.size)),
                None => String::new(),
            }
        };
        let ic = if p.marked.is_empty() { AQUA } else { LEMON };
        s.text(x + 1, iy, &clip(&info, inner), ic, bg);
    }

    fn draw_drives(&self, s: &mut Screen, panel: usize, cursor: usize) {
        let drives: Vec<_> = self.dos.drives().cloned().collect();
        let w = 36u16;
        let h = drives.len() as u16 + 4;
        let half = (s.cols / 2) as usize;
        let x = if panel == 0 {
            half as u16 / 2
        } else {
            (half + half / 2) as u16
        }
        .saturating_sub(w / 2);
        let y = 4;
        s.dialog(x, y, w, h, "Drive", BLACK, SILVER);
        for (i, d) in drives.iter().enumerate() {
            let kind = match d.kind {
                retro_dosfs::DriveKind::Home => "Your files",
                retro_dosfs::DriveKind::Root => "System",
                retro_dosfs::DriveKind::Removable => "Removable",
            };
            let line = format!(" {}:  {:<12} {}", d.letter, clip(&d.label, 12), kind);
            let (fg, bg) = if i == cursor {
                (BLACK, TEAL)
            } else {
                (BLACK, SILVER)
            };
            s.fill(x + 2, y + 2 + i as u16, w - 4, 1, ' ', fg, bg);
            s.text(
                x + 2,
                y + 2 + i as u16,
                &clip(&line, w as usize - 4),
                fg,
                bg,
            );
        }
    }

    fn draw_viewer(&self, s: &mut Screen, v: &Viewer) {
        s.clear(SILVER, COBALT);
        s.fill(0, 0, s.cols, 1, ' ', BLACK, TEAL);
        s.text(1, 0, &format!("View: {}", v.title), BLACK, TEAL);
        let pct = if v.lines.is_empty() {
            100
        } else {
            ((v.top + (s.rows as usize - 2)).min(v.lines.len()) * 100) / v.lines.len()
        };
        let right = format!("Line {}  {pct}%", v.top + 1);
        s.text(
            s.cols.saturating_sub(right.len() as u16 + 1),
            0,
            &right,
            BLACK,
            TEAL,
        );
        for row in 0..s.rows as usize - 2 {
            if let Some(l) = v.lines.get(v.top + row) {
                s.text(0, 1 + row as u16, &clip(l, s.cols as usize), SILVER, COBALT);
            }
        }
        let y = s.rows - 1;
        s.fill(0, y, s.cols, 1, ' ', BLACK, TEAL);
        s.text(1, y, "Arrows/PgUp/PgDn Scroll   Esc Close", BLACK, TEAL);
    }
}

fn draw_dialog(s: &mut Screen, d: &Dialog) {
    let buttons: Vec<String> = d.buttons.iter().map(|b| format!("< {b} >")).collect();
    let bw: usize = buttons.iter().map(|b| b.chars().count() + 2).sum();
    let tw = d.text.iter().map(|t| t.chars().count()).max().unwrap_or(0);
    let w = ((tw + 6)
        .max(bw + 4)
        .max(if d.field.is_some() { 56 } else { 30 }) as u16)
        .min(s.cols.saturating_sub(4));
    let h = (d.text.len() + if d.field.is_some() { 2 } else { 0 } + 5) as u16;
    let x = s.cols.saturating_sub(w) / 2;
    let y = s.rows.saturating_sub(h) / 2;
    let (fg, bg) = if d.ask == Ask::Delete || d.title == "Error" {
        (WHITE, BRICK)
    } else {
        (BLACK, SILVER)
    };
    s.dialog(x, y, w, h, &d.title, fg, bg);
    let mut row = y + 2;
    for t in &d.text {
        s.text(x + 3, row, &clip(t, w as usize - 6), fg, bg);
        row += 1;
    }
    if let Some(f) = &d.field {
        let fbg = if d.focus == 0 { TEAL } else { COBALT };
        let cx = f.draw(s, x + 3, row, w - 6, WHITE, fbg);
        if d.focus == 0 {
            s.cursor = Some((cx, row));
        }
    }
    let nf = usize::from(d.field.is_some());
    let by = y + h - 2;
    let mut bx = x + (w.saturating_sub(bw as u16)) / 2;
    for (i, b) in buttons.iter().enumerate() {
        let focused = d.focus == nf + i;
        let (bf, bb) = if focused { (BLACK, TEAL) } else { (fg, bg) };
        s.text(bx, by, b, bf, bb);
        bx += b.chars().count() as u16 + 2;
    }
}

const HELP: &[&str] = &[
    "Tab          Switch panels           Enter   Open directory / view file",
    "Ins, Space   Mark a file             Bksp    Parent directory",
    "+  -  *      Mark all, none, invert  Ctrl+R  Re-read directories",
    "F2           Show hidden files       F3      View      F4  Edit",
    "F5           Copy to other panel     F6      Move or rename",
    "F7           Make directory          F8/Del  Delete",
    "F9, Alt+1/2  Change drive            F10     Quit",
    "Letters      Jump to the next name starting with that letter",
];

fn draw_help(s: &mut Screen) {
    let w = 76u16.min(s.cols - 2);
    let h = HELP.len() as u16 + 5;
    let x = s.cols.saturating_sub(w) / 2;
    let y = s.rows.saturating_sub(h) / 2;
    s.dialog(x, y, w, h, "Filer Keyboard", BLACK, SILVER);
    for (i, l) in HELP.iter().enumerate() {
        s.text(
            x + 2,
            y + 2 + i as u16,
            &clip(l, w as usize - 4),
            BLACK,
            SILVER,
        );
    }
    s.center(y + h - 2, "Press any key to continue", BLACK, SILVER);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(name: &str) -> (PathBuf, App) {
        let d = std::env::temp_dir().join(format!("retro-filer-app-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("left/docs")).unwrap();
        std::fs::create_dir_all(d.join("right")).unwrap();
        std::fs::write(d.join("left/readme.txt"), "line one\nline two\n").unwrap();
        let dos = Dos::new(&d);
        let app = App::new(dos, &d.join("left"), &d.join("right"));
        (d, app)
    }

    fn typ(a: &mut App, s: &str) {
        for c in s.chars() {
            a.key(Key::Char(c));
        }
    }

    #[test]
    fn draws_two_panels_and_fkeys() {
        let (d, mut a) = setup("draw");
        let mut s = Screen::with_size(120, 33, None);
        a.draw(&mut s);
        let t = s.dump();
        assert!(t.contains(" C:\\left ") && t.contains(" C:\\right "), "{t}");
        assert!(t.contains("UP--DIR") && t.contains("DOCS") && t.contains("readme.txt"));
        assert!(t.lines().last().unwrap().contains("5Copy"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn copy_mkdir_delete_view() {
        let (d, mut a) = setup("ops");
        a.key(Key::End); // readme.txt
        a.key(Key::F(5));
        a.key(Key::Enter); // to the other panel
        assert!(d.join("right/readme.txt").exists());
        a.key(Key::F(7));
        typ(&mut a, "NEWDIR");
        a.key(Key::Enter);
        assert!(d.join("left/NEWDIR").is_dir());
        assert_eq!(a.panels[0].current().unwrap().name, "NEWDIR");
        a.key(Key::F(8));
        a.key(Key::Enter);
        assert!(!d.join("left/NEWDIR").exists());
        a.key(Key::End);
        a.key(Key::F(3));
        assert!(matches!(&a.mode, Mode::View(v) if v.lines[1] == "line two"));
        a.key(Key::Esc);
        // Into a directory and back.
        a.key(Key::Home);
        a.key(Key::Down);
        a.key(Key::Enter);
        assert!(a.panels[0].dir.ends_with("docs"));
        a.key(Key::Backspace);
        assert_eq!(a.panels[0].current().unwrap().name, "docs");
        let _ = std::fs::remove_dir_all(d);
    }
}

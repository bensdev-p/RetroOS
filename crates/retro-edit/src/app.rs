//! EDIT's state, keys and drawing. Everything modal (menus, dialogs) is a
//! state of `App`, so the whole editor can be driven by tests without a
//! terminal.

use crate::buffer::{Buffer, Pos};
use retro_tui::keys::Mouse;
use retro_tui::{Field, Key, Screen, BLACK, BRICK, COBALT, SILVER, SLATE, TEAL, WHITE};

/// Menu and button mnemonic letters (docs/STYLE.md §3).
const MNEMONIC: u8 = BRICK;
use std::path::PathBuf;

const MENUS: &[(&str, &[(&str, &str)])] = &[
    (
        "File",
        &[
            ("New", "Ctrl+N"),
            ("Open...", "Ctrl+O"),
            ("Save", "Ctrl+S"),
            ("Save As...", ""),
            ("-", ""),
            ("Exit", "Ctrl+Q"),
        ],
    ),
    (
        "Edit",
        &[
            ("Undo", "Ctrl+Z"),
            ("-", ""),
            ("Cut", "Shift+Del"),
            ("Copy", "Ctrl+Ins"),
            ("Paste", "Shift+Ins"),
            ("Clear", "Del"),
            ("-", ""),
            ("Select All", "Ctrl+A"),
            ("Delete Line", "Ctrl+Y"),
        ],
    ),
    (
        "Search",
        &[
            ("Find...", "Ctrl+F"),
            ("Repeat Last Find", "F3"),
            ("Change...", "Ctrl+H"),
            ("-", ""),
            ("Go to Line...", "Ctrl+G"),
        ],
    ),
    ("Options", &[("Line Ends...", ""), ("Match Case", "")]),
    ("Help", &[("Keyboard", "F1"), ("About EDIT...", "")]),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum After {
    Nothing,
    Exit,
    New,
    Open,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogKind {
    Open,
    SaveAs(After),
    Find,
    Change,
    /// Confirm one replacement: (find, replace).
    ChangeAsk(String, String),
    Goto,
    Unsaved(After),
    LineEnds,
    Message,
}

pub struct Dialog {
    pub kind: DialogKind,
    pub title: String,
    pub text: Vec<String>,
    pub fields: Vec<(String, Field)>,
    pub buttons: Vec<&'static str>,
    /// 0..fields are fields, then buttons.
    pub focus: usize,
    /// No key pressed yet: typing replaces a pre-filled field.
    fresh: bool,
}

impl Dialog {
    fn new(kind: DialogKind, title: &str, text: &[&str], buttons: &[&'static str]) -> Dialog {
        Dialog {
            kind,
            title: title.into(),
            text: text.iter().map(|s| s.to_string()).collect(),
            fields: Vec::new(),
            buttons: buttons.to_vec(),
            focus: 0,
            fresh: true,
        }
    }

    fn field(mut self, label: &str, value: &str) -> Dialog {
        let mut f = Field::new(240, false);
        f.value = value.into();
        self.fields.push((label.into(), f));
        self
    }

    fn value(&self, i: usize) -> String {
        self.fields
            .get(i)
            .map(|f| f.1.value.clone())
            .unwrap_or_default()
    }
}

pub enum Mode {
    Edit,
    Menu { menu: usize, item: usize },
    Dialog(Dialog),
    Help,
}

pub struct App {
    pub buf: Buffer,
    pub cur: Pos,
    /// Display column remembered across Up/Down.
    want: usize,
    pub anchor: Option<Pos>,
    pub top: usize,
    pub left: usize,
    pub clip: String,
    pub insert: bool,
    pub find: String,
    pub match_case: bool,
    pub mode: Mode,
    pub quit: bool,
    undo: Vec<(Vec<Vec<char>>, Pos)>,
    typing: bool,
    /// Text-area size from the last draw.
    view: (usize, usize),
    pub status: Option<String>,
    dragging: bool,
}

fn title_of(b: &Buffer) -> String {
    b.path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "UNTITLED".into())
}

impl App {
    pub fn new(buf: Buffer) -> App {
        App {
            buf,
            cur: Pos::default(),
            want: 0,
            anchor: None,
            top: 0,
            left: 0,
            clip: String::new(),
            insert: true,
            find: String::new(),
            match_case: false,
            mode: Mode::Edit,
            quit: false,
            undo: Vec::new(),
            typing: false,
            view: (78, 20),
            status: None,
            dragging: false,
        }
    }

    // ------------------------------------------------------------ editing

    fn selection(&self) -> Option<(Pos, Pos)> {
        let a = self.anchor?;
        (a != self.cur).then(|| (a.min(self.cur), a.max(self.cur)))
    }

    fn snapshot(&mut self, typing: bool) {
        if !(typing && self.typing) {
            self.undo.push((self.buf.lines.clone(), self.cur));
            if self.undo.len() > 200 {
                self.undo.remove(0);
            }
        }
        self.typing = typing;
    }

    fn undo(&mut self) {
        if let Some((lines, cur)) = self.undo.pop() {
            self.buf.lines = lines;
            self.buf.modified = true;
            self.cur = self.buf.clamp(cur);
            self.anchor = None;
        }
        self.typing = false;
    }

    fn delete_selection(&mut self) -> bool {
        match self.selection() {
            Some((a, b)) => {
                self.cur = self.buf.delete(a, b);
                self.anchor = None;
                true
            }
            None => false,
        }
    }

    fn type_char(&mut self, c: char) {
        self.snapshot(c.is_alphanumeric());
        self.delete_selection();
        if !self.insert && self.cur.col < self.buf.line_len(self.cur.line) {
            self.buf
                .delete(self.cur, Pos::new(self.cur.line, self.cur.col + 1));
        }
        self.cur = self.buf.insert(self.cur, &c.to_string());
        self.want = self.disp();
    }

    fn newline(&mut self) {
        self.snapshot(false);
        self.delete_selection();
        // Keep the indentation of the line above, like EDIT.COM.
        let indent: String = self.buf.lines[self.cur.line]
            .iter()
            .take(self.cur.col)
            .take_while(|c| **c == ' ' || **c == '\t')
            .collect();
        self.cur = self.buf.insert(self.cur, &format!("\n{indent}"));
        self.want = self.disp();
    }

    fn backspace(&mut self) {
        self.snapshot(false);
        if !self.delete_selection() {
            let p = self.buf.prev(self.cur);
            self.cur = self.buf.delete(p, self.cur);
        }
        self.want = self.disp();
    }

    fn delete(&mut self) {
        self.snapshot(false);
        if !self.delete_selection() {
            let n = self.buf.next(self.cur);
            self.buf.delete(self.cur, n);
        }
    }

    fn copy(&mut self) {
        if let Some((a, b)) = self.selection() {
            self.clip = self.buf.get(a, b);
        }
    }

    fn cut(&mut self) {
        if self.selection().is_some() {
            self.snapshot(false);
            self.copy();
            self.delete_selection();
        }
    }

    fn paste(&mut self) {
        if self.clip.is_empty() {
            return;
        }
        self.snapshot(false);
        self.delete_selection();
        let clip = self.clip.clone();
        self.cur = self.buf.insert(self.cur, &clip);
        self.want = self.disp();
    }

    fn delete_line(&mut self) {
        self.snapshot(false);
        let l = self.cur.line;
        if l + 1 < self.buf.lines.len() {
            self.clip = self.buf.get(Pos::new(l, 0), Pos::new(l + 1, 0));
            self.buf.delete(Pos::new(l, 0), Pos::new(l + 1, 0));
        } else {
            self.clip = self.buf.lines[l].iter().collect();
            self.buf
                .delete(Pos::new(l, 0), Pos::new(l, self.buf.line_len(l)));
        }
        self.cur = Pos::new(self.cur.line.min(self.buf.lines.len() - 1), 0);
        self.anchor = None;
    }

    fn disp(&self) -> usize {
        self.buf.display_col(self.cur.line, self.cur.col)
    }

    /// Move the cursor; `select` extends the selection (Shift).
    fn go(&mut self, p: Pos, select: bool, keep_want: bool) {
        if select {
            self.anchor.get_or_insert(self.cur);
        } else {
            self.anchor = None;
        }
        self.cur = self.buf.clamp(p);
        if !keep_want {
            self.want = self.disp();
        }
        self.typing = false;
    }

    fn vertical(&mut self, delta: isize, select: bool) {
        let last = self.buf.lines.len() as isize - 1;
        let line = (self.cur.line as isize + delta).clamp(0, last) as usize;
        let col = self.buf.col_for_display(line, self.want);
        self.go(Pos::new(line, col), select, true);
    }

    fn find_next(&mut self, from_next: bool) -> bool {
        if self.find.is_empty() {
            return false;
        }
        let start = if from_next {
            self.buf.next(self.cur)
        } else {
            self.cur
        };
        match self.buf.find(&self.find, start, self.match_case) {
            Some(p) => {
                let end = Pos::new(p.line, p.col + self.find.chars().count());
                self.anchor = Some(p);
                self.cur = end;
                self.want = self.disp();
                true
            }
            None => {
                self.message("Find", &["Match not found."]);
                false
            }
        }
    }

    fn message(&mut self, title: &str, text: &[&str]) {
        self.mode = Mode::Dialog(Dialog::new(DialogKind::Message, title, text, &["OK"]));
    }

    // -------------------------------------------------------------- files

    fn save_or_ask(&mut self, then: After) {
        if self.buf.path.is_none() {
            let d = Dialog::new(DialogKind::SaveAs(then), "Save As", &[], &["OK", "Cancel"])
                .field("File Name:", "");
            self.mode = Mode::Dialog(d);
            return;
        }
        match self.buf.save() {
            Ok(()) => {
                self.status = Some(format!("Saved {}", title_of(&self.buf)));
                self.after(then);
            }
            Err(e) => self.message("Error", &["Could not save the file:", &e.to_string()]),
        }
    }

    /// Ask to save unsaved changes before `then`.
    fn guard(&mut self, then: After) {
        if self.buf.modified {
            let d = Dialog::new(
                DialogKind::Unsaved(then),
                "EDIT",
                &["Loaded file is not saved. Save it now?"],
                &["Yes", "No", "Cancel"],
            );
            self.mode = Mode::Dialog(d);
        } else {
            self.after(then);
        }
    }

    fn after(&mut self, then: After) {
        match then {
            After::Nothing => {}
            After::Exit => self.quit = true,
            After::New => self.load(Buffer::default()),
            After::Open => {
                let d = Dialog::new(DialogKind::Open, "Open", &[], &["OK", "Cancel"])
                    .field("File Name:", "");
                self.mode = Mode::Dialog(d);
            }
        }
    }

    fn load(&mut self, b: Buffer) {
        *self = App {
            clip: std::mem::take(&mut self.clip),
            find: std::mem::take(&mut self.find),
            match_case: self.match_case,
            insert: self.insert,
            view: self.view,
            ..App::new(b)
        };
    }

    pub fn open_path(&mut self, name: &str) {
        let path = PathBuf::from(name);
        match Buffer::open(&path) {
            Ok(b) => self.load(b),
            Err(e) => self.message("Error", &["Could not open the file:", &e.to_string()]),
        }
    }

    // --------------------------------------------------------------- keys

    pub fn key(&mut self, k: Key) {
        self.status = None;
        match std::mem::replace(&mut self.mode, Mode::Edit) {
            Mode::Edit => self.edit_key(k),
            Mode::Menu { menu, item } => self.menu_key(menu, item, k),
            Mode::Dialog(d) => self.dialog_key(d, k),
            Mode::Help => {}
        }
        self.scroll_to_cursor();
    }

    fn edit_key(&mut self, k: Key) {
        let (m, base) = k.split();
        let sel = m.shift;
        let base = base.clone();
        match (m.ctrl, base) {
            (_, Key::Mouse(ms)) => self.mouse(ms),
            (false, Key::Char(c)) if !c.is_control() => self.type_char(c),
            (false, Key::Tab) => self.type_char('\t'),
            (_, Key::Enter) => self.newline(),
            (_, Key::Backspace) => self.backspace(),
            (false, Key::Delete) if sel => self.cut(),
            (false, Key::Delete) => self.delete(),
            (true, Key::Insert) => self.copy(),
            (false, Key::Insert) if sel => self.paste(),
            (false, Key::Insert) => self.insert = !self.insert,
            (false, Key::Left) => {
                let p = self.buf.prev(self.cur);
                self.go(p, sel, false)
            }
            (false, Key::Right) => {
                let p = self.buf.next(self.cur);
                self.go(p, sel, false)
            }
            (true, Key::Left) => {
                let p = self.buf.word_left(self.cur);
                self.go(p, sel, false)
            }
            (true, Key::Right) => {
                let p = self.buf.word_right(self.cur);
                self.go(p, sel, false)
            }
            (_, Key::Up) => self.vertical(-1, sel),
            (_, Key::Down) => self.vertical(1, sel),
            (_, Key::PgUp) => self.vertical(-(self.view.1 as isize - 1).max(1), sel),
            (_, Key::PgDn) => self.vertical((self.view.1 as isize - 1).max(1), sel),
            (false, Key::Home) => {
                // Home toggles between the first non-blank and column 0.
                let first = self.buf.lines[self.cur.line]
                    .iter()
                    .position(|c| !c.is_whitespace())
                    .unwrap_or(0);
                let col = if self.cur.col == first { 0 } else { first };
                self.go(Pos::new(self.cur.line, col), sel, false)
            }
            (false, Key::End) => {
                let l = self.cur.line;
                self.go(Pos::new(l, self.buf.line_len(l)), sel, false)
            }
            (true, Key::Home) => self.go(Pos::default(), sel, false),
            (true, Key::End) => {
                let l = self.buf.lines.len() - 1;
                self.go(Pos::new(l, self.buf.line_len(l)), sel, false)
            }
            (_, Key::Ctrl('a')) => {
                let l = self.buf.lines.len() - 1;
                self.anchor = Some(Pos::default());
                self.cur = Pos::new(l, self.buf.line_len(l));
            }
            (_, Key::Ctrl('c')) => self.copy(),
            (_, Key::Ctrl('x')) => self.cut(),
            (_, Key::Ctrl('v')) => self.paste(),
            (_, Key::Ctrl('z')) => self.undo(),
            (_, Key::Ctrl('y')) => self.delete_line(),
            (_, Key::Ctrl('s')) => self.save_or_ask(After::Nothing),
            (_, Key::Ctrl('q')) => self.guard(After::Exit),
            (_, Key::Ctrl('n')) => self.guard(After::New),
            (_, Key::Ctrl('o')) => self.guard(After::Open),
            (_, Key::Ctrl('f')) => self.command("Search", "Find..."),
            (_, Key::Ctrl('h')) => self.command("Search", "Change..."),
            (_, Key::Ctrl('g')) => self.command("Search", "Go to Line..."),
            (_, Key::F(1)) => self.mode = Mode::Help,
            (_, Key::F(3)) => {
                self.find_next(true);
            }
            (_, Key::F(10)) => self.mode = Mode::Menu { menu: 0, item: 0 },
            (_, Key::Alt(c)) => {
                if let Some(i) = MENUS
                    .iter()
                    .position(|(n, _)| n.to_ascii_lowercase().starts_with(c))
                {
                    self.mode = Mode::Menu { menu: i, item: 0 };
                }
            }
            (_, Key::Esc) => self.anchor = None,
            _ => {}
        }
    }

    fn mouse(&mut self, m: Mouse) {
        let (x, y) = (m.x as usize, m.y as usize);
        match m.button {
            64 => self.top = self.top.saturating_sub(3),
            65 => {
                self.top = (self.top + 3).min(self.buf.lines.len().saturating_sub(1));
            }
            0 | 32 if m.pressed => {
                if y == 0 && m.button == 0 {
                    if let Some(i) = menu_at(x) {
                        self.mode = Mode::Menu { menu: i, item: 0 };
                    }
                    return;
                }
                // Inside the text area: place the cursor; dragging selects.
                if (1..=self.view.0).contains(&x) && (2..2 + self.view.1).contains(&y) {
                    let line = (self.top + y - 2).min(self.buf.lines.len() - 1);
                    let col = self.buf.col_for_display(line, self.left + x - 1);
                    let drag = m.button == 32 && self.dragging;
                    self.go(Pos::new(line, col), drag, false);
                    self.dragging = true;
                    if !drag {
                        self.anchor = Some(self.cur);
                    }
                }
            }
            _ if !m.pressed => {
                self.dragging = false;
                if self.anchor == Some(self.cur) {
                    self.anchor = None;
                }
            }
            _ => {}
        }
    }

    fn menu_key(&mut self, menu: usize, item: usize, k: Key) {
        let items = MENUS[menu].1;
        let step = |i: usize, fwd: bool| {
            let n = items.len();
            let mut j = i;
            loop {
                j = if fwd { (j + 1) % n } else { (j + n - 1) % n };
                if items[j].0 != "-" {
                    return j;
                }
            }
        };
        let n = MENUS.len();
        self.mode = match k {
            Key::Esc | Key::F(10) => Mode::Edit,
            Key::Left => Mode::Menu {
                menu: (menu + n - 1) % n,
                item: 0,
            },
            Key::Right => Mode::Menu {
                menu: (menu + 1) % n,
                item: 0,
            },
            Key::Up => Mode::Menu {
                menu,
                item: step(item, false),
            },
            Key::Down => Mode::Menu {
                menu,
                item: step(item, true),
            },
            Key::Enter => {
                self.command(MENUS[menu].0, items[item].0);
                return;
            }
            Key::Char(c) | Key::Alt(c) => {
                if let Some((label, _)) = items
                    .iter()
                    .find(|(l, _)| l.to_ascii_lowercase().starts_with(c.to_ascii_lowercase()))
                {
                    self.command(MENUS[menu].0, label);
                    return;
                }
                Mode::Menu { menu, item }
            }
            Key::Mouse(m) if m.pressed && m.button == 0 => {
                let (mx, w) = menu_box(menu);
                let (x, y) = (m.x as usize, m.y as usize);
                if y == 0 {
                    match menu_at(x) {
                        Some(i) => Mode::Menu { menu: i, item: 0 },
                        None => Mode::Edit,
                    }
                } else if (mx..mx + w).contains(&x) && (2..2 + items.len()).contains(&y) {
                    if items[y - 2].0 != "-" {
                        self.command(MENUS[menu].0, items[y - 2].0);
                        return;
                    }
                    Mode::Menu { menu, item }
                } else {
                    Mode::Edit
                }
            }
            _ => Mode::Menu { menu, item },
        };
    }

    /// Run a menu command.
    fn command(&mut self, menu: &str, item: &str) {
        self.mode = Mode::Edit;
        match (menu, item) {
            ("File", "New") => self.guard(After::New),
            ("File", "Open...") => self.guard(After::Open),
            ("File", "Save") => self.save_or_ask(After::Nothing),
            ("File", "Save As...") => {
                let name = self
                    .buf
                    .path
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let d = Dialog::new(
                    DialogKind::SaveAs(After::Nothing),
                    "Save As",
                    &[],
                    &["OK", "Cancel"],
                )
                .field("File Name:", &name);
                self.mode = Mode::Dialog(d);
            }
            ("File", "Exit") => self.guard(After::Exit),
            ("Edit", "Undo") => self.undo(),
            ("Edit", "Cut") => self.cut(),
            ("Edit", "Copy") => self.copy(),
            ("Edit", "Paste") => self.paste(),
            ("Edit", "Clear") => {
                if self.selection().is_some() {
                    self.snapshot(false);
                    self.delete_selection();
                }
            }
            ("Edit", "Select All") => self.edit_key(Key::Ctrl('a')),
            ("Edit", "Delete Line") => self.delete_line(),
            ("Search", "Find...") => {
                let seed = self
                    .selection()
                    .map(|(a, b)| self.buf.get(a, b))
                    .filter(|s| !s.contains('\n'))
                    .unwrap_or_else(|| self.find.clone());
                let d = Dialog::new(DialogKind::Find, "Find", &[], &["OK", "Cancel"])
                    .field("Find What:", &seed);
                self.mode = Mode::Dialog(d);
            }
            ("Search", "Repeat Last Find") => {
                self.find_next(true);
            }
            ("Search", "Change...") => {
                let d = Dialog::new(
                    DialogKind::Change,
                    "Change",
                    &[],
                    &["Find and Verify", "Change All", "Cancel"],
                )
                .field("Find What:", &self.find.clone())
                .field("Change To:", "");
                self.mode = Mode::Dialog(d);
            }
            ("Search", "Go to Line...") => {
                let d = Dialog::new(DialogKind::Goto, "Go to Line", &[], &["OK", "Cancel"])
                    .field("Line Number:", "");
                self.mode = Mode::Dialog(d);
            }
            ("Options", "Line Ends...") => {
                let now = if self.buf.crlf {
                    "DOS (CR LF)"
                } else {
                    "Unix (LF)"
                };
                let d = Dialog::new(
                    DialogKind::LineEnds,
                    "Line Ends",
                    &[&format!("This file uses {now} line ends."), "Save it with:"],
                    &["DOS (CR LF)", "Unix (LF)", "Cancel"],
                );
                self.mode = Mode::Dialog(d);
            }
            ("Options", "Match Case") => {
                self.match_case = !self.match_case;
                self.status = Some(format!(
                    "Find: case {}",
                    if self.match_case {
                        "must match"
                    } else {
                        "ignored"
                    }
                ));
            }
            ("Help", "Keyboard") => self.mode = Mode::Help,
            ("Help", "About EDIT...") => self.message(
                "About",
                &[
                    "RetroOS EDIT Version 1.0",
                    "(C)Copyright Phosphor Computer Company 1987-1993.",
                ],
            ),
            _ => {}
        }
    }

    fn dialog_key(&mut self, mut d: Dialog, k: Key) {
        let total = d.fields.len() + d.buttons.len();
        match k {
            Key::Esc => {
                let cancel = d.buttons.iter().position(|b| *b == "Cancel");
                match cancel {
                    Some(i) => self.dialog_done(d, i),
                    None => self.dialog_done(d, usize::MAX),
                }
                return;
            }
            Key::Tab | Key::Down => d.focus = (d.focus + 1) % total,
            Key::BackTab | Key::Up => d.focus = (d.focus + total - 1) % total,
            Key::Left | Key::Right if d.focus >= d.fields.len() => {
                let nb = d.buttons.len();
                let b = d.focus - d.fields.len();
                let b = if k == Key::Left {
                    (b + nb - 1) % nb
                } else {
                    (b + 1) % nb
                };
                d.focus = d.fields.len() + b;
            }
            Key::Enter => {
                // Enter in a field means the first button (OK).
                let b = d.focus.saturating_sub(d.fields.len());
                let b = if d.focus < d.fields.len() { 0 } else { b };
                self.dialog_done(d, b);
                return;
            }
            Key::Char(c) if d.focus >= d.fields.len() => {
                // Button mnemonics.
                if let Some(i) = d
                    .buttons
                    .iter()
                    .position(|b| b.to_ascii_lowercase().starts_with(c.to_ascii_lowercase()))
                {
                    self.dialog_done(d, i);
                    return;
                }
            }
            k => {
                if let Some((_, f)) = d.fields.get_mut(d.focus) {
                    if d.fresh && matches!(k, Key::Char(_)) {
                        f.value.clear();
                    }
                    f.edit(&k);
                }
            }
        }
        d.fresh = false;
        self.mode = Mode::Dialog(d);
    }

    /// A dialog closed with button `b` (usize::MAX = dismissed).
    fn dialog_done(&mut self, d: Dialog, b: usize) {
        let label = d.buttons.get(b).copied().unwrap_or("Cancel");
        if label == "Cancel" {
            return;
        }
        match d.kind.clone() {
            DialogKind::Message => {}
            DialogKind::Open => {
                let name = d.value(0);
                if !name.trim().is_empty() {
                    self.open_path(name.trim());
                }
            }
            DialogKind::SaveAs(then) => {
                let name = d.value(0);
                if name.trim().is_empty() {
                    self.mode = Mode::Dialog(d);
                    return;
                }
                self.buf.path = Some(PathBuf::from(name.trim()));
                self.save_or_ask(then);
            }
            DialogKind::Unsaved(then) => match label {
                "Yes" => self.save_or_ask(then),
                "No" => self.after(then),
                _ => {}
            },
            DialogKind::Find => {
                self.find = d.value(0);
                self.find_next(false);
            }
            DialogKind::Goto => {
                if let Ok(n) = d.value(0).trim().parse::<usize>() {
                    self.go(Pos::new(n.saturating_sub(1), 0), false, false);
                }
            }
            DialogKind::LineEnds => {
                let crlf = label.starts_with("DOS");
                if crlf != self.buf.crlf {
                    self.buf.crlf = crlf;
                    self.buf.modified = true;
                }
            }
            DialogKind::Change => {
                self.find = d.value(0);
                let to = d.value(1);
                if label == "Change All" {
                    self.change_all(&to);
                } else {
                    self.change_next(to, false);
                }
            }
            DialogKind::ChangeAsk(_, to) => match label {
                "Change" => {
                    self.snapshot(false);
                    self.delete_selection();
                    self.cur = self.buf.insert(self.cur, &to);
                    self.change_next(to, true);
                }
                "Skip" => self.change_next(to, true),
                _ => {}
            },
        }
    }

    fn change_next(&mut self, to: String, from_next: bool) {
        let start = if from_next {
            self.cur
        } else {
            self.buf.clamp(self.cur)
        };
        let Some(p) = self.buf.find(&self.find, start, self.match_case) else {
            self.message("Change", &["Change complete."]);
            return;
        };
        self.anchor = Some(p);
        self.cur = Pos::new(p.line, p.col + self.find.chars().count());
        self.scroll_to_cursor();
        let d = Dialog::new(
            DialogKind::ChangeAsk(self.find.clone(), to),
            "Change",
            &["Change this occurrence?"],
            &["Change", "Skip", "Cancel"],
        );
        self.mode = Mode::Dialog(d);
    }

    fn change_all(&mut self, to: &str) {
        if self.find.is_empty() {
            return;
        }
        self.snapshot(false);
        let mut n = 0;
        let mut p = Pos::default();
        let len = self.find.chars().count();
        let to_len = to.chars().count();
        // Scan forward without wrapping.
        while let Some(hit) = self.buf.find(&self.find, p, self.match_case) {
            if hit < p || n > 100_000 {
                break;
            }
            self.buf.delete(hit, Pos::new(hit.line, hit.col + len));
            let end = self.buf.insert(hit, to);
            p = if to_len == 0 && end == hit { hit } else { end };
            n += 1;
            if p.line >= self.buf.lines.len() {
                break;
            }
            // Stop when the search would wrap back to the start.
            if self
                .buf
                .find(&self.find, p, self.match_case)
                .is_some_and(|h| h < p)
            {
                break;
            }
        }
        self.anchor = None;
        self.cur = self.buf.clamp(self.cur);
        let msg = format!("{n} occurrence(s) changed.");
        self.message("Change", &[&msg]);
    }

    fn scroll_to_cursor(&mut self) {
        let (w, h) = self.view;
        if self.cur.line < self.top {
            self.top = self.cur.line;
        } else if self.cur.line >= self.top + h {
            self.top = self.cur.line + 1 - h;
        }
        let d = self.disp();
        if d < self.left {
            self.left = d;
        } else if d >= self.left + w {
            self.left = d + 1 - w;
        }
    }

    // ------------------------------------------------------------ drawing

    pub fn draw(&mut self, s: &mut Screen) {
        let (cols, rows) = (s.cols as usize, s.rows as usize);
        let w = cols.saturating_sub(2).max(1);
        let h = rows.saturating_sub(4).max(1);
        self.view = (w, h);
        s.clear(SILVER, COBALT);
        s.cursor = None;
        s.block_cursor = !self.insert;

        // Menu bar.
        s.fill(0, 0, s.cols, 1, ' ', BLACK, SILVER);
        let open = match self.mode {
            Mode::Menu { menu, .. } => Some(menu),
            _ => None,
        };
        let mut x = 2u16;
        for (i, (name, _)) in MENUS.iter().enumerate() {
            let (fg, bg) = if open == Some(i) {
                (SILVER, BLACK)
            } else {
                (BLACK, SILVER)
            };
            if i == MENUS.len() - 1 {
                x = s.cols.saturating_sub(name.len() as u16 + 3);
            }
            s.text(x, 0, &format!(" {name} "), fg, bg);
            s.text(
                x + 1,
                0,
                &name[..1],
                if open == Some(i) { WHITE } else { MNEMONIC },
                bg,
            );
            x += name.len() as u16 + 2;
        }

        // Window frame with the file name, scroll bars.
        let title = format!(" {} ", title_of(&self.buf));
        for xx in 0..cols as u16 {
            s.text(xx, 1, "─", SILVER, COBALT);
            s.text(xx, rows as u16 - 2, "─", SILVER, COBALT);
        }
        s.text(0, 1, "┌", SILVER, COBALT);
        s.text(cols as u16 - 1, 1, "┐", SILVER, COBALT);
        s.text(0, rows as u16 - 2, "└", SILVER, COBALT);
        s.text(cols as u16 - 1, rows as u16 - 2, "┘", SILVER, COBALT);
        let tx = (cols.saturating_sub(title.chars().count()) / 2) as u16;
        s.text(tx, 1, &title, COBALT, SILVER);
        for yy in 2..2 + h as u16 {
            s.text(0, yy, "│", SILVER, COBALT);
        }
        self.scrollbars(s, w, h);

        // Text.
        let sel = self.selection();
        for row in 0..h {
            let line = self.top + row;
            if line >= self.buf.lines.len() {
                break;
            }
            let chars = &self.buf.lines[line];
            let mut d = 0usize;
            for (i, c) in chars.iter().enumerate() {
                let width = if *c == '\t' { (d / 8 + 1) * 8 - d } else { 1 };
                let in_sel = sel.is_some_and(|(a, b)| {
                    let p = Pos::new(line, i);
                    p >= a && p < b
                });
                let (fg, bg) = if in_sel {
                    (COBALT, SILVER)
                } else {
                    (SILVER, COBALT)
                };
                for k in 0..width {
                    let dc = d + k;
                    if dc >= self.left && dc < self.left + w {
                        let ch = if *c == '\t' || c.is_control() {
                            ' '
                        } else {
                            *c
                        };
                        s.text(
                            (1 + dc - self.left) as u16,
                            (2 + row) as u16,
                            &ch.to_string(),
                            fg,
                            bg,
                        );
                    }
                }
                d += width;
            }
            // A selected line end shows as one highlighted cell.
            if sel.is_some_and(|(a, b)| {
                Pos::new(line, chars.len()) >= a && Pos::new(line, chars.len()) < b
            }) && d >= self.left
                && d < self.left + w
            {
                s.text(
                    (1 + d - self.left) as u16,
                    (2 + row) as u16,
                    " ",
                    COBALT,
                    SILVER,
                );
            }
        }

        // Status bar.
        let y = rows as u16 - 1;
        s.fill(0, y, s.cols, 1, ' ', BLACK, TEAL);
        let left = self
            .status
            .clone()
            .unwrap_or_else(|| "F1=Help  F10=Menu  Ctrl+S=Save  Ctrl+Q=Exit".into());
        s.text(1, y, &left, BLACK, TEAL);
        let right = format!(
            "{}{} {:>5}:{:<3}",
            if self.buf.modified { "* " } else { "" },
            if self.insert { "   " } else { "OVR" },
            self.cur.line + 1,
            self.disp() + 1
        );
        s.text(
            s.cols.saturating_sub(right.chars().count() as u16 + 1),
            y,
            &right,
            BLACK,
            TEAL,
        );

        let cx = self.disp().saturating_sub(self.left);
        if self.cur.line >= self.top && self.cur.line < self.top + h && cx < w {
            s.cursor = Some(((1 + cx) as u16, (2 + self.cur.line - self.top) as u16));
        }

        match &self.mode {
            Mode::Menu { menu, item } => draw_menu(s, *menu, *item),
            Mode::Dialog(d) => draw_dialog(s, d),
            Mode::Help => draw_help(s),
            Mode::Edit => {}
        }
    }

    fn scrollbars(&self, s: &mut Screen, w: usize, h: usize) {
        let (cols, rows) = (s.cols, s.rows);
        let x = cols - 1;
        s.text(x, 2, "▲", BLACK, SILVER);
        s.text(x, 1 + h as u16, "▼", BLACK, SILVER);
        for yy in 3..1 + h as u16 {
            s.text(x, yy, "░", BLACK, SILVER);
        }
        let n = self.buf.lines.len();
        if h > 3 {
            let span = h - 3;
            let pos = if n <= 1 {
                0
            } else {
                (self.top * span) / (n - 1).max(1)
            };
            s.text(x, 3 + pos.min(span) as u16, "█", BLACK, SILVER);
        }
        let y = rows - 2;
        if w > 4 {
            s.text(1, y, "◄", BLACK, SILVER);
            s.text(w as u16, y, "►", BLACK, SILVER);
            for xx in 2..w as u16 {
                s.text(xx, y, "░", BLACK, SILVER);
            }
            let longest = 256usize.max(self.left + w);
            let span = w - 3;
            let pos = (self.left * span) / longest;
            s.text(2 + pos.min(span) as u16, y, "█", BLACK, SILVER);
        }
    }
}

/// Which menu title is at column `x` of the menu bar.
fn menu_at(x: usize) -> Option<usize> {
    (0..MENUS.len()).find(|&i| {
        let (start, _) = title_span(i);
        (start..start + MENUS[i].0.len() + 2).contains(&x)
    })
}

fn title_span(i: usize) -> (usize, usize) {
    let mut x = 2;
    for (j, (name, _)) in MENUS.iter().enumerate() {
        if j == i {
            return (x, name.len() + 2);
        }
        x += name.len() + 2;
    }
    (x, 0)
}

/// Left edge and width of an open menu's box.
fn menu_box(menu: usize) -> (usize, usize) {
    let items = MENUS[menu].1;
    let w = items
        .iter()
        .map(|(l, k)| l.len() + k.len() + 6)
        .max()
        .unwrap_or(10);
    let (x, _) = title_span(menu);
    (x, w)
}

fn draw_menu(s: &mut Screen, menu: usize, item: usize) {
    let items = MENUS[menu].1;
    let (mut x, w) = menu_box(menu);
    if menu == MENUS.len() - 1 {
        x = (s.cols as usize).saturating_sub(w + 2);
    }
    let (x, w) = (x as u16, w as u16);
    let h = items.len() as u16 + 2;
    s.fill(x, 1, w, h, ' ', BLACK, SILVER);
    for xx in x + 1..x + w - 1 {
        s.text(xx, 1, "─", BLACK, SILVER);
        s.text(xx, h, "─", BLACK, SILVER);
    }
    for yy in 2..h {
        s.text(x, yy, "│", BLACK, SILVER);
        s.text(x + w - 1, yy, "│", BLACK, SILVER);
    }
    s.text(x, 1, "┌", BLACK, SILVER);
    s.text(x + w - 1, 1, "┐", BLACK, SILVER);
    s.text(x, h, "└", BLACK, SILVER);
    s.text(x + w - 1, h, "┘", BLACK, SILVER);
    for (i, (label, keys)) in items.iter().enumerate() {
        let y = 2 + i as u16;
        if *label == "-" {
            s.text(x, y, "├", BLACK, SILVER);
            for xx in x + 1..x + w - 1 {
                s.text(xx, y, "─", BLACK, SILVER);
            }
            s.text(x + w - 1, y, "┤", BLACK, SILVER);
            continue;
        }
        let (fg, bg) = if i == item {
            (SILVER, BLACK)
        } else {
            (BLACK, SILVER)
        };
        s.fill(x + 1, y, w - 2, 1, ' ', fg, bg);
        s.text(x + 2, y, label, fg, bg);
        s.text(
            x + 2,
            y,
            &label[..1],
            if i == item { WHITE } else { MNEMONIC },
            bg,
        );
        s.text(x + w - 2 - keys.len() as u16, y, keys, fg, bg);
    }
    // Shadow.
    for yy in 2..=h + 1 {
        shade(s, x + w, yy);
        shade(s, x + w + 1, yy);
    }
    for xx in x + 2..x + w + 2 {
        shade(s, xx, h + 1);
    }
}

fn shade(s: &mut Screen, x: u16, y: u16) {
    if let Some(mut c) = s.get(x, y) {
        c.fg = SLATE;
        c.bg = BLACK;
        s.set(x, y, c);
    }
}

fn draw_dialog(s: &mut Screen, d: &Dialog) {
    let buttons: Vec<String> = d.buttons.iter().map(|b| format!("< {b} >")).collect();
    let bw: usize = buttons.iter().map(|b| b.chars().count() + 2).sum();
    let tw = d.text.iter().map(|t| t.chars().count()).max().unwrap_or(0);
    let w = (tw + 6)
        .max(bw + 4)
        .max(if d.fields.is_empty() { 30 } else { 56 }) as u16;
    let w = w.min(s.cols.saturating_sub(4));
    let h = (d.text.len() + d.fields.len() * 2 + 5) as u16;
    let x = s.cols.saturating_sub(w) / 2;
    let y = s.rows.saturating_sub(h) / 2;
    s.dialog(x, y, w, h, &d.title, BLACK, SILVER);
    let mut row = y + 2;
    for t in &d.text {
        s.text(x + 3, row, t, BLACK, SILVER);
        row += 1;
    }
    for (i, (label, f)) in d.fields.iter().enumerate() {
        s.text(x + 3, row, label, BLACK, SILVER);
        let fx = x + 4 + label.chars().count() as u16;
        let fw = (x + w).saturating_sub(fx + 3);
        let bg = if d.focus == i { TEAL } else { COBALT };
        let cx = f.draw(s, fx, row, fw, WHITE, bg);
        if d.focus == i {
            s.cursor = Some((cx, row));
        }
        row += 2;
    }
    if d.fields.is_empty() {
        s.cursor = None;
    }
    let row = y + h - 2;
    let mut bx = x + (w.saturating_sub(bw as u16)) / 2;
    for (i, b) in buttons.iter().enumerate() {
        let focused = d.focus == d.fields.len() + i;
        let (fg, bg) = if focused {
            (BLACK, TEAL)
        } else {
            (BLACK, SILVER)
        };
        s.text(bx, row, b, fg, bg);
        s.text(bx + 2, row, &d.buttons[i][..1], MNEMONIC, bg);
        bx += b.chars().count() as u16 + 2;
    }
}

const HELP: &[&str] = &[
    "Cursor     Arrows, Home, End, PgUp, PgDn, Ctrl+Home, Ctrl+End",
    "Words      Ctrl+Left, Ctrl+Right",
    "Select     Shift + any cursor key, or drag with the mouse",
    "Clipboard  Shift+Del cut, Ctrl+Ins copy, Shift+Ins paste",
    "           (Ctrl+X, Ctrl+C and Ctrl+V work too)",
    "Undo       Ctrl+Z            Delete line  Ctrl+Y",
    "Find       Ctrl+F, F3 again  Change       Ctrl+H",
    "Go to line Ctrl+G            Insert/Overtype  Ins",
    "Files      Ctrl+N new, Ctrl+O open, Ctrl+S save, Ctrl+Q exit",
    "Menus      F10 or Alt + the highlighted letter",
];

fn draw_help(s: &mut Screen) {
    let w = 68u16.min(s.cols - 2);
    let h = HELP.len() as u16 + 5;
    let x = s.cols.saturating_sub(w) / 2;
    let y = s.rows.saturating_sub(h) / 2;
    s.dialog(x, y, w, h, "EDIT Keyboard", BLACK, SILVER);
    for (i, l) in HELP.iter().enumerate() {
        s.text(x + 3, y + 2 + i as u16, l, BLACK, SILVER);
    }
    s.center(y + h - 2, "Press any key to continue", BLACK, SILVER);
    s.cursor = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(text: &str) -> App {
        App::new(Buffer::from_text(text))
    }

    fn typ(a: &mut App, s: &str) {
        for c in s.chars() {
            a.key(if c == '\n' { Key::Enter } else { Key::Char(c) });
        }
    }

    #[test]
    fn typing_selection_clipboard_undo() {
        let mut a = app("");
        typ(&mut a, "    hello\nworld");
        assert_eq!(a.buf.text(), "    hello\n    world\n");
        a.key(Key::Home); // first non-blank
        a.key(Key::shift(Key::End));
        a.key(Key::Ctrl('x'));
        assert_eq!(a.clip, "world");
        a.key(Key::Up);
        a.key(Key::End);
        a.key(Key::shift(Key::Insert));
        assert_eq!(a.buf.text(), "    helloworld\n    \n");
        a.key(Key::Ctrl('z'));
        assert_eq!(a.buf.text(), "    hello\n    \n");
    }

    #[test]
    fn find_change_goto() {
        let mut a = app("one two one\nthree one\n");
        a.key(Key::Ctrl('h'));
        typ(&mut a, "one");
        a.key(Key::Tab);
        typ(&mut a, "1");
        a.key(Key::Tab);
        a.key(Key::Tab); // "Change All"
        a.key(Key::Enter);
        assert_eq!(a.buf.text(), "1 two 1\nthree 1\n");
        assert!(matches!(&a.mode, Mode::Dialog(d) if d.text[0].starts_with("3 ")));
        a.key(Key::Enter);
        a.key(Key::Ctrl('g'));
        typ(&mut a, "2");
        a.key(Key::Enter);
        assert_eq!(a.cur, Pos::new(1, 0));
        a.key(Key::Ctrl('f'));
        typ(&mut a, "TWO");
        a.key(Key::Enter);
        assert_eq!(a.anchor, Some(Pos::new(0, 2)));
    }

    #[test]
    fn unsaved_guard_and_save_as() {
        let dir = std::env::temp_dir().join(format!("retro-edit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("NOTES.TXT");
        let mut a = app("");
        typ(&mut a, "hi");
        a.key(Key::Ctrl('q'));
        assert!(matches!(&a.mode, Mode::Dialog(d) if d.kind == DialogKind::Unsaved(After::Exit)));
        a.key(Key::Enter); // Yes → Save As
        typ(&mut a, &path.to_string_lossy());
        a.key(Key::Enter);
        assert!(a.quit);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hi\n");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn draws_like_edit_com() {
        let mut a = app("Hello, world.\n");
        a.buf.path = Some("HELLO.TXT".into());
        let mut s = Screen::with_size(80, 25, None);
        a.draw(&mut s);
        let d = s.dump();
        let lines: Vec<&str> = d.lines().collect();
        assert!(lines[0].contains("File") && lines[0].contains("Help"));
        assert!(lines[1].contains(" HELLO.TXT "));
        assert!(lines[2].starts_with("│Hello, world."));
        assert!(lines[24].contains("F1=Help") && lines[24].contains("1:1"));
        assert_eq!(s.cursor, Some((1, 2)));
        a.key(Key::Alt('f'));
        a.draw(&mut s);
        assert!(s.dump().contains("Save As..."));
        a.key(Key::Down);
        a.key(Key::Esc);
        assert!(matches!(a.mode, Mode::Edit));
    }
}

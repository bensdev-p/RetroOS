//! retro-bar — the System Bar of the RetroOS Desktop (docs/DESIGN.md §9.1,
//! docs/STYLE.md §4): a 20-pixel White bar along the top of the screen with
//! the ◆ Phosphor menu, Programs, Window, and the clock.
//!
//! Station starts it with the Desktop and shows it only there. Menus are
//! popups with a grab (a click elsewhere closes them); keys: ↑ ↓ Enter Esc
//! and the underlined letters.

mod clock;

use retro_gfx::palette::{BLACK, SILVER, SKY, WHITE};
use retro_gfx::widgets::{draw_menu, menu_hit, menu_layout, push_button, text_width, MenuItem};
use retro_gfx::{font::lantern_12, Canvas};
use retro_ui::station;
use retro_ui::{Event, Program, Rect, SurfaceId, Ui};
use std::time::Duration;

const HEIGHT: i32 = 20;
const BTN_LEFT: u32 = 0x110;
const CLOCK_TIMER: u32 = 1;

/// What a menu item does.
#[derive(Clone, Debug, PartialEq)]
enum Cmd {
    None,
    DosPrompt,
    Launch(String, Vec<String>),
    /// Open the Programs submenu of group `n`.
    Group(usize),
    Switch(u64),
    Restart,
    ShutDown,
    ExitToDos,
}

struct Menu {
    /// Which title on the bar (or None for a submenu).
    title: Option<usize>,
    surface: SurfaceId,
    items: Vec<(MenuItem, Cmd)>,
    sel: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ask {
    Restart,
    ShutDown,
    ExitToDos,
}

struct Dialog {
    surface: SurfaceId,
    ask: Ask,
    /// "Close all programs first" (Exit to DOS).
    close_all: bool,
    pressed: Option<usize>,
}

/// A Programs menu group: its name and its programs (name, argv).
type Group = (String, Vec<(String, Vec<String>)>);

struct Bar {
    bar: Option<SurfaceId>,
    menus: Vec<Menu>,
    dialog: Option<Dialog>,
    /// Programs menu groups: name and (label, argv) entries.
    groups: Vec<Group>,
}

const TITLES: [&str; 3] = ["◆", "Programs", "Window"];

/// Title positions on the bar (x, width), art pixels.
fn title_rects() -> Vec<(i32, i32)> {
    let mut x = 6;
    TITLES
        .iter()
        .map(|t| {
            let w = text_width(t) + 14;
            let r = (x, w);
            x += w;
            r
        })
        .collect()
}

fn ask_text(a: Ask) -> (&'static str, &'static str) {
    match a {
        Ask::Restart => ("Restart", "Restart the computer?"),
        Ask::ShutDown => ("Shut Down", "Shut down the computer?"),
        Ask::ExitToDos => ("Exit to DOS", "Leave the Desktop for RetroDOS?"),
    }
}

// Dialog layout, art pixels.
const DLG_W: i32 = 260;
const DLG_H: i32 = 96;
const OK: Rect = Rect {
    x: 80,
    y: 64,
    w: 56,
    h: 22,
};
const CANCEL: Rect = Rect {
    x: 146,
    y: 64,
    w: 56,
    h: 22,
};
const CHECK: Rect = Rect {
    x: 16,
    y: 38,
    w: 200,
    h: 14,
};

impl Bar {
    fn load_groups(&mut self) {
        let mut groups: Vec<Group> = Vec::new();
        let mut apps = retro_apps::desktop_apps();
        apps.retain(|a| !a.no_display && !a.terminal);
        apps.sort_by_key(|a| a.name.to_lowercase());
        for a in apps {
            let g = retro_apps::group_of(&a).to_string();
            let entry = (a.name.clone(), a.exec.clone());
            match groups.iter_mut().find(|(n, _)| *n == g) {
                Some((_, v)) => v.push(entry),
                None => groups.push((g, vec![entry])),
            }
        }
        groups.sort_by(|a, b| a.0.cmp(&b.0));
        self.groups = groups;
    }

    fn items_for(&mut self, title: usize) -> Vec<(MenuItem, Cmd)> {
        let sep = || (MenuItem::separator(), Cmd::None);
        match title {
            0 => vec![
                (MenuItem::new("&About RetroOS...", "", false), Cmd::None),
                sep(),
                (MenuItem::new("&Control Panel", "", false), Cmd::None),
                (MenuItem::new("Software &Library", "", false), Cmd::None),
                sep(),
                (MenuItem::new("&DOS Prompt", "", true), Cmd::DosPrompt),
                sep(),
                (MenuItem::new("&Restart...", "", true), Cmd::Restart),
                (MenuItem::new("Shut &Down...", "", true), Cmd::ShutDown),
                sep(),
                (
                    MenuItem::new("E&xit to DOS...", "Ctrl+Alt+F1", true),
                    Cmd::ExitToDos,
                ),
            ],
            1 => {
                self.load_groups();
                if self.groups.is_empty() {
                    return vec![(MenuItem::new("(no programs)", "", false), Cmd::None)];
                }
                self.groups
                    .iter()
                    .enumerate()
                    .map(|(i, (g, _))| (MenuItem::new(&format!("&{g}"), "▸", true), Cmd::Group(i)))
                    .collect()
            }
            _ => {
                let wins = station::windows();
                if wins.is_empty() {
                    return vec![(MenuItem::new("(no windows)", "", false), Cmd::None)];
                }
                let top = wins.iter().find(|w| w.state != "minimized").map(|w| w.id);
                wins.into_iter()
                    .map(|w| {
                        let label = retro_gfx::widgets::ellipsize(&w.title.replace('&', "&&"), 220);
                        let mut it = MenuItem::new(&label, "", true);
                        it.checked = Some(w.id) == top;
                        (it, Cmd::Switch(w.id))
                    })
                    .collect()
            }
        }
    }

    fn close_menus(&mut self, ui: &mut Ui, from: usize) {
        for m in self.menus.drain(from..) {
            ui.close(m.surface);
        }
        if let Some(b) = self.bar {
            ui.redraw(b);
        }
    }

    fn open_title(&mut self, ui: &mut Ui, title: usize) {
        self.close_menus(ui, 0);
        let Some(bar) = self.bar else { return };
        let items = self.items_for(title);
        let only: Vec<MenuItem> = items.iter().map(|(i, _)| i.clone()).collect();
        let (w, h, _) = menu_layout(&only);
        let (x, tw) = title_rects()[title];
        if let Some(s) = ui.popup(
            bar,
            Rect::new(x, 0, tw, HEIGHT - 1),
            (w + 1, h + 1),
            false,
            true,
        ) {
            self.menus.push(Menu {
                title: Some(title),
                surface: s,
                items,
                sel: None,
            });
        }
        ui.redraw(bar);
    }

    fn open_group(&mut self, ui: &mut Ui, level: usize, item: usize, group: usize) {
        self.close_menus(ui, level + 1);
        let Some(parent) = self.menus.get(level) else {
            return;
        };
        let Some((_, apps)) = self.groups.get(group) else {
            return;
        };
        let items: Vec<(MenuItem, Cmd)> = apps
            .iter()
            .map(|(name, argv)| {
                (
                    MenuItem::new(name, "", true),
                    Cmd::Launch(name.clone(), argv.clone()),
                )
            })
            .collect();
        let only: Vec<MenuItem> = items.iter().map(|(i, _)| i.clone()).collect();
        let (w, h, _) = menu_layout(&only);
        let parent_only: Vec<MenuItem> = parent.items.iter().map(|(i, _)| i.clone()).collect();
        let (pw, _, rows) = menu_layout(&parent_only);
        let (ry, rh) = rows[item];
        let surface = parent.surface;
        if let Some(s) = ui.popup(
            surface,
            Rect::new(0, ry, pw - 2, rh),
            (w + 1, h + 1),
            true,
            true,
        ) {
            self.menus.push(Menu {
                title: None,
                surface: s,
                items,
                sel: None,
            });
        }
    }

    fn run(&mut self, ui: &mut Ui, level: usize, item: usize) {
        let Some(cmd) = self.menus.get(level).map(|m| m.items[item].1.clone()) else {
            return;
        };
        if let Cmd::Group(g) = cmd {
            return self.open_group(ui, level, item, g);
        }
        self.close_menus(ui, 0);
        match cmd {
            Cmd::None | Cmd::Group(_) => {}
            Cmd::DosPrompt => {
                let _ = station::launch("DOS PROMPT", &["retro-console".to_string()]);
            }
            Cmd::Launch(name, argv) => {
                let _ = station::launch(&name.replace(' ', "").to_uppercase(), &argv);
            }
            Cmd::Switch(id) => {
                let _ = station::request(&["SWITCH", &id.to_string()]);
            }
            Cmd::Restart => self.ask(ui, Ask::Restart),
            Cmd::ShutDown => self.ask(ui, Ask::ShutDown),
            Cmd::ExitToDos => self.ask(ui, Ask::ExitToDos),
        }
    }

    fn ask(&mut self, ui: &mut Ui, ask: Ask) {
        if let Some(d) = self.dialog.take() {
            ui.close(d.surface);
        }
        let (title, _) = ask_text(ask);
        let surface = ui.window(title, "retroos.bar.dialog", (DLG_W, DLG_H), true);
        self.dialog = Some(Dialog {
            surface,
            ask,
            close_all: false,
            pressed: None,
        });
    }

    fn answer(&mut self, ui: &mut Ui, ok: bool) {
        let Some(d) = self.dialog.take() else { return };
        ui.close(d.surface);
        if !ok {
            return;
        }
        match d.ask {
            Ask::Restart | Ask::ShutDown => {
                let verb = if d.ask == Ask::Restart {
                    "reboot"
                } else {
                    "poweroff"
                };
                let _ = std::process::Command::new("systemctl").arg(verb).status();
            }
            Ask::ExitToDos => {
                if d.close_all {
                    if let Ok((_, tasks)) = station::request(&["TASKS"]) {
                        for t in tasks {
                            let f: Vec<&str> = t.split('\t').collect();
                            if f.len() > 3 && f[0] == "TASK" && f[3] == "App" {
                                let _ = station::request(&["END", f[1]]);
                            }
                        }
                    }
                }
                let _ = station::request(&["PERSONALITY", "text"]);
            }
        }
    }

    fn level_of(&self, id: SurfaceId) -> Option<usize> {
        self.menus.iter().position(|m| m.surface == id)
    }

    fn menu_key(&mut self, ui: &mut Ui, level: usize, sym: u32) {
        use retro_ui_keys::*;
        let m = &mut self.menus[level];
        let n = m.items.len();
        let usable = |i: usize, m: &Menu| m.items[i].0.enabled && !m.items[i].0.is_separator();
        let step = |m: &mut Menu, d: isize| {
            let mut i = m
                .sel
                .map(|s| s as isize)
                .unwrap_or(if d > 0 { -1 } else { 0 });
            for _ in 0..n {
                i = (i + d).rem_euclid(n as isize);
                if usable(i as usize, m) {
                    m.sel = Some(i as usize);
                    break;
                }
            }
        };
        match sym {
            UP => step(m, -1),
            DOWN => step(m, 1),
            ESCAPE | LEFT if level > 0 => self.close_menus(ui, level),
            ESCAPE => self.close_menus(ui, 0),
            RETURN | KP_ENTER | RIGHT => {
                if let Some(s) = m.sel {
                    let is_group = matches!(m.items[s].1, Cmd::Group(_));
                    if sym != RIGHT || is_group {
                        return self.run(ui, level, s);
                    }
                }
            }
            _ => {
                let c = char::from_u32(sym).map(|c| c.to_ascii_lowercase());
                if let Some(i) = m
                    .items
                    .iter()
                    .position(|(it, _)| it.enabled && c.is_some() && it.mnemonic() == c)
                {
                    return self.run(ui, level, i);
                }
            }
        }
        if let Some(m) = self.menus.get(level) {
            ui.redraw(m.surface);
        }
    }

    fn draw_bar(&self, c: &mut Canvas) {
        let w = c.width as i32;
        let mut sf = c.surface();
        sf.fill_rect(0, 0, w as u32, HEIGHT as u32, WHITE);
        sf.fill_rect(0, HEIGHT - 1, w as u32, 1, BLACK);
        let open = self.menus.first().and_then(|m| m.title);
        let font = lantern_12();
        for (i, ((x, tw), t)) in title_rects().into_iter().zip(TITLES).enumerate() {
            let (fg, bg) = if open == Some(i) {
                (WHITE, SKY)
            } else {
                (BLACK, WHITE)
            };
            sf.fill_rect(x, 0, tw as u32, (HEIGHT - 1) as u32, bg);
            sf.text(font, x + 7, 3, 1, fg, None, t);
        }
        let now = clock::now_text();
        sf.text(font, w - 8 - text_width(&now), 3, 1, BLACK, None, &now);
    }

    fn draw_dialog(&self, d: &Dialog, c: &mut Canvas) {
        let mut sf = c.surface();
        sf.clear(SILVER);
        let font = lantern_12();
        let (_, question) = ask_text(d.ask);
        sf.text(font, 16, 14, 1, BLACK, None, question);
        if d.ask == Ask::ExitToDos {
            // A check box: Black frame, White inside, an X when set.
            sf.fill_rect(CHECK.x, CHECK.y, 13, 13, WHITE);
            sf.frame(CHECK.x, CHECK.y, 13, 13, 1, BLACK);
            if d.close_all {
                for i in 2..11 {
                    sf.put(CHECK.x + i, CHECK.y + i, BLACK);
                    sf.put(CHECK.x + 12 - i, CHECK.y + i, BLACK);
                }
            }
            sf.text(
                font,
                CHECK.x + 20,
                CHECK.y,
                1,
                BLACK,
                None,
                "Close all programs first",
            );
            sf.fill_rect(CHECK.x + 20, CHECK.y + 11, 8, 1, BLACK); // C: the key that ticks it
        }
        push_button(
            &mut sf,
            OK.x,
            OK.y,
            OK.w,
            OK.h,
            1,
            "OK",
            true,
            d.pressed == Some(0),
        );
        push_button(
            &mut sf,
            CANCEL.x,
            CANCEL.y,
            CANCEL.w,
            CANCEL.h,
            1,
            "Cancel",
            false,
            d.pressed == Some(1),
        );
    }
}

/// Key symbols the bar uses (X11 keysym values).
mod retro_ui_keys {
    pub const UP: u32 = 0xff52;
    pub const DOWN: u32 = 0xff54;
    pub const LEFT: u32 = 0xff51;
    pub const RIGHT: u32 = 0xff53;
    pub const RETURN: u32 = 0xff0d;
    pub const KP_ENTER: u32 = 0xff8d;
    pub const ESCAPE: u32 = 0xff1b;
    pub const SPACE: u32 = 0x20;
    pub const TAB: u32 = 0xff09;
}

impl Program for Bar {
    fn start(&mut self, ui: &mut Ui) {
        self.bar = ui.bar_top(HEIGHT, "retroos-system-bar");
        if self.bar.is_none() {
            eprintln!("retro-bar: the compositor has no layer shell");
            ui.quit();
            return;
        }
        ui.timer(CLOCK_TIMER, clock::until_next_minute());
    }

    fn draw(&mut self, _ui: &Ui, id: SurfaceId, c: &mut Canvas) {
        if Some(id) == self.bar {
            return self.draw_bar(c);
        }
        if let Some(m) = self.menus.iter().find(|m| m.surface == id) {
            let only: Vec<MenuItem> = m.items.iter().map(|(i, _)| i.clone()).collect();
            let menu = draw_menu(&only, m.sel, 1);
            c.surface().blit(&menu, 0, 0);
            return;
        }
        if let Some(d) = self.dialog.as_ref().filter(|d| d.surface == id) {
            self.draw_dialog(d, c);
        }
    }

    fn event(&mut self, ui: &mut Ui, id: SurfaceId, ev: Event) {
        if std::env::var_os("RETRO_BAR_DEBUG").is_some() {
            eprintln!("retro-bar: {id:?} {ev:?}");
        }
        if Some(id) == self.bar {
            if let Event::Button {
                button: BTN_LEFT,
                pressed: true,
                x,
                ..
            } = ev
            {
                let hit = title_rects()
                    .iter()
                    .position(|(tx, tw)| x >= *tx && x < tx + tw);
                match hit {
                    Some(t) if self.menus.first().and_then(|m| m.title) == Some(t) => {
                        self.close_menus(ui, 0)
                    }
                    Some(t) => self.open_title(ui, t),
                    None => self.close_menus(ui, 0),
                }
            }
            if let Event::Close = ev {
                ui.quit();
            }
            return;
        }
        if let Some(level) = self.level_of(id) {
            let only: Vec<MenuItem> = self.menus[level]
                .items
                .iter()
                .map(|(i, _)| i.clone())
                .collect();
            match ev {
                Event::PointerMove { x, y } | Event::PointerEnter { x, y } => {
                    let sel = menu_hit(&only, x, y);
                    if sel.is_some() && sel != self.menus[level].sel {
                        self.menus[level].sel = sel;
                        ui.redraw(id);
                    }
                }
                Event::Button {
                    button: BTN_LEFT,
                    pressed: false,
                    x,
                    y,
                } => {
                    if let Some(i) = menu_hit(&only, x, y) {
                        self.run(ui, level, i);
                    }
                }
                Event::Key { sym, .. } => self.menu_key(ui, level, sym),
                Event::PopupDone => {
                    // Station closed it (a click outside): close the others too.
                    self.menus.retain(|m| m.surface != id);
                    let rest = level.min(self.menus.len());
                    self.close_menus(ui, rest);
                    if level == 0 {
                        self.close_menus(ui, 0);
                    }
                }
                _ => {}
            }
            return;
        }
        let Some(d) = self.dialog.as_mut().filter(|d| d.surface == id) else {
            return;
        };
        use retro_ui_keys::*;
        match ev {
            Event::Close => self.answer(ui, false),
            Event::Key {
                sym: RETURN | KP_ENTER,
                ..
            } => self.answer(ui, true),
            Event::Key { sym: ESCAPE, .. } => self.answer(ui, false),
            Event::Key {
                sym: SPACE | TAB, ..
            } if d.ask == Ask::ExitToDos => {
                d.close_all = !d.close_all;
                ui.redraw(id);
            }
            Event::Key { text: Some(t), .. }
                if d.ask == Ask::ExitToDos && t.eq_ignore_ascii_case("c") =>
            {
                d.close_all = !d.close_all;
                ui.redraw(id);
            }
            Event::Button {
                button: BTN_LEFT,
                pressed: true,
                x,
                y,
            } => {
                d.pressed = if OK.contains(x, y) {
                    Some(0)
                } else if CANCEL.contains(x, y) {
                    Some(1)
                } else {
                    None
                };
                if d.ask == Ask::ExitToDos && CHECK.contains(x, y) {
                    d.close_all = !d.close_all;
                }
                ui.redraw(id);
            }
            Event::Button {
                button: BTN_LEFT,
                pressed: false,
                x,
                y,
            } => {
                let pressed = d.pressed.take();
                ui.redraw(id);
                if pressed == Some(0) && OK.contains(x, y) {
                    self.answer(ui, true);
                } else if pressed == Some(1) && CANCEL.contains(x, y) {
                    self.answer(ui, false);
                }
            }
            _ => {}
        }
    }

    fn timer(&mut self, ui: &mut Ui, token: u32) {
        if token == CLOCK_TIMER {
            if let Some(b) = self.bar {
                ui.redraw(b);
            }
            ui.timer(
                CLOCK_TIMER,
                clock::until_next_minute().max(Duration::from_secs(1)),
            );
        }
    }
}

fn main() {
    let bar = Bar {
        bar: None,
        menus: Vec::new(),
        dialog: None,
        groups: Vec::new(),
    };
    if let Err(e) = retro_ui::run(Box::new(bar)) {
        eprintln!("retro-bar: {e}");
        std::process::exit(1);
    }
}

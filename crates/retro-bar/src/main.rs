//! retro-bar — the System Bar of the RetroOS Desktop (docs/DESIGN.md §9.1,
//! docs/STYLE.md §4): a 20-pixel White bar along the top of the screen with
//! the ◆ Phosphor menu, Programs, Window, and the clock.
//!
//! Station starts it with the Desktop and shows it only there. Menus are
//! popups with a grab (a click elsewhere closes them); keys: ↑ ↓ Enter Esc
//! and the underlined letters.

mod clock;
mod volume;

use retro_gfx::widgets::{draw_menu, menu_hit, menu_layout, push_button, text_width, MenuItem};
use retro_gfx::{font::lantern_12, Canvas, Theme};
use retro_ui::station;
use retro_ui::{Event, Program, Rect, SurfaceId, Ui};
use std::time::Duration;

const HEIGHT: i32 = 20;
const BTN_LEFT: u32 = 0x110;
const CLOCK_TIMER: u32 = 1;
const VOL_TIMER: u32 = 2;

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
    /// The output in use, while PipeWire runs (the volume item, D-053).
    vol: Option<retro_sys::sound::Channel>,
    vol_popup: Option<volume::Popup>,
    /// Where the pointer is on the bar (the wheel over the speaker).
    bar_x: i32,
    /// Updates waiting (the daily check's count, D-055): a mark left of
    /// the speaker; a click opens the Software Library's Updates.
    updates: u32,
}

/// The speaker's place on a bar `w` wide: left of the clock.
fn speaker_rect(w: i32) -> Rect {
    let clock = text_width(&clock::now_text());
    Rect::new(
        w - 8 - clock - 14 - volume::ICON_W,
        0,
        volume::ICON_W + 6,
        HEIGHT - 1,
    )
}

/// The updates mark's place: left of the speaker.
fn updates_rect(w: i32) -> Rect {
    let s = speaker_rect(w);
    Rect::new(s.x - 18, 0, 16, HEIGHT - 1)
}

/// A box with an arrow down into it.
fn draw_updates_mark(sf: &mut retro_gfx::Surface, x: i32, y: i32, c: retro_gfx::Rgb) {
    sf.frame(x, y + 5, 12, 7, 1, c);
    sf.fill_rect(x + 5, y, 2, 6, c);
    for i in 0..4 {
        sf.fill_rect(x + 2 + i, y + 3 + i, (8 - 2 * i) as u32, 1, c);
    }
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
                (
                    MenuItem::new("&About This Computer...", "", true),
                    Cmd::Launch("ABOUT".into(), vec!["retro-about".into()]),
                ),
                sep(),
                (
                    MenuItem::new("&Control Panel", "", true),
                    Cmd::Launch("CONTROL".into(), vec!["control-panel".into()]),
                ),
                (
                    MenuItem::new("Software &Library", "", true),
                    Cmd::Launch("LIBRARY".into(), vec!["software-library".into()]),
                ),
                sep(),
                (MenuItem::new("&DOS Prompt", "", true), Cmd::DosPrompt),
                (
                    MenuItem::new("&Program Manager", "", true),
                    Cmd::Launch("PROGMAN".into(), vec!["retro-progman".into()]),
                ),
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
                // The falling fifth first (STYLE.md §6, D-053).
                if station::request(&["SOUND", "shutdown"]).is_ok_and(|(f, _)| f.is_empty()) {
                    std::thread::sleep(Duration::from_millis(900));
                }
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

    /// Open or close the volume popup under the speaker.
    fn toggle_volume(&mut self, ui: &mut Ui) {
        self.close_menus(ui, 0);
        if let Some(p) = self.vol_popup.take() {
            ui.close(p.surface);
        } else if let Some(bar) = self.bar {
            let w = ui.size(bar).0;
            let r = speaker_rect(w);
            if let Some(s) = ui.popup(bar, r, (volume::POP_W, volume::POP_H), false, true) {
                let p = volume::Popup::new(s);
                self.vol = p.vol.clone();
                self.vol_popup = Some(p);
            }
        }
        if let Some(b) = self.bar {
            ui.redraw(b);
        }
    }

    /// The wheel over the speaker: ±5 % a step, no popup.
    fn wheel(&mut self, ui: &mut Ui, steps: f64) {
        let Some(mut v) = volume::read() else { return };
        v.volume = (v.volume - steps as f32 * 0.05).clamp(0.0, 1.0);
        if retro_sys::sound::set_volume(v.id, v.volume).is_ok() {
            self.vol = Some(v);
            if let Some(b) = self.bar {
                ui.redraw(b);
            }
        }
    }

    fn popup_event(&mut self, ui: &mut Ui, ev: Event) {
        let Some(p) = self.vol_popup.as_mut() else {
            return;
        };
        let out = match ev {
            Event::PopupDone => volume::Out::Close,
            Event::Button {
                button: BTN_LEFT,
                pressed,
                x,
                y,
            } => p.button(pressed, x, y),
            Event::PointerMove { y, .. } => {
                if !p.motion(y) {
                    return;
                }
                volume::Out::None
            }
            Event::Scroll { steps } => {
                p.step(-steps as f32 * 0.05);
                volume::Out::None
            }
            Event::Key { sym, text, .. } => p.key(sym, text.as_deref()),
            _ => return,
        };
        self.vol = p.vol.clone();
        let surface = p.surface;
        match out {
            volume::Out::None => ui.redraw(surface),
            volume::Out::Close => {
                self.vol_popup = None;
                ui.close(surface);
            }
            volume::Out::Mixer => {
                self.vol_popup = None;
                ui.close(surface);
                let _ = station::launch("VOLUME", &["volume".to_string()]);
            }
        }
        if let Some(b) = self.bar {
            ui.redraw(b);
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

    fn draw_bar(&self, c: &mut Canvas, t: &Theme) {
        let w = c.width as i32;
        let mut sf = c.surface();
        sf.fill_rect(0, 0, w as u32, HEIGHT as u32, t.bar);
        sf.fill_rect(0, HEIGHT - 1, w as u32, 1, t.bar_text);
        let open = self.menus.first().and_then(|m| m.title);
        let font = lantern_12();
        for (i, ((x, tw), title)) in title_rects().into_iter().zip(TITLES).enumerate() {
            let (fg, bg) = if open == Some(i) {
                (t.sel_text, t.sel)
            } else {
                (t.bar_text, t.bar)
            };
            sf.fill_rect(x, 0, tw as u32, (HEIGHT - 1) as u32, bg);
            sf.text(font, x + 7, 3, 1, fg, None, title);
        }
        let now = clock::now_text();
        sf.text(font, w - 8 - text_width(&now), 3, 1, t.bar_text, None, &now);
        let r = speaker_rect(w);
        let (fg, bg) = if self.vol_popup.is_some() {
            (t.sel_text, t.sel)
        } else {
            (t.bar_text, t.bar)
        };
        sf.fill_rect(r.x, r.y, r.w as u32, r.h as u32, bg);
        volume::draw_icon(&mut sf, r.x + 3, 4, self.vol.as_ref(), fg, bg);
        if self.updates > 0 {
            let u = updates_rect(w);
            draw_updates_mark(&mut sf, u.x + 2, 3, t.bar_text);
        }
    }

    fn draw_dialog(&self, d: &Dialog, c: &mut Canvas, t: &Theme) {
        let mut sf = c.surface();
        sf.clear(t.face);
        let font = lantern_12();
        let (_, question) = ask_text(d.ask);
        sf.text(font, 16, 14, 1, t.text, None, question);
        if d.ask == Ask::ExitToDos {
            // A check box: outlined, light inside, an X when set.
            sf.fill_rect(CHECK.x, CHECK.y, 13, 13, t.well);
            sf.frame(CHECK.x, CHECK.y, 13, 13, 1, t.line);
            let mark = t.text;
            if d.close_all {
                for i in 2..11 {
                    sf.put(CHECK.x + i, CHECK.y + i, mark);
                    sf.put(CHECK.x + 12 - i, CHECK.y + i, mark);
                }
            }
            sf.text(
                font,
                CHECK.x + 20,
                CHECK.y,
                1,
                t.text,
                None,
                "Close all programs first",
            );
            sf.fill_rect(CHECK.x + 20, CHECK.y + 11, 8, 1, t.text); // C: the key that ticks it
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
            t,
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
            t,
        );
    }
}

/// Key symbols the bar uses (X11 keysym values).
use retro_ui::keys as retro_ui_keys;

impl Program for Bar {
    fn start(&mut self, ui: &mut Ui) {
        self.bar = ui.bar_top(HEIGHT, "retroos-system-bar");
        if self.bar.is_none() {
            eprintln!("retro-bar: the compositor has no layer shell");
            ui.quit();
            return;
        }
        ui.timer(CLOCK_TIMER, clock::until_next_minute());
        ui.timer(VOL_TIMER, Duration::from_secs(1));
    }

    fn draw(&mut self, ui: &Ui, id: SurfaceId, c: &mut Canvas) {
        if Some(id) == self.bar {
            return self.draw_bar(c, &ui.theme);
        }
        if let Some(m) = self.menus.iter().find(|m| m.surface == id) {
            let only: Vec<MenuItem> = m.items.iter().map(|(i, _)| i.clone()).collect();
            let menu = draw_menu(&only, m.sel, 1, &ui.theme);
            c.surface().blit(&menu, 0, 0);
            return;
        }
        if let Some(p) = self.vol_popup.as_ref().filter(|p| p.surface == id) {
            return p.draw(c, &ui.theme);
        }
        if let Some(d) = self.dialog.as_ref().filter(|d| d.surface == id) {
            self.draw_dialog(d, c, &ui.theme);
        }
    }

    fn event(&mut self, ui: &mut Ui, id: SurfaceId, ev: Event) {
        if std::env::var_os("RETRO_BAR_DEBUG").is_some() {
            eprintln!("retro-bar: {id:?} {ev:?}");
        }
        if self.vol_popup.as_ref().is_some_and(|p| p.surface == id) {
            return self.popup_event(ui, ev);
        }
        if Some(id) == self.bar {
            match ev {
                Event::PointerMove { x, .. } | Event::PointerEnter { x, .. } => self.bar_x = x,
                Event::Scroll { steps } => {
                    let w = ui.size(id).0;
                    if speaker_rect(w).contains(self.bar_x, 2) {
                        self.wheel(ui, steps);
                    }
                }
                _ => {}
            }
            if let Event::Button {
                button: BTN_LEFT,
                pressed: true,
                x,
                ..
            } = ev
            {
                let w = ui.size(id).0;
                if speaker_rect(w).contains(x, 2) {
                    self.toggle_volume(ui);
                    return;
                }
                if self.updates > 0 && updates_rect(w).contains(x, 2) {
                    self.close_menus(ui, 0);
                    let argv = ["software-library".to_string(), "Updates".to_string()];
                    let _ = station::launch("LIBRARY", &argv);
                    return;
                }
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
        if token == VOL_TIMER {
            // Only while PipeWire runs: asking would start it.
            let now = if volume::pipewire_running() {
                volume::read()
            } else {
                self.vol.clone()
            };
            if now != self.vol {
                self.vol = now;
                if let Some(b) = self.bar {
                    ui.redraw(b);
                }
            }
            // New updates found by the daily check: the mark, and a chime.
            let n = retro_sys::software::status().count;
            if n != self.updates {
                if n > self.updates {
                    let _ = station::request(&["SOUND", "notify"]);
                }
                self.updates = n;
                if let Some(b) = self.bar {
                    ui.redraw(b);
                }
            }
            ui.timer(VOL_TIMER, Duration::from_secs(5));
        }
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
        vol: None,
        vol_popup: None,
        bar_x: 0,
        updates: retro_sys::software::status().count,
    };
    if let Err(e) = retro_ui::run(Box::new(bar)) {
        eprintln!("retro-bar: {e}");
        std::process::exit(1);
    }
}

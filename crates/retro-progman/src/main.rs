//! Program Manager: the RetroOS Desktop's launcher (docs/DESIGN.md §9.1–9.2).
//!
//! One window with a menu bar (File, Options, Window, Help) and Windows 3.1
//! group windows inside it: Main (DOS Prompt) and one group per category of
//! the installed `.desktop` files (retro_apps::group_of, the same groups as
//! the System Bar's Programs menu). Double-click or Enter starts a program
//! through Station; arrows move in a group, Ctrl+Tab / Ctrl+F6 to the next
//! group, the wheel and Page Up/Down scroll. Station starts it with the
//! first Desktop of a session; a second copy brings the first one forward.

mod layout;

use layout::{GroupBox, Hit, MENU_H};
use retro_gfx::font::{lantern_12, lantern_9};
use retro_gfx::icons::{icon_picture, IconArt};
use retro_gfx::iconset::{self, Icon};
use retro_gfx::widgets::{draw_menu, ellipsize, ellipsize_in, menu_hit, menu_layout, text_width};
use retro_gfx::widgets::{MenuItem, TEXT_Y};
use retro_gfx::{Canvas, Theme};
use retro_ui::keys::*;
use retro_ui::{station, Event, Program, Rect, SurfaceId, Ui};
use std::time::{Duration, Instant};

const TITLE: &str = "Program Manager";
const APP_ID: &str = "retroos.progman";
const BTN_LEFT: u32 = 0x110;
const DOUBLE_CLICK: Duration = Duration::from_millis(450);
const MENU_TITLES: [&str; 4] = ["File", "Options", "Window", "Help"];

struct Item {
    name: String,
    argv: Vec<String>,
    art: IconArt,
    /// Its own picture (ours, or its theme icon retro-fied), else `art`.
    icon: Option<Icon>,
}

struct Group {
    name: String,
    items: Vec<Item>,
}

#[derive(Clone, Debug, PartialEq)]
enum Cmd {
    None,
    Open,
    Exit,
    MinimizeOnUse,
    Tile,
    Activate(usize),
}

struct Menu {
    title: usize,
    surface: SurfaceId,
    items: Vec<(MenuItem, Cmd)>,
    sel: Option<usize>,
}

struct ProgMan {
    win: Option<SurfaceId>,
    size: (i32, i32),
    groups: Vec<Group>,
    boxes: Vec<GroupBox>,
    content_h: i32,
    active: usize,
    sel: Vec<Option<usize>>,
    scroll: i32,
    menu: Option<Menu>,
    minimize_on_use: bool,
    last_click: Option<(usize, usize, Instant)>,
}

/// Main first, then the categories in the System Bar's order.
fn load_groups() -> Vec<Group> {
    let mut groups = vec![Group {
        name: "Main".into(),
        items: vec![
            Item {
                name: "File Cabinet".into(),
                argv: vec!["file-cabinet".into()],
                art: IconArt::Folder,
                icon: iconset::builtin("file-cabinet").cloned(),
            },
            Item {
                name: "DOS Prompt".into(),
                argv: vec!["retro-console".into()],
                art: IconArt::Dos,
                icon: None,
            },
            Item {
                name: "Control Panel".into(),
                argv: vec!["control-panel".into()],
                art: IconArt::Program,
                icon: iconset::builtin("control-panel").cloned(),
            },
        ],
    }];
    let mut apps = retro_apps::desktop_apps();
    apps.retain(|a| !a.no_display && !a.terminal && !a.exec.is_empty());
    apps.retain(|a| {
        !["retro-console", "file-cabinet", "control-panel"].contains(&a.exec[0].as_str())
    });
    apps.sort_by_key(|a| a.name.to_lowercase());
    let mut rest: Vec<Group> = Vec::new();
    for a in apps {
        let g = retro_apps::group_of(&a);
        let item = Item {
            name: a.name.clone(),
            argv: a.exec.clone(),
            art: IconArt::Program,
            icon: a.icon.as_deref().and_then(iconset::for_app),
        };
        match rest.iter_mut().find(|x| x.name == g) {
            Some(x) => x.items.push(item),
            None => rest.push(Group {
                name: g.into(),
                items: vec![item],
            }),
        }
    }
    rest.sort_by(|a, b| a.name.cmp(&b.name));
    groups.extend(rest);
    groups
}

fn config_path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".config")))?;
    Some(base.join("retroos").join("progman.conf"))
}

fn load_minimize_on_use() -> bool {
    config_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .is_some_and(|s| s.lines().any(|l| l.trim() == "minimize_on_use=yes"))
}

fn save_minimize_on_use(on: bool) {
    if let Some(p) = config_path() {
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let v = if on { "yes" } else { "no" };
        let _ = std::fs::write(p, format!("minimize_on_use={v}\n"));
    }
}

fn menu_title_rects() -> Vec<Rect> {
    let mut x = 2;
    MENU_TITLES
        .iter()
        .map(|t| {
            let w = text_width(t) + 14;
            let r = Rect::new(x, 0, w, MENU_H - 1);
            x += w;
            r
        })
        .collect()
}

impl ProgMan {
    fn counts(&self) -> Vec<usize> {
        self.groups.iter().map(|g| g.items.len()).collect()
    }

    fn relayout(&mut self) {
        let (b, h) = layout::tile(self.size.0, &self.counts());
        self.boxes = b;
        self.content_h = h;
        self.clamp_scroll();
    }

    fn view_h(&self) -> i32 {
        (self.size.1 - MENU_H).max(0)
    }

    fn clamp_scroll(&mut self) {
        self.scroll = self
            .scroll
            .clamp(0, (self.content_h - self.view_h()).max(0));
    }

    /// Scroll so that the selected icon (or the active group's title) shows.
    fn reveal(&mut self) {
        let Some(b) = self.boxes.get(self.active) else {
            return;
        };
        let r = match self.sel[self.active] {
            Some(i) => layout::icon_rect(b, i),
            None => layout::title_rect(b),
        };
        let top = if self.sel[self.active].is_some() {
            r.y
        } else {
            b.rect.y
        };
        if top - layout::GAP < self.scroll {
            self.scroll = top - layout::GAP;
        } else if r.y + r.h + layout::GAP > self.scroll + self.view_h() {
            self.scroll = r.y + r.h + layout::GAP - self.view_h();
        }
        self.clamp_scroll();
    }

    fn redraw(&self, ui: &mut Ui) {
        if let Some(w) = self.win {
            ui.redraw(w);
        }
    }

    fn launch(&mut self, ui: &mut Ui, g: usize, i: usize) {
        let Some(item) = self.groups.get(g).and_then(|g| g.items.get(i)) else {
            return;
        };
        let name = item.name.replace(' ', "").to_uppercase();
        if let Err(e) = station::launch(&name, &item.argv) {
            eprintln!("retro-progman: cannot start {}: {e}", item.name);
            return;
        }
        if self.minimize_on_use {
            if let Some(w) = self.win {
                ui.minimize(w);
            }
        }
    }

    // ------------------------------------------------------------ menus

    fn menu_items(&self, title: usize) -> Vec<(MenuItem, Cmd)> {
        let sep = || (MenuItem::separator(), Cmd::None);
        let can_open = self.sel.get(self.active).copied().flatten().is_some();
        match title {
            0 => vec![
                (MenuItem::new("&Open", "Enter", can_open), Cmd::Open),
                sep(),
                (MenuItem::new("&Run...", "", false), Cmd::None),
                sep(),
                (
                    MenuItem::new("E&xit Program Manager", "Alt+F4", true),
                    Cmd::Exit,
                ),
            ],
            1 => {
                let mut it = MenuItem::new("&Minimize on Use", "", true);
                it.checked = self.minimize_on_use;
                vec![(it, Cmd::MinimizeOnUse)]
            }
            2 => {
                let mut v = vec![(MenuItem::new("&Tile", "", true), Cmd::Tile), sep()];
                for (n, g) in self.groups.iter().enumerate().take(9) {
                    let mut it = MenuItem::new(&format!("&{} {}", n + 1, g.name), "", true);
                    it.checked = n == self.active;
                    v.push((it, Cmd::Activate(n)));
                }
                v
            }
            _ => vec![(
                MenuItem::new("&About Program Manager...", "", false),
                Cmd::None,
            )],
        }
    }

    fn close_menu(&mut self, ui: &mut Ui) {
        if let Some(m) = self.menu.take() {
            ui.close(m.surface);
        }
        self.redraw(ui);
    }

    fn open_menu(&mut self, ui: &mut Ui, title: usize) {
        self.close_menu(ui);
        let Some(win) = self.win else { return };
        let items = self.menu_items(title);
        let only: Vec<MenuItem> = items.iter().map(|(i, _)| i.clone()).collect();
        let (w, h, _) = menu_layout(&only);
        let r = menu_title_rects()[title];
        if let Some(s) = ui.popup(win, r, (w + 1, h + 1), false, true) {
            self.menu = Some(Menu {
                title,
                surface: s,
                items,
                sel: None,
            });
        }
        self.redraw(ui);
    }

    fn run(&mut self, ui: &mut Ui, cmd: Cmd) {
        self.close_menu(ui);
        match cmd {
            Cmd::None => {}
            Cmd::Open => {
                if let Some(i) = self.sel[self.active] {
                    self.launch(ui, self.active, i);
                }
            }
            Cmd::Exit => ui.quit(),
            Cmd::MinimizeOnUse => {
                self.minimize_on_use = !self.minimize_on_use;
                save_minimize_on_use(self.minimize_on_use);
            }
            Cmd::Tile => {
                self.relayout();
                self.reveal();
            }
            Cmd::Activate(g) => self.activate(g),
        }
        self.redraw(ui);
    }

    fn menu_key(&mut self, ui: &mut Ui, sym: u32) {
        let Some(m) = self.menu.as_mut() else { return };
        let n = m.items.len();
        let usable = |i: usize, m: &Menu| m.items[i].0.enabled && !m.items[i].0.is_separator();
        let mut step = |d: isize| {
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
            UP => step(-1),
            DOWN => step(1),
            ESCAPE => return self.close_menu(ui),
            LEFT | RIGHT => {
                let t = m.title as isize + if sym == LEFT { -1 } else { 1 };
                let t = t.rem_euclid(MENU_TITLES.len() as isize) as usize;
                return self.open_menu(ui, t);
            }
            RETURN | KP_ENTER => {
                if let Some(s) = m.sel {
                    let cmd = m.items[s].1.clone();
                    return self.run(ui, cmd);
                }
            }
            _ => {
                let c = char::from_u32(sym).map(|c| c.to_ascii_lowercase());
                if let Some(i) = m
                    .items
                    .iter()
                    .position(|(it, _)| it.enabled && c.is_some() && it.mnemonic() == c)
                {
                    let cmd = m.items[i].1.clone();
                    return self.run(ui, cmd);
                }
            }
        }
        if let Some(m) = &self.menu {
            ui.redraw(m.surface);
        }
    }

    // ------------------------------------------------------ selection

    fn activate(&mut self, g: usize) {
        if g < self.groups.len() {
            self.active = g;
            if self.sel[g].is_none() && !self.groups[g].items.is_empty() {
                self.sel[g] = Some(0);
            }
            self.reveal();
        }
    }

    fn key(
        &mut self,
        ui: &mut Ui,
        sym: u32,
        ctrl: bool,
        shift: bool,
        alt: bool,
        text: Option<String>,
    ) {
        if self.menu.is_some() {
            return self.menu_key(ui, sym);
        }
        let n_groups = self.groups.len();
        if alt || sym == F10 {
            // Alt+letter (or F10) opens a menu.
            let c = text.as_deref().and_then(|t| t.chars().next());
            let t = match c.map(|c| c.to_ascii_lowercase()) {
                Some('f') => Some(0),
                Some('o') => Some(1),
                Some('w') => Some(2),
                Some('h') => Some(3),
                _ if sym == F10 => Some(0),
                _ => None,
            };
            if let Some(t) = t {
                self.open_menu(ui, t);
            }
            return;
        }
        let g = self.active;
        let n = self.groups.get(g).map_or(0, |x| x.items.len());
        let per_row = self.boxes.get(g).map_or(1, |b| b.per_row);
        let mv = |s: &mut ProgMan, dx: i32, dy: i32| {
            let i = s.sel[g].unwrap_or(0);
            let to = layout::step(i, n, per_row, dx, dy);
            s.sel[g] = (n > 0).then_some(to);
        };
        match sym {
            TAB | ISO_LEFT_TAB | F6 if ctrl || sym != F6 => {
                let back = shift || sym == ISO_LEFT_TAB;
                let next = if back {
                    (g + n_groups - 1) % n_groups.max(1)
                } else {
                    (g + 1) % n_groups.max(1)
                };
                self.activate(next);
            }
            LEFT => mv(self, -1, 0),
            RIGHT => mv(self, 1, 0),
            UP => mv(self, 0, -1),
            DOWN => mv(self, 0, 1),
            HOME => self.sel[g] = (n > 0).then_some(0),
            END => self.sel[g] = n.checked_sub(1),
            PAGE_UP => self.scroll -= self.view_h() - layout::CELL_H,
            PAGE_DOWN => self.scroll += self.view_h() - layout::CELL_H,
            RETURN | KP_ENTER => {
                if let Some(i) = self.sel[g] {
                    self.launch(ui, g, i);
                }
            }
            _ => return,
        }
        if matches!(sym, PAGE_UP | PAGE_DOWN) {
            self.clamp_scroll();
        } else {
            self.reveal();
        }
        self.redraw(ui);
    }

    fn click(&mut self, ui: &mut Ui, x: i32, y: i32) {
        if y < MENU_H {
            let t = menu_title_rects().iter().position(|r| r.contains(x, y));
            match t {
                Some(t) if self.menu.as_ref().map(|m| m.title) == Some(t) => self.close_menu(ui),
                Some(t) => self.open_menu(ui, t),
                None => self.close_menu(ui),
            }
            return;
        }
        let counts = self.counts();
        let wy = y - MENU_H + self.scroll;
        match layout::hit(&self.boxes, &counts, x, wy) {
            Some(Hit::Icon(g, i)) => {
                let now = Instant::now();
                let double = self.last_click.is_some_and(|(lg, li, t)| {
                    lg == g && li == i && now.duration_since(t) < DOUBLE_CLICK
                });
                self.active = g;
                self.sel[g] = Some(i);
                if double {
                    self.last_click = None;
                    self.launch(ui, g, i);
                } else {
                    self.last_click = Some((g, i, now));
                }
            }
            Some(Hit::Group(g)) => self.activate(g),
            None => {}
        }
        self.redraw(ui);
    }

    // --------------------------------------------------------- drawing

    fn draw_window(&self, c: &mut Canvas, t: &Theme) {
        let (w, h) = (c.width as i32, c.height as i32);
        let mut sf = c.surface();
        // Workspace, then the groups (scrolled), then the menu bar over them.
        sf.fill_rect(0, MENU_H, w as u32, (h - MENU_H).max(0) as u32, t.light);
        for (g, b) in self.boxes.iter().enumerate() {
            let oy = MENU_H - self.scroll;
            let r = b.rect;
            if r.y + oy + r.h < MENU_H || r.y + oy > h {
                continue;
            }
            self.draw_group(&mut sf, g, b, oy, t);
        }
        sf.fill_rect(0, 0, w as u32, (MENU_H - 1) as u32, t.light);
        sf.fill_rect(0, MENU_H - 1, w as u32, 1, t.line);
        let open = self.menu.as_ref().map(|m| m.title);
        for (i, (r, title)) in menu_title_rects().into_iter().zip(MENU_TITLES).enumerate() {
            let (fg, bg) = if open == Some(i) {
                (t.sel_text, t.sel)
            } else {
                (t.text, t.light)
            };
            sf.fill_rect(r.x, r.y, r.w as u32, r.h as u32, bg);
            sf.text(lantern_12(), r.x + 7, TEXT_Y, 1, fg, None, title);
            // The mnemonic (first letter) underlined, as in Windows 3.1.
            let first = title.chars().next().map(String::from).unwrap_or_default();
            sf.fill_rect(
                r.x + 7,
                TEXT_Y + 11,
                (text_width(&first) - 1).max(1) as u32,
                1,
                fg,
            );
        }
    }

    fn draw_group(&self, sf: &mut retro_gfx::Surface, g: usize, b: &GroupBox, oy: i32, t: &Theme) {
        let r = b.rect;
        let (x, y) = (r.x, r.y + oy);
        let active = g == self.active;
        // Frame: outline, face border, title bar, line, client.
        sf.fill_rect(x, y, r.w as u32, r.h as u32, t.face);
        sf.frame(x, y, r.w as u32, r.h as u32, 1, t.line);
        let tr = layout::title_rect(b);
        let (tbg, tfg) = if active {
            (t.active, t.active_text)
        } else {
            (t.inactive, t.inactive_text)
        };
        sf.fill_rect(tr.x, tr.y + oy, tr.w as u32, tr.h as u32, tbg);
        sf.fill_rect(tr.x, tr.y + oy + tr.h, tr.w as u32, 1, t.line);
        let name = ellipsize(&self.groups[g].name, tr.w - 8);
        let tx = tr.x + (tr.w - text_width(&name)) / 2;
        sf.text(lantern_12(), tx, tr.y + oy + TEXT_Y, 1, tfg, None, &name);
        let cy = tr.y + tr.h + 1;
        let ch = r.y + r.h - layout::BORDER - cy;
        sf.fill_rect(
            x + layout::BORDER,
            cy + oy,
            (r.w - 2 * layout::BORDER) as u32,
            ch.max(0) as u32,
            t.light,
        );
        sf.frame(
            x + layout::BORDER - 1,
            cy + oy - 1,
            (r.w - 2 * layout::BORDER + 2) as u32,
            (ch + 2).max(0) as u32,
            1,
            t.line,
        );
        let font = lantern_9();
        for (i, item) in self.groups[g].items.iter().enumerate() {
            let cell = layout::icon_rect(b, i);
            let (cx, cy) = (cell.x, cell.y + oy);
            match &item.icon {
                Some(i) => iconset::draw(sf, i, cx + (cell.w - 32) / 2, cy + 2, 1, t),
                None => icon_picture(sf, item.art, cx + (cell.w - 32) / 2, cy + 2, 1, t),
            }
            let label = ellipsize_in(font, &item.name, cell.w - 6);
            let tw = font.text_width(&label) as i32;
            let lx = cx + (cell.w - tw) / 2;
            let ly = cy + 38;
            let selected = active && self.sel[g] == Some(i);
            let fg = if selected {
                sf.fill_rect(lx - 2, ly - 1, (tw + 3) as u32, 11, t.sel);
                t.sel_text
            } else {
                t.text
            };
            sf.text(font, lx, ly, 1, fg, None, &label);
        }
    }
}

impl Program for ProgMan {
    fn start(&mut self, ui: &mut Ui) {
        // One Program Manager: a second start brings the first forward.
        if let Some(w) = station::windows().iter().find(|w| w.title == TITLE) {
            let _ = station::request(&["SWITCH", &w.id.to_string()]);
            ui.quit();
            return;
        }
        self.win = Some(ui.window(TITLE, APP_ID, self.size, false));
        self.relayout();
    }

    fn draw(&mut self, ui: &Ui, id: SurfaceId, c: &mut Canvas) {
        if Some(id) == self.win {
            return self.draw_window(c, &ui.theme);
        }
        if let Some(m) = self.menu.as_ref().filter(|m| m.surface == id) {
            let only: Vec<MenuItem> = m.items.iter().map(|(i, _)| i.clone()).collect();
            let menu = draw_menu(&only, m.sel, 1, &ui.theme);
            c.surface().blit(&menu, 0, 0);
        }
    }

    fn event(&mut self, ui: &mut Ui, id: SurfaceId, ev: Event) {
        if std::env::var_os("RETRO_PROGMAN_DEBUG").is_some() {
            eprintln!("retro-progman: {id:?} {ev:?}");
        }
        if let Some(m) = self.menu.as_mut().filter(|m| m.surface == id) {
            let only: Vec<MenuItem> = m.items.iter().map(|(i, _)| i.clone()).collect();
            match ev {
                Event::PointerMove { x, y } | Event::PointerEnter { x, y } => {
                    let sel = menu_hit(&only, x, y);
                    if sel.is_some() && sel != m.sel {
                        m.sel = sel;
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
                        let cmd = m.items[i].1.clone();
                        self.run(ui, cmd);
                    }
                }
                Event::Key { sym, .. } => self.menu_key(ui, sym),
                Event::PopupDone => {
                    self.menu = None;
                    self.redraw(ui);
                }
                _ => {}
            }
            return;
        }
        if Some(id) != self.win {
            return;
        }
        match ev {
            Event::Configure { w, h } => {
                self.size = (w, h);
                self.relayout();
            }
            Event::Close => ui.quit(),
            Event::Button {
                button: BTN_LEFT,
                pressed: true,
                x,
                y,
            } => self.click(ui, x, y),
            Event::Scroll { steps } => {
                self.scroll += (steps * layout::CELL_H as f64 / 2.0).round() as i32;
                self.clamp_scroll();
                self.redraw(ui);
            }
            Event::Key {
                sym,
                text,
                ctrl,
                alt,
                shift,
            } => self.key(ui, sym, ctrl, shift, alt, text),
            _ => {}
        }
    }
}

fn main() {
    let groups = load_groups();
    let sel = groups
        .iter()
        .map(|g| (!g.items.is_empty()).then_some(0))
        .collect();
    let pm = ProgMan {
        win: None,
        size: (500, 330),
        groups,
        boxes: Vec::new(),
        content_h: 0,
        active: 0,
        sel,
        scroll: 0,
        menu: None,
        minimize_on_use: load_minimize_on_use(),
        last_click: None,
    };
    if let Err(e) = retro_ui::run(Box::new(pm)) {
        eprintln!("retro-progman: {e}");
        std::process::exit(1);
    }
}

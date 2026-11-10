//! Arcade — the game library (DESIGN.md §9.2, D-057): the consoles on the
//! left (with how many games each has), their games on the right; Play
//! starts the game full screen in its emulator (a RetroArch core, or
//! Mednafen / Stella), Install Emulator installs what plays a console
//! (through the Software Library: the administrator's password), Open
//! Folder shows its game folder (C:\GAMES\<console>) in File Cabinet.
//! In RetroDOS, ARCADE runs it full screen.

mod systems;

use retro_gfx::font::lantern_12;
use retro_gfx::widgets::{ellipsize_in, push_button};
use retro_gfx::{Canvas, Theme};
use retro_ui::controls::{ListBox, ROW_H};
use retro_ui::keys::*;
use retro_ui::{station, Event, Program, Rect, SurfaceId, Ui};
use std::path::PathBuf;
use std::time::Duration;
use systems::{fullscreen_cfg, games_dir, make_folders, title, System, SYSTEMS};

const TITLE: &str = "Arcade";
const W: i32 = 540;
const H: i32 = 340;
const LEFT_BUTTON: u32 = 0x110;
const REFRESH: u32 = 1;
const SYS: Rect = Rect {
    x: 8,
    y: 26,
    w: 200,
    h: 4 + 15 * ROW_H,
};
const GAMES: Rect = Rect {
    x: 216,
    y: 26,
    w: W - 224,
    h: 4 + 15 * ROW_H,
};
const BTN_H: i32 = 24;
const BUTTONS: [(&str, i32); 3] = [
    ("Play", 72),
    ("Install Emulator", 132),
    ("Open Folder", 104),
];

/// The buttons, left to right, ending at the window's right edge.
fn button(i: usize) -> Rect {
    let total: i32 = BUTTONS.iter().map(|(_, w)| w + 8).sum::<i32>() - 8;
    let start = W - 8 - total;
    let before: i32 = BUTTONS.iter().take(i).map(|(_, w)| w + 8).sum();
    Rect::new(start + before, H - BTN_H - 10, BUTTONS[i].1, BTN_H)
}

struct Arcade {
    win: Option<SurfaceId>,
    games_dir: PathBuf,
    systems: ListBox,
    games: ListBox,
    found: Vec<PathBuf>,
    /// Keys go to the games list.
    right: bool,
    pressed: Option<usize>,
    note: Option<String>,
    last_click: Option<(usize, std::time::Instant)>,
}

impl Arcade {
    fn system(&self) -> &'static System {
        &SYSTEMS[self.systems.sel.unwrap_or(0).min(SYSTEMS.len() - 1)]
    }

    /// Re-read the consoles' game counts and the chosen console's games.
    fn load(&mut self) {
        let sel = self.systems.sel;
        let items = SYSTEMS
            .iter()
            .map(|s| {
                let n = s.games(&self.games_dir).len();
                if n > 0 {
                    format!("{} ({n})", s.name)
                } else {
                    s.name.to_string()
                }
            })
            .collect();
        let top = self.systems.top;
        self.systems = ListBox::new(items);
        self.systems.sel = sel.or(Some(0));
        self.systems.top = top;
        let keep = self.games.sel;
        self.found = self.system().games(&self.games_dir);
        self.games = ListBox::new(self.found.iter().map(|g| title(g)).collect());
        if let Some(k) = keep.filter(|k| *k < self.found.len()) {
            self.games.sel = Some(k);
        }
    }

    fn status(&self) -> String {
        if let Some(n) = &self.note {
            return n.clone();
        }
        let s = self.system();
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let dir = retro_dosfs::Dos::new(&home).display(&s.dir(&self.games_dir));
        if !s.ready() {
            format!("{} is not installed: Install Emulator.", s.emulator())
        } else if self.found.is_empty() {
            format!("No games yet: put them in {dir}.")
        } else {
            format!("{} games in {dir}.", self.found.len())
        }
    }

    fn draw_window(&self, c: &mut Canvas, t: &Theme) {
        let mut sf = c.surface();
        sf.clear(t.face);
        let font = lantern_12();
        sf.text(font, SYS.x, 8, 1, t.text, None, "Consoles:");
        sf.text(font, GAMES.x, 8, 1, t.text, None, "Games:");
        self.systems.draw(&mut sf, SYS, !self.right, t);
        self.games.draw(&mut sf, GAMES, self.right, t);
        if self.found.is_empty() {
            sf.text(
                font,
                GAMES.x + 8,
                GAMES.y + 8,
                1,
                t.disabled,
                None,
                "(none)",
            );
        }
        let y = SYS.y + SYS.h + 8;
        sf.text(
            font,
            8,
            y,
            1,
            t.text,
            None,
            &ellipsize_in(font, &self.status(), W - 16),
        );
        let ready = self.system().ready();
        for (i, (l, _)) in BUTTONS.iter().enumerate() {
            let b = button(i);
            let enabled = match i {
                0 => ready && self.games.sel.is_some() && !self.found.is_empty(),
                1 => !ready,
                _ => true,
            };
            push_button(
                &mut sf,
                b.x,
                b.y,
                b.w,
                b.h,
                1,
                l,
                i == 0 && enabled,
                self.pressed == Some(i),
                t,
            );
            if !enabled {
                // Greyed: the label redrawn in the disabled colour.
                let tw = font.text_width(l) as i32;
                sf.fill_rect(b.x + 4, b.y + 4, (b.w - 8) as u32, (b.h - 8) as u32, t.face);
                sf.text(font, b.x + (b.w - tw) / 2, b.y + 6, 1, t.disabled, None, l);
            }
        }
    }

    fn play(&mut self) {
        let s = self.system();
        let Some(game) = self.games.sel.and_then(|i| self.found.get(i)).cloned() else {
            return;
        };
        if !s.ready() {
            self.note = Some(format!("{} is not installed yet.", s.emulator()));
            return;
        }
        let argv = s.command(&game, fullscreen_file().as_deref());
        match station::launch(&title(&game).to_ascii_uppercase(), &argv) {
            Ok(_) => self.note = Some(format!("Starting {}...", title(&game))),
            Err(e) => self.note = Some(format!("Cannot start the game: {e}")),
        }
    }

    fn press(&mut self, i: usize) {
        let s = self.system();
        match i {
            0 => self.play(),
            1 if !s.ready() => {
                // The Software Library asks for the password and installs.
                let mut argv = vec!["software-library".to_string(), "--install".to_string()];
                argv.extend(s.packages.iter().map(|p| p.to_string()));
                self.note = match station::launch("LIBRARY", &argv) {
                    Ok(_) => Some(format!("Installing {}...", s.emulator())),
                    Err(e) => Some(format!("Cannot start the Software Library: {e}")),
                };
            }
            2 => {
                let dir = s.dir(&self.games_dir);
                let _ = std::fs::create_dir_all(&dir);
                let argv = [
                    "file-cabinet".to_string(),
                    dir.to_string_lossy().into_owned(),
                ];
                if let Err(e) = station::launch("FILE CABINET", &argv) {
                    self.note = Some(format!("Cannot open the folder: {e}"));
                }
            }
            _ => {}
        }
    }

    fn event_window(&mut self, ui: &mut Ui, ev: Event) {
        match ev {
            Event::Close => return ui.quit(),
            Event::Button {
                button: LEFT_BUTTON,
                pressed,
                x,
                y,
            } => {
                if !pressed {
                    if let Some(i) = self.pressed.take().filter(|i| button(*i).contains(x, y)) {
                        self.press(i);
                    }
                } else if let Some(i) = (0..BUTTONS.len()).find(|i| button(*i).contains(x, y)) {
                    self.pressed = Some(i);
                } else if SYS.contains(x, y) {
                    self.right = false;
                    if self.systems.click(y - SYS.y).is_some() {
                        self.note = None;
                        self.games = ListBox::default();
                        self.load();
                    }
                } else if GAMES.contains(x, y) {
                    self.right = true;
                    if let Some(i) = self.games.click(y - GAMES.y) {
                        // A double-click plays.
                        let double = self.last_click.is_some_and(|(j, t)| {
                            j == i && t.elapsed() < retro_gfx::prefs::double_click()
                        });
                        self.last_click = Some((i, std::time::Instant::now()));
                        if double {
                            self.last_click = None;
                            self.play();
                        }
                    }
                }
            }
            Event::Scroll { steps } => {
                let d = steps.round() as isize * 3;
                if self.right {
                    self.games.scroll(d, GAMES.h);
                } else {
                    self.systems.scroll(d, SYS.h);
                }
            }
            Event::Key { sym, .. } => match sym {
                ESCAPE => return ui.quit(),
                TAB | ISO_LEFT_TAB => self.right = !self.right,
                RIGHT => self.right = true,
                LEFT => self.right = false,
                RETURN | KP_ENTER if self.right => self.play(),
                RETURN | KP_ENTER => self.right = true,
                F1 | INSERT => self.press(1),
                _ if self.right => {
                    self.games.key(sym, GAMES.h);
                }
                _ => {
                    let before = self.systems.sel;
                    self.systems.key(sym, SYS.h);
                    if self.systems.sel != before {
                        self.note = None;
                        self.games = ListBox::default();
                        self.load();
                    }
                }
            },
            _ => return,
        }
        if let Some(w) = self.win {
            ui.redraw(w);
        }
    }
}

/// RetroArch's full-screen size for the main screen (where Station puts
/// new windows), written to the runtime folder; None outside Station.
fn fullscreen_file() -> Option<PathBuf> {
    let (_, _, w, h) = *station::screens().first()?;
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)?;
    let path = dir.join("retroos-arcade-retroarch.cfg");
    std::fs::write(&path, fullscreen_cfg((w, h))).ok()?;
    Some(path)
}

impl Program for Arcade {
    fn start(&mut self, ui: &mut Ui) {
        if let Some(w) = station::windows().iter().find(|w| w.title == TITLE) {
            let _ = station::request(&["SWITCH", &w.id.to_string()]);
            ui.quit();
            return;
        }
        self.win = Some(ui.window(TITLE, "retroos.arcade", (W, H), true));
        ui.timer(REFRESH, Duration::from_secs(3));
    }

    fn draw(&mut self, ui: &Ui, id: SurfaceId, c: &mut Canvas) {
        if Some(id) == self.win {
            self.draw_window(c, &ui.theme);
        }
    }

    fn event(&mut self, ui: &mut Ui, id: SurfaceId, ev: Event) {
        if Some(id) == self.win {
            self.event_window(ui, ev);
        }
    }

    /// New games copied in, an emulator installed: show them.
    fn timer(&mut self, ui: &mut Ui, token: u32) {
        if token == REFRESH {
            self.load();
            if let Some(w) = self.win {
                ui.redraw(w);
            }
            ui.timer(REFRESH, Duration::from_secs(3));
        }
    }
}

fn main() {
    let games_dir = games_dir();
    make_folders(&games_dir);
    let mut a = Arcade {
        win: None,
        games_dir,
        systems: ListBox::default(),
        games: ListBox::default(),
        found: Vec::new(),
        right: false,
        pressed: None,
        note: None,
        last_click: None,
    };
    a.load();
    if let Err(e) = retro_ui::run(Box::new(a)) {
        eprintln!("arcade: {e}");
        std::process::exit(1);
    }
}

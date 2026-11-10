//! Media Player — the Desktop's player (DESIGN.md §9.2, D-057), after
//! Windows 3.1's: a small window with the file's title and time, a
//! position slider, Play / Pause / Stop / back / forward / Eject (open)
//! buttons and a volume slider. mpv does the playing (player.rs); video
//! appears in its own window.
//!
//!   media-player [file]

mod player;

use player::{clock, Fake, Mpv, Player, State};
use retro_gfx::font::lantern_12;
use retro_gfx::widgets::{bevel, ellipsize_in, push_button, text_width};
use retro_gfx::{Canvas, Rgb, Surface, Theme};
use retro_ui::keys::*;
use retro_ui::opendlg::{self, OpenDlg};
use retro_ui::{station, Event, Program, Rect, SurfaceId, Ui};
use std::path::PathBuf;
use std::time::Duration;

const TITLE: &str = "Media Player";
const W: i32 = 420;
const H: i32 = 124;
const LEFT_BUTTON: u32 = 0x110;
const TICK: u32 = 1;

/// What mpv opens (Open... shows these).
pub const EXTS: &[&str] = &[
    "mp3", "ogg", "oga", "opus", "flac", "wav", "m4a", "aac", "wma", "mid", "mod", "xm", "s3m",
    "it", "mp4", "m4v", "mkv", "webm", "avi", "mov", "mpg", "mpeg", "ogv", "wmv", "flv", "3gp",
];

const TRACK: Rect = Rect {
    x: 10,
    y: 30,
    w: W - 20,
    h: 16,
};
const BTN_W: i32 = 36;
const BTN_H: i32 = 26;
const BTN_Y: i32 = 56;
const VOL: Rect = Rect {
    x: 300,
    y: BTN_Y + 6,
    w: 110,
    h: 14,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Btn {
    Play,
    Pause,
    Stop,
    Back,
    Forward,
    Eject,
}

const BUTTONS: [Btn; 6] = [
    Btn::Play,
    Btn::Pause,
    Btn::Stop,
    Btn::Back,
    Btn::Forward,
    Btn::Eject,
];

fn button_rect(i: usize) -> Rect {
    // Eject stands apart, as on a tape deck.
    let gap = if i == 5 { 12 } else { 0 };
    Rect::new(10 + i as i32 * (BTN_W + 4) + gap, BTN_Y, BTN_W, BTN_H)
}

/// The glyph on a transport button, in `c`, centred in `r`.
fn glyph(sf: &mut Surface, b: Btn, r: Rect, c: Rgb) {
    let (cx, cy) = (r.x + r.w / 2, r.y + r.h / 2);
    let tri_right = |sf: &mut Surface, x: i32| {
        for i in 0..5 {
            sf.fill_rect(x + i, cy - 5 + i, 1, (11 - 2 * i) as u32, c);
        }
    };
    let tri_left = |sf: &mut Surface, x: i32| {
        for i in 0..5 {
            sf.fill_rect(x - i, cy - 5 + i, 1, (11 - 2 * i) as u32, c);
        }
    };
    match b {
        Btn::Play => tri_right(sf, cx - 2),
        Btn::Pause => {
            sf.fill_rect(cx - 4, cy - 5, 3, 11, c);
            sf.fill_rect(cx + 2, cy - 5, 3, 11, c);
        }
        Btn::Stop => sf.fill_rect(cx - 4, cy - 4, 9, 9, c),
        Btn::Back => {
            tri_left(sf, cx);
            tri_left(sf, cx + 5);
            sf.fill_rect(cx - 6, cy - 5, 2, 11, c);
        }
        Btn::Forward => {
            tri_right(sf, cx - 5);
            tri_right(sf, cx);
            sf.fill_rect(cx + 5, cy - 5, 2, 11, c);
        }
        Btn::Eject => {
            for i in 0..5 {
                sf.fill_rect(cx - i, cy - 5 + i, (2 * i + 1) as u32, 1, c);
            }
            sf.fill_rect(cx - 5, cy + 2, 11, 3, c);
        }
    }
}

struct Media {
    win: Option<SurfaceId>,
    player: Result<Box<dyn Player>, String>,
    state: State,
    pressed: Option<usize>,
    dragging: Option<f64>,
    vol_drag: bool,
    open: Option<OpenDlg>,
    first: Option<PathBuf>,
}

impl Media {
    fn player(&mut self) -> Option<&mut Box<dyn Player>> {
        self.player.as_mut().ok()
    }

    fn refresh(&mut self) {
        if let Ok(p) = self.player.as_mut() {
            self.state = p.poll().clone();
        }
    }

    fn press(&mut self, ui: &mut Ui, b: Btn) {
        let s = self.state.clone();
        match b {
            Btn::Eject => return self.open_dialog(ui),
            Btn::Play if s.idle && !s.path.is_empty() => {
                let p = PathBuf::from(&s.path);
                if let Some(pl) = self.player() {
                    pl.load(&p)
                }
            }
            Btn::Play if s.path.is_empty() => return self.open_dialog(ui),
            Btn::Play => {
                if let Some(pl) = self.player() {
                    pl.set_pause(false)
                }
            }
            Btn::Pause => {
                if let Some(pl) = self.player() {
                    pl.set_pause(!s.paused)
                }
            }
            Btn::Stop => {
                if let Some(pl) = self.player() {
                    pl.stop()
                }
            }
            Btn::Back => {
                if let Some(pl) = self.player() {
                    pl.seek(s.pos - 10.0)
                }
            }
            Btn::Forward => {
                if let Some(pl) = self.player() {
                    pl.seek((s.pos + 10.0).min(s.duration))
                }
            }
        }
        self.refresh();
    }

    fn open_dialog(&mut self, ui: &mut Ui) {
        let Some(win) = self.win else { return };
        if self.open.is_some() {
            return;
        }
        let dir = if self.state.path.is_empty() {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| "/".into());
            ["Music", "Videos"]
                .iter()
                .map(|d| home.join(d))
                .find(|d| d.is_dir())
                .unwrap_or(home)
        } else {
            PathBuf::from(&self.state.path)
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| "/".into())
        };
        let s = ui.dialog(win, "Open", opendlg::SIZE);
        self.open = Some(OpenDlg::new(s, dir, EXTS));
    }

    fn play(&mut self, path: PathBuf) {
        if let Some(p) = self.player() {
            p.load(&path);
        }
        self.refresh();
    }

    /// Seconds at x on the position slider.
    fn pos_at(&self, x: i32) -> f64 {
        let f = ((x - TRACK.x - 6) as f64 / (TRACK.w - 12) as f64).clamp(0.0, 1.0);
        f * self.state.duration
    }

    fn draw_window(&self, c: &mut Canvas, t: &Theme) {
        let mut sf = c.surface();
        sf.clear(t.face);
        let font = lantern_12();
        let s = &self.state;
        let time = if s.duration > 0.0 {
            format!(
                "{} / {}",
                clock(self.dragging.unwrap_or(s.pos)),
                clock(s.duration)
            )
        } else {
            String::new()
        };
        let name = if s.path.is_empty() {
            "(no file) - Eject opens one".to_string()
        } else if s.title.is_empty() {
            s.path.rsplit('/').next().unwrap_or("").to_string()
        } else {
            s.title.clone()
        };
        let tw = text_width(&time);
        sf.text(
            font,
            10,
            10,
            1,
            t.text,
            None,
            &ellipsize_in(font, &name, W - 30 - tw),
        );
        sf.text(font, W - 10 - tw, 10, 1, t.text, None, &time);
        // Position: a sunken track with tick marks and a raised thumb.
        let r = TRACK;
        sf.fill_rect(r.x, r.y + 5, r.w as u32, 6, t.well);
        sf.frame(r.x, r.y + 5, r.w as u32, 6, 1, t.line);
        for i in 0..=10 {
            let x = r.x + 6 + (r.w - 12) * i / 10;
            sf.fill_rect(x, r.y + 14, 1, 3, t.line);
        }
        if s.duration > 0.0 {
            let pos = self.dragging.unwrap_or(s.pos);
            let x = r.x + 6 + ((r.w - 12) as f64 * (pos / s.duration).clamp(0.0, 1.0)) as i32;
            bevel(&mut sf, x - 5, r.y, 11, 16, 1, false, t);
            sf.frame(x - 5, r.y, 11, 16, 1, t.line);
        }
        for (i, b) in BUTTONS.iter().enumerate() {
            let br = button_rect(i);
            let on = matches!(
                (b, s.paused, s.idle || s.path.is_empty()),
                (Btn::Play, false, false) | (Btn::Pause, true, false)
            );
            push_button(
                &mut sf,
                br.x,
                br.y,
                br.w,
                br.h,
                1,
                "",
                false,
                self.pressed == Some(i) || on,
                t,
            );
            let off = i32::from(self.pressed == Some(i) || on);
            glyph(
                &mut sf,
                *b,
                Rect::new(br.x + off, br.y + off, br.w, br.h),
                t.text,
            );
        }
        // Volume.
        sf.text(font, VOL.x - 30, VOL.y + 1, 1, t.text, None, "Vol");
        sf.fill_rect(VOL.x, VOL.y + 4, VOL.w as u32, 6, t.well);
        sf.frame(VOL.x, VOL.y + 4, VOL.w as u32, 6, 1, t.line);
        let vx = VOL.x + ((VOL.w - 8) as f64 * s.volume / 100.0) as i32;
        bevel(&mut sf, vx, VOL.y, 8, 14, 1, false, t);
        sf.frame(vx, VOL.y, 8, 14, 1, t.line);
        // Status line.
        let status = match (&self.player, &s.error) {
            (Err(e), _) => e.clone(),
            (_, Some(e)) => e.clone(),
            _ if s.path.is_empty() => String::new(),
            _ if s.idle => "Stopped.".into(),
            _ if s.paused => "Paused.".into(),
            _ => "Playing.".into(),
        };
        sf.fill_rect(0, H - 22, W as u32, 1, t.line);
        sf.text(
            font,
            10,
            H - 17,
            1,
            t.text,
            None,
            &ellipsize_in(font, &status, W - 20),
        );
    }

    fn window_event(&mut self, ui: &mut Ui, ev: Event) {
        match ev {
            Event::Close => return ui.quit(),
            Event::Button {
                button: LEFT_BUTTON,
                pressed,
                x,
                y,
            } => {
                if pressed {
                    if let Some(i) = (0..BUTTONS.len()).find(|i| button_rect(*i).contains(x, y)) {
                        self.pressed = Some(i);
                    } else if TRACK.contains(x, y) && self.state.duration > 0.0 {
                        self.dragging = Some(self.pos_at(x));
                    } else if Rect::new(VOL.x - 4, VOL.y, VOL.w + 8, VOL.h).contains(x, y) {
                        self.vol_drag = true;
                        self.set_volume_at(x);
                    }
                } else {
                    if let Some(i) = self.pressed.take() {
                        if button_rect(i).contains(x, y) {
                            self.press(ui, BUTTONS[i]);
                        }
                    }
                    if self.dragging.take().is_some() {
                        let target = self.pos_at(x);
                        if let Some(pl) = self.player() {
                            pl.seek(target);
                        }
                    }
                    self.vol_drag = false;
                }
            }
            Event::PointerMove { x, .. } => {
                if self.dragging.is_some() {
                    self.dragging = Some(self.pos_at(x));
                } else if self.vol_drag {
                    self.set_volume_at(x);
                } else {
                    return;
                }
            }
            Event::Key { sym, text, .. } => {
                let t = text.as_deref().unwrap_or("").to_ascii_lowercase();
                match sym {
                    ESCAPE => return ui.quit(),
                    SPACE => self.press(ui, Btn::Pause),
                    RETURN | KP_ENTER => self.press(ui, Btn::Play),
                    LEFT => self.press(ui, Btn::Back),
                    RIGHT => self.press(ui, Btn::Forward),
                    UP | DOWN => {
                        let v = self.state.volume + if sym == UP { 5.0 } else { -5.0 };
                        if let Some(p) = self.player() {
                            p.set_volume(v);
                        }
                    }
                    _ if t == "s" => self.press(ui, Btn::Stop),
                    _ if t == "o" => self.press(ui, Btn::Eject),
                    _ => return,
                }
            }
            _ => return,
        }
        self.refresh();
        if let Some(w) = self.win {
            ui.redraw(w);
        }
    }

    fn set_volume_at(&mut self, x: i32) {
        let v = ((x - VOL.x - 4) as f64 / (VOL.w - 8) as f64 * 100.0).clamp(0.0, 100.0);
        if let Some(p) = self.player() {
            p.set_volume(v);
        }
    }
}

impl Program for Media {
    fn start(&mut self, ui: &mut Ui) {
        // Started again without a file: bring the open one forward. (With a
        // file, another player opens, as Windows 3.1's did per device.)
        if self.first.is_none() {
            if let Some(w) = station::windows().iter().find(|w| w.title == TITLE) {
                let _ = station::request(&["SWITCH", &w.id.to_string()]);
                ui.quit();
                return;
            }
        }
        self.win = Some(ui.window(TITLE, "retroos.media-player", (W, H), false));
        if let Some(p) = self.first.take() {
            self.play(p);
        }
        ui.timer(TICK, Duration::from_millis(250));
    }

    fn draw(&mut self, ui: &Ui, id: SurfaceId, c: &mut Canvas) {
        let t = ui.theme;
        if Some(id) == self.win {
            self.draw_window(c, &t);
        } else if let Some(d) = self.open.as_ref().filter(|d| d.surface == id) {
            d.draw(c, &t);
        }
    }

    fn event(&mut self, ui: &mut Ui, id: SurfaceId, ev: Event) {
        if let Some(d) = self.open.as_mut().filter(|d| d.surface == id) {
            match d.event(ev) {
                opendlg::Out::None => ui.redraw(id),
                opendlg::Out::Cancel => {
                    ui.close(id);
                    self.open = None;
                }
                opendlg::Out::Chosen(p) => {
                    ui.close(id);
                    self.open = None;
                    self.play(p);
                    if let Some(w) = self.win {
                        ui.redraw(w);
                    }
                }
            }
            return;
        }
        if Some(id) == self.win {
            self.window_event(ui, ev);
        }
    }

    fn timer(&mut self, ui: &mut Ui, token: u32) {
        if token != TICK {
            return;
        }
        let before = self.state.clone();
        self.refresh();
        if self.state != before {
            if let Some(w) = self.win {
                ui.redraw(w);
            }
        }
        ui.timer(TICK, Duration::from_millis(250));
    }
}

fn main() {
    let first = std::env::args_os().nth(1).map(PathBuf::from);
    let player: Result<Box<dyn Player>, String> =
        if std::env::var_os("RETROOS_TEST_MEDIA").is_some() {
            Ok(Box::new(Fake::new()))
        } else {
            Mpv::start().map(|m| Box::new(m) as Box<dyn Player>)
        };
    let m = Media {
        win: None,
        player,
        state: State {
            idle: true,
            volume: 100.0,
            ..State::default()
        },
        pressed: None,
        dragging: None,
        vol_drag: false,
        open: None,
        first,
    };
    if let Err(e) = retro_ui::run(Box::new(m)) {
        eprintln!("media-player: {e}");
        std::process::exit(1);
    }
}

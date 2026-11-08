//! About This Computer (docs/DESIGN.md §9.2): RetroOS's version, the
//! computer's memory, and a bar per running program showing its share of
//! the memory in use, in the manner of "About This Macintosh". "RetroOS" is
//! everything that is not a program's: the kernel, Station, the System Bar,
//! services. Refreshed every two seconds; Enter, Esc or the close box end it.

mod mem;

use retro_gfx::font::{lantern_12, lantern_9};
use retro_gfx::icons::{icon_picture, IconArt};
use retro_gfx::widgets::{ellipsize, text_width};
use retro_gfx::{Canvas, Theme};
use retro_ui::keys::{ESCAPE, KP_ENTER, RETURN};
use retro_ui::{station, Event, Program, SurfaceId, Ui};
use std::time::Duration;

const TITLE: &str = "About This Computer";
const W: i32 = 440;
const ROW_H: i32 = 20;
const TOP: i32 = 84;
const MAX_ROWS: usize = 8;
const BAR_X: i32 = 248;
const BAR_W: i32 = 176;
const REFRESH: u32 = 1;

#[derive(Clone, Debug, PartialEq)]
struct Row {
    name: String,
    kb: u64,
}

struct About {
    win: Option<SurfaceId>,
    total_kb: u64,
    used_kb: u64,
    rows: Vec<Row>,
}

/// "1,234.5 MB" from kB.
fn mb(kb: u64) -> String {
    let tenths = (kb * 10 + 512) / 1024;
    let (whole, frac) = (tenths / 10, tenths % 10);
    let s = whole.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    format!("{out}.{frac} MB")
}

/// The rows: RetroOS first (what the programs do not account for), then
/// every program Station lists, by name; programs under 0.1 MB (no access
/// to their memory) are left out.
fn rows(used_kb: u64, programs: Vec<Row>) -> Vec<Row> {
    let mut programs: Vec<Row> = programs.into_iter().filter(|r| r.kb >= 100).collect();
    programs.sort_by_key(|r| r.name.to_lowercase());
    let theirs: u64 = programs.iter().map(|r| r.kb).sum();
    let mut v = vec![Row {
        name: "RetroOS".into(),
        kb: used_kb.saturating_sub(theirs),
    }];
    v.extend(programs);
    v.truncate(MAX_ROWS);
    v
}

impl About {
    fn refresh(&mut self) {
        let mi = mem::meminfo();
        self.total_kb = mi.get("MemTotal").copied().unwrap_or(0);
        let avail = mi.get("MemAvailable").copied().unwrap_or(0);
        self.used_kb = self.total_kb.saturating_sub(avail);
        let tree = mem::process_tree();
        let programs = station::request(&["TASKS"])
            .map(|(_, lines)| lines)
            .unwrap_or_default()
            .iter()
            .filter_map(|l| {
                let f: Vec<&str> = l.split('\t').collect();
                let pid: u32 = f.get(2)?.parse().ok()?;
                if pid == std::process::id() {
                    return None;
                }
                let title = f.get(6).filter(|t| !t.is_empty()).or(f.get(5))?;
                Some(Row {
                    name: title.to_string(),
                    kb: if pid > 0 {
                        mem::tree_pss(&tree, pid)
                    } else {
                        0
                    },
                })
            })
            .collect();
        self.rows = rows(self.used_kb, programs);
    }

    fn draw_window(&self, c: &mut Canvas, t: &Theme) {
        let w = c.width as i32;
        let mut sf = c.surface();
        sf.clear(t.face);
        let (f12, f9) = (lantern_12(), lantern_9());
        icon_picture(&mut sf, IconArt::Computer, 16, 14, 1, t);
        sf.text(f12, 60, 14, 1, t.text, None, "RetroOS");
        let ver = format!("Version 1.0 (build {})", env!("CARGO_PKG_VERSION"));
        sf.text(f9, 60, 31, 1, t.text, None, &ver);
        sf.text(
            f9,
            60,
            43,
            1,
            t.text,
            None,
            "© 1987–1993 Phosphor Computer Company",
        );
        let right = |s: &str| w - 16 - text_width(s);
        let total = format!("Total memory: {}", mb(self.total_kb));
        let unused = format!("Unused: {}", mb(self.total_kb.saturating_sub(self.used_kb)));
        sf.text(f12, right(&total), 14, 1, t.text, None, &total);
        sf.text(f12, right(&unused), 32, 1, t.text, None, &unused);
        sf.fill_rect(8, 62, (w - 16) as u32, 1, t.line);
        sf.fill_rect(8, 63, (w - 16) as u32, 1, t.light);
        sf.text(f9, 16, 68, 1, t.text, None, "Program");
        let head = "Memory";
        let hw = f9.text_width(head) as i32;
        sf.text(f9, BAR_X - 12 - hw, 68, 1, t.text, None, head);
        sf.text(f9, BAR_X, 68, 1, t.text, None, "Share of memory in use");
        let used = self.used_kb.max(1);
        for (i, r) in self.rows.iter().enumerate() {
            let y = TOP + i as i32 * ROW_H;
            let name = ellipsize(&r.name, BAR_X - 120);
            sf.text(f12, 16, y + 3, 1, t.text, None, &name);
            let size = mb(r.kb);
            sf.text(
                f12,
                BAR_X - 12 - text_width(&size),
                y + 3,
                1,
                t.text,
                None,
                &size,
            );
            // The bar: outlined, filled with the program's share.
            sf.fill_rect(BAR_X, y + 4, BAR_W as u32, 12, t.well);
            sf.frame(BAR_X, y + 4, BAR_W as u32, 12, 1, t.line);
            let fill = ((BAR_W - 2) as u64 * r.kb.min(used) / used) as u32;
            if fill > 0 {
                sf.fill_rect(BAR_X + 1, y + 5, fill, 10, t.active);
            }
        }
    }
}

impl Program for About {
    fn start(&mut self, ui: &mut Ui) {
        if let Some(w) = station::windows().iter().find(|w| w.title == TITLE) {
            let _ = station::request(&["SWITCH", &w.id.to_string()]);
            ui.quit();
            return;
        }
        self.refresh();
        let h = TOP + MAX_ROWS as i32 * ROW_H + 8;
        self.win = Some(ui.window(TITLE, "retroos.about", (W, h), true));
        ui.timer(REFRESH, Duration::from_secs(2));
    }

    fn draw(&mut self, ui: &Ui, id: SurfaceId, c: &mut Canvas) {
        if Some(id) == self.win {
            self.draw_window(c, &ui.theme);
        }
    }

    fn event(&mut self, ui: &mut Ui, _id: SurfaceId, ev: Event) {
        match ev {
            Event::Close
            | Event::Key {
                sym: ESCAPE | RETURN | KP_ENTER,
                ..
            } => ui.quit(),
            _ => {}
        }
    }

    fn timer(&mut self, ui: &mut Ui, token: u32) {
        if token == REFRESH {
            let before = (self.used_kb, self.rows.clone());
            self.refresh();
            if (self.used_kb, self.rows.clone()) != before {
                if let Some(w) = self.win {
                    ui.redraw(w);
                }
            }
            ui.timer(REFRESH, Duration::from_secs(2));
        }
    }
}

fn main() {
    let about = About {
        win: None,
        total_kb: 0,
        used_kb: 0,
        rows: Vec::new(),
    };
    if let Err(e) = retro_ui::run(Box::new(about)) {
        eprintln!("retro-about: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn megabytes_read_like_the_dos_tools() {
        assert_eq!(mb(8_048_000), "7,859.4 MB");
        assert_eq!(mb(512), "0.5 MB");
        assert_eq!(mb(0), "0.0 MB");
    }

    #[test]
    fn retroos_is_what_programs_do_not_use() {
        let r = rows(
            100_000,
            vec![
                Row {
                    name: "Firefox".into(),
                    kb: 60_000,
                },
                Row {
                    name: "DOS Prompt".into(),
                    kb: 4_000,
                },
                Row {
                    name: "hidden".into(),
                    kb: 0,
                },
            ],
        );
        let names: Vec<&str> = r.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["RetroOS", "DOS Prompt", "Firefox"]);
        assert_eq!(r[0].kb, 36_000);
    }
}

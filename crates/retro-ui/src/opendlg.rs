//! An Open dialog (Windows 3.1's File Open, simplified): the folder's path,
//! its folders ([name]) and the files whose extensions the program takes,
//! OK and Cancel. A double-click (or Enter) on a folder opens it, on a
//! file chooses it. The program creates the surface with [`Ui::dialog`] at
//! [`SIZE`], then passes draw and events here.

use crate::controls::{ListBox, ROW_H};
use crate::keys::*;
use crate::{Event, Rect, SurfaceId};
use retro_gfx::font::lantern_12;
use retro_gfx::widgets::{ellipsize_in, push_button};
use retro_gfx::{Canvas, Theme};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const SIZE: (i32, i32) = (380, 280);
const LIST: Rect = Rect {
    x: 10,
    y: 34,
    w: 260,
    h: 4 + 14 * ROW_H,
};
const LEFT_BUTTON: u32 = 0x110;

fn button(i: usize) -> Rect {
    Rect::new(SIZE.0 - 96, 34 + i as i32 * 32, 86, 24)
}

/// What happened in the dialog.
#[derive(Debug, PartialEq)]
pub enum Out {
    None,
    Cancel,
    Chosen(PathBuf),
}

pub struct OpenDlg {
    pub surface: SurfaceId,
    pub dir: PathBuf,
    /// File extensions shown (lower case, no dot); empty: every file.
    pub exts: Vec<String>,
    entries: Vec<(String, bool)>,
    list: ListBox,
    pressed: Option<usize>,
    last_click: Option<(usize, Instant)>,
}

/// "..", the folders, then the files with one of `exts`; no dot files.
pub fn entries(dir: &Path, exts: &[String]) -> Vec<(String, bool)> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            if e.path().is_dir() {
                dirs.push(name);
            } else {
                let ext = Path::new(&name)
                    .extension()
                    .map(|x| x.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if exts.is_empty() || exts.contains(&ext) {
                    files.push(name);
                }
            }
        }
    }
    dirs.sort_by_key(|n| n.to_lowercase());
    files.sort_by_key(|n| n.to_lowercase());
    let mut v = Vec::new();
    if dir.parent().is_some() {
        v.push(("..".to_string(), true));
    }
    v.extend(dirs.into_iter().map(|d| (d, true)));
    v.extend(files.into_iter().map(|f| (f, false)));
    v
}

impl OpenDlg {
    pub fn new(surface: SurfaceId, dir: PathBuf, exts: &[&str]) -> OpenDlg {
        let mut d = OpenDlg {
            surface,
            dir,
            exts: exts.iter().map(|e| e.to_ascii_lowercase()).collect(),
            entries: Vec::new(),
            list: ListBox::default(),
            pressed: None,
            last_click: None,
        };
        d.load();
        d
    }

    fn load(&mut self) {
        self.entries = entries(&self.dir, &self.exts);
        self.list = ListBox::new(
            self.entries
                .iter()
                .map(|(n, d)| if *d { format!("[{n}]") } else { n.clone() })
                .collect(),
        );
    }

    fn enter(&mut self, i: usize) -> Out {
        let Some((name, is_dir)) = self.entries.get(i).cloned() else {
            return Out::None;
        };
        if is_dir {
            self.dir = if name == ".." {
                self.dir
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| self.dir.clone())
            } else {
                self.dir.join(name)
            };
            self.load();
            Out::None
        } else {
            Out::Chosen(self.dir.join(name))
        }
    }

    pub fn draw(&self, c: &mut Canvas, t: &Theme) {
        let mut sf = c.surface();
        sf.clear(t.face);
        let font = lantern_12();
        // The folder as RetroOS names it: C:\MUSIC (the home is C:, / is Z:).
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| "/".into());
        let path = retro_dosfs::Dos::new(&home).display(&self.dir);
        sf.text(
            font,
            10,
            12,
            1,
            t.text,
            None,
            &ellipsize_in(font, &path, SIZE.0 - 20),
        );
        self.list.draw(&mut sf, LIST, true, t);
        for (i, l) in ["OK", "Cancel"].iter().enumerate() {
            let b = button(i);
            push_button(
                &mut sf,
                b.x,
                b.y,
                b.w,
                b.h,
                1,
                l,
                i == 0,
                self.pressed == Some(i),
                t,
            );
        }
    }

    pub fn event(&mut self, ev: Event) -> Out {
        match ev {
            Event::Close => return Out::Cancel,
            Event::Key { sym, .. } => match sym {
                ESCAPE => return Out::Cancel,
                RETURN | KP_ENTER => {
                    if let Some(i) = self.list.sel {
                        return self.enter(i);
                    }
                }
                BACKSPACE => return self.enter(0).min_none(),
                _ => {
                    self.list.key(sym, LIST.h);
                }
            },
            Event::Scroll { steps } => self.list.scroll(steps.round() as isize * 3, LIST.h),
            Event::Button {
                button: LEFT_BUTTON,
                pressed,
                x,
                y,
            } => {
                if !pressed {
                    let hit = self.pressed.take().filter(|b| button(*b).contains(x, y));
                    match hit {
                        Some(0) => {
                            if let Some(i) = self.list.sel {
                                return self.enter(i);
                            }
                        }
                        Some(_) => return Out::Cancel,
                        None => {}
                    }
                } else if let Some(b) = (0..2).find(|b| button(*b).contains(x, y)) {
                    self.pressed = Some(b);
                } else if LIST.contains(x, y) {
                    if let Some(i) = self.list.click(y - LIST.y) {
                        let double = self.last_click.is_some_and(|(j, t)| {
                            j == i && t.elapsed() < retro_gfx::prefs::double_click()
                        });
                        self.last_click = Some((i, Instant::now()));
                        if double {
                            self.last_click = None;
                            return self.enter(i);
                        }
                    }
                }
            }
            _ => {}
        }
        Out::None
    }
}

trait MinNone {
    fn min_none(self) -> Out;
}

impl MinNone for Out {
    /// Backspace goes up a folder and never chooses.
    fn min_none(self) -> Out {
        match self {
            Out::Chosen(_) => Out::None,
            o => o,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_then_matching_files() {
        let d = std::env::temp_dir().join(format!("retro-open-{}", std::process::id()));
        std::fs::create_dir_all(d.join("Music")).unwrap();
        for f in ["song.MP3", "notes.txt", ".hidden.mp3", "clip.mkv"] {
            std::fs::write(d.join(f), "").unwrap();
        }
        let e = entries(&d, &["mp3".into(), "mkv".into()]);
        let names: Vec<&str> = e.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["..", "Music", "clip.mkv", "song.MP3"]);
        std::fs::remove_dir_all(&d).unwrap();
    }
}

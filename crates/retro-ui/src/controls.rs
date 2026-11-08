//! Controls programs put in their windows and dialogs (docs/STYLE.md §4):
//! a menu bar, a one-line text field, a list box, a check box. Each keeps
//! its own state, takes keys and clicks in art pixels relative to where it
//! was drawn, and draws itself with the current [`Theme`].

use crate::keys::*;
use crate::Rect;
use retro_gfx::font::lantern_12;
use retro_gfx::widgets::{ellipsize, text_width, TEXT_Y};
use retro_gfx::{Surface, Theme};

// ------------------------------------------------------------ menu bar

/// Height of a menu bar, including its 1-pixel line.
pub const MENU_BAR_H: i32 = 20;

/// The titles of a window's menu bar ("File", "Edit", …): the first
/// letter is the mnemonic (Alt+letter).
pub struct MenuBar {
    pub titles: Vec<&'static str>,
}

impl MenuBar {
    pub fn new(titles: &[&'static str]) -> MenuBar {
        MenuBar {
            titles: titles.to_vec(),
        }
    }

    pub fn rects(&self) -> Vec<Rect> {
        let mut x = 2;
        self.titles
            .iter()
            .map(|t| {
                let w = text_width(t) + 14;
                let r = Rect::new(x, 0, w, MENU_BAR_H - 1);
                x += w;
                r
            })
            .collect()
    }

    pub fn hit(&self, x: i32, y: i32) -> Option<usize> {
        self.rects().iter().position(|r| r.contains(x, y))
    }

    /// The title whose mnemonic is `c`.
    pub fn by_letter(&self, c: char) -> Option<usize> {
        let c = c.to_ascii_lowercase();
        self.titles
            .iter()
            .position(|t| t.chars().next().map(|f| f.to_ascii_lowercase()) == Some(c))
    }

    /// Draw across `w` art pixels at the top; `open` is highlighted.
    pub fn draw(&self, sf: &mut Surface, w: i32, open: Option<usize>, t: &Theme) {
        sf.fill_rect(0, 0, w as u32, (MENU_BAR_H - 1) as u32, t.well);
        sf.fill_rect(0, MENU_BAR_H - 1, w as u32, 1, t.line);
        for (i, (r, title)) in self.rects().into_iter().zip(&self.titles).enumerate() {
            let (fg, bg) = if open == Some(i) {
                (t.sel_text, t.sel)
            } else {
                (t.text, t.well)
            };
            sf.fill_rect(r.x, r.y, r.w as u32, r.h as u32, bg);
            sf.text(lantern_12(), r.x + 7, TEXT_Y, 1, fg, None, title);
            let first: String = title.chars().take(1).collect();
            let uw = (text_width(&first) - 1).max(1) as u32;
            sf.fill_rect(r.x + 7, TEXT_Y + 11, uw, 1, fg);
        }
    }
}

// ---------------------------------------------------------- text field

/// Height of a text field.
pub const FIELD_H: i32 = 20;

/// A one-line text field (Lantern 12): typing, selection with Shift,
/// Home/End, Ctrl+A; the text scrolls to keep the cursor in view.
#[derive(Clone, Debug, Default)]
pub struct TextField {
    pub text: Vec<char>,
    pub cur: usize,
    pub anchor: Option<usize>,
    /// Shows `*` for every character (passwords); copying is left to the
    /// program, which should not.
    pub secret: bool,
}

impl TextField {
    pub fn new(s: &str) -> TextField {
        let text: Vec<char> = s.chars().collect();
        let n = text.len();
        TextField {
            text,
            cur: n,
            anchor: (n > 0).then_some(0),
            secret: false,
        }
    }

    pub fn value(&self) -> String {
        self.text.iter().collect()
    }

    pub fn set(&mut self, s: &str) {
        let secret = self.secret;
        *self = TextField::new(s);
        self.secret = secret;
    }

    /// An empty password field.
    pub fn password() -> TextField {
        TextField {
            secret: true,
            ..TextField::default()
        }
    }

    /// The character shown for `c`.
    fn shown(&self, c: char) -> char {
        if self.secret {
            '*'
        } else {
            c
        }
    }

    fn sel(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        (a != self.cur).then(|| (a.min(self.cur), a.max(self.cur)))
    }

    fn delete_sel(&mut self) -> bool {
        if let Some((a, b)) = self.sel() {
            self.text.drain(a..b);
            self.cur = a;
            self.anchor = None;
            true
        } else {
            self.anchor = None;
            false
        }
    }

    pub fn insert(&mut self, s: &str) {
        self.delete_sel();
        for c in s.chars().filter(|c| !c.is_control()) {
            self.text.insert(self.cur, c);
            self.cur += 1;
        }
    }

    /// Handle a key; false if the field does not use it (Enter, Tab, Esc…).
    pub fn key(&mut self, sym: u32, text: Option<&str>, ctrl: bool, shift: bool) -> bool {
        let mv = |f: &mut TextField, to: usize| {
            if shift {
                f.anchor.get_or_insert(f.cur);
            } else {
                f.anchor = None;
            }
            f.cur = to;
        };
        match sym {
            LEFT => mv(self, self.cur.saturating_sub(1)),
            RIGHT => mv(self, (self.cur + 1).min(self.text.len())),
            HOME => mv(self, 0),
            END => mv(self, self.text.len()),
            BACKSPACE => {
                if !self.delete_sel() && self.cur > 0 {
                    self.cur -= 1;
                    self.text.remove(self.cur);
                }
            }
            DELETE => {
                if !self.delete_sel() && self.cur < self.text.len() {
                    self.text.remove(self.cur);
                }
            }
            _ if ctrl && text.is_some_and(|t| t.eq_ignore_ascii_case("a") || t == "\u{1}") => {
                self.anchor = Some(0);
                self.cur = self.text.len();
            }
            _ if !ctrl => match text {
                Some(t) if !t.is_empty() && !t.chars().any(char::is_control) => self.insert(t),
                _ => return false,
            },
            _ => return false,
        }
        true
    }

    /// Draw at (x, y), `w` wide; the cursor and selection show when focused.
    pub fn draw(&self, sf: &mut Surface, x: i32, y: i32, w: i32, focused: bool, t: &Theme) {
        sf.fill_rect(x, y, w as u32, FIELD_H as u32, t.well);
        sf.frame(x, y, w as u32, FIELD_H as u32, 1, t.line);
        let font = lantern_12();
        let cw = |c: &char| font.glyph_or_replacement(self.shown(*c)).width as i32;
        // Scroll so that the cursor is inside the field.
        let inner = w - 8;
        let before: i32 = self.text[..self.cur].iter().map(cw).sum();
        let shift = (before - inner + 2).max(0);
        let mut cx = x + 4 - shift;
        let sel = self.sel().filter(|_| focused);
        for (i, c) in self.text.iter().enumerate() {
            let g = font.glyph_or_replacement(self.shown(*c));
            let gw = g.width as i32;
            if cx + gw > x + 2 && cx < x + w - 2 {
                let on = sel.is_some_and(|(a, b)| i >= a && i < b);
                let (fg, bg) = if on {
                    (t.sel_text, Some(t.sel))
                } else {
                    (t.text, None)
                };
                sf.glyph(g, cx, y + TEXT_Y + 1, 1, fg, bg);
            }
            cx += gw;
        }
        if focused {
            let caret = x + 4 - shift + before;
            sf.fill_rect(caret, y + 3, 1, (FIELD_H - 6) as u32, t.text);
        }
    }

    /// A click at `px` art pixels from the field's left edge.
    pub fn click(&mut self, px: i32) {
        let font = lantern_12();
        let mut x = 4;
        let mut at = self.text.len();
        for (i, c) in self.text.iter().enumerate() {
            let gw = font.glyph_or_replacement(self.shown(*c)).width as i32;
            if px < x + gw / 2 {
                at = i;
                break;
            }
            x += gw;
        }
        self.cur = at;
        self.anchor = None;
    }
}

// ------------------------------------------------------------ list box

/// Row height of a list box.
pub const ROW_H: i32 = 16;

/// A list of lines (Lantern 12) with one selected; scrolls to keep it in
/// view. Wheel and keys scroll it.
#[derive(Clone, Debug, Default)]
pub struct ListBox {
    pub items: Vec<String>,
    pub sel: Option<usize>,
    pub top: usize,
}

impl ListBox {
    pub fn new(items: Vec<String>) -> ListBox {
        let sel = (!items.is_empty()).then_some(0);
        ListBox { items, sel, top: 0 }
    }

    fn rows(h: i32) -> usize {
        ((h - 4) / ROW_H).max(1) as usize
    }

    fn reveal(&mut self, h: i32) {
        let rows = ListBox::rows(h);
        if let Some(s) = self.sel {
            if s < self.top {
                self.top = s;
            } else if s >= self.top + rows {
                self.top = s + 1 - rows;
            }
        }
    }

    /// Handle a key in a list `h` pixels tall; false if unused.
    pub fn key(&mut self, sym: u32, h: i32) -> bool {
        let n = self.items.len();
        if n == 0 {
            return false;
        }
        let rows = ListBox::rows(h);
        let s = self.sel.unwrap_or(0);
        self.sel = Some(match sym {
            UP => s.saturating_sub(1),
            DOWN => (s + 1).min(n - 1),
            HOME => 0,
            END => n - 1,
            PAGE_UP => s.saturating_sub(rows - 1),
            PAGE_DOWN => (s + rows - 1).min(n - 1),
            _ => return false,
        });
        self.reveal(h);
        true
    }

    /// The item at `py` pixels from the top of the list, selected.
    pub fn click(&mut self, py: i32) -> Option<usize> {
        let i = self.top + ((py - 2).max(0) / ROW_H) as usize;
        (i < self.items.len()).then(|| {
            self.sel = Some(i);
            i
        })
    }

    pub fn scroll(&mut self, d: isize, h: i32) {
        let max = self.items.len().saturating_sub(ListBox::rows(h));
        self.top = (self.top as isize + d).clamp(0, max as isize) as usize;
    }

    pub fn draw(&self, sf: &mut Surface, r: Rect, focused: bool, t: &Theme) {
        sf.fill_rect(r.x, r.y, r.w as u32, r.h as u32, t.well);
        sf.frame(r.x, r.y, r.w as u32, r.h as u32, 1, t.line);
        let font = lantern_12();
        for (row, i) in (self.top..self.items.len())
            .take(ListBox::rows(r.h))
            .enumerate()
        {
            let y = r.y + 2 + row as i32 * ROW_H;
            let text = ellipsize(&self.items[i], r.w - 10);
            let fg = if self.sel == Some(i) {
                let bg = if focused { t.sel } else { t.dark };
                sf.fill_rect(r.x + 2, y, (r.w - 4) as u32, ROW_H as u32, bg);
                t.sel_text
            } else {
                t.text
            };
            sf.text(font, r.x + 5, y + 2, 1, fg, None, &text);
        }
    }
}

// ----------------------------------------------------------- check box

/// A check box with its label at (x, y); 13 pixels tall.
pub fn check_box(sf: &mut Surface, x: i32, y: i32, label: &str, on: bool, t: &Theme) {
    sf.fill_rect(x, y, 13, 13, t.well);
    sf.frame(x, y, 13, 13, 1, t.line);
    let mark = t.text;
    if on {
        for i in 2..11 {
            sf.put(x + i, y + i, mark);
            sf.put(x + 12 - i, y + i, mark);
        }
    }
    sf.text(lantern_12(), x + 20, y, 1, t.text, None, label);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_types_selects_and_deletes() {
        let mut f = TextField::new("name.txt");
        // A new field starts with everything selected: typing replaces it.
        assert!(f.key(0x61, Some("a"), false, false));
        assert_eq!(f.value(), "a");
        f.key(HOME, None, false, false);
        f.key(END, None, false, true);
        f.key(BACKSPACE, None, false, false);
        assert_eq!(f.value(), "");
        assert!(
            !f.key(RETURN, Some("\r"), false, false),
            "Enter is the dialog's"
        );
    }

    #[test]
    fn a_list_keeps_its_selection_in_view() {
        let mut l = ListBox::new((0..30).map(|i| i.to_string()).collect());
        let h = 4 + 5 * ROW_H;
        l.key(END, h);
        assert_eq!((l.sel, l.top), (Some(29), 25));
        l.key(HOME, h);
        assert_eq!(l.top, 0);
        assert_eq!(l.click(2 + ROW_H * 2 + 1), Some(2));
    }

    #[test]
    fn menu_titles_by_letter() {
        let m = MenuBar::new(&["File", "Edit", "Search", "Help"]);
        assert_eq!(m.by_letter('S'), Some(2));
        let r = m.rects();
        assert_eq!(m.hit(r[1].x + 1, 5), Some(1));
    }
}

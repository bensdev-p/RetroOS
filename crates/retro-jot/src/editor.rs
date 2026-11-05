//! Jot's editing state over EDIT's [`Buffer`]: cursor, selection, the view
//! (first line and column shown) and undo. Pure, so it is tested without a
//! compositor; positions on screen are cells of Phosphor Mono.

use retro_edit::buffer::{Buffer, Pos};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    DocStart,
    DocEnd,
    WordLeft,
    WordRight,
}

pub struct Editor {
    pub buf: Buffer,
    pub cur: Pos,
    /// The other end of the selection, when there is one.
    pub anchor: Option<Pos>,
    /// First line and display column in view; the view's size in cells.
    pub top: usize,
    pub left: usize,
    pub rows: usize,
    pub cols: usize,
    /// The display column Up/Down aim for.
    want: Option<usize>,
    undo: Vec<(Vec<Vec<char>>, Pos)>,
}

const UNDO_MAX: usize = 200;

impl Editor {
    pub fn new(buf: Buffer) -> Editor {
        Editor {
            buf,
            cur: Pos::default(),
            anchor: None,
            top: 0,
            left: 0,
            rows: 20,
            cols: 60,
            want: None,
            undo: Vec::new(),
        }
    }

    /// The selection as (start, end), start first; None when empty.
    pub fn selection(&self) -> Option<(Pos, Pos)> {
        let a = self.anchor?;
        (a != self.cur).then(|| (a.min(self.cur), a.max(self.cur)))
    }

    pub fn selected_text(&self) -> Option<String> {
        self.selection().map(|(a, b)| self.buf.get(a, b))
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    fn snapshot(&mut self) {
        self.undo.push((self.buf.lines.clone(), self.cur));
        if self.undo.len() > UNDO_MAX {
            self.undo.remove(0);
        }
    }

    pub fn undo(&mut self) {
        if let Some((lines, cur)) = self.undo.pop() {
            self.buf.lines = lines;
            self.buf.modified = true;
            self.cur = self.buf.clamp(cur);
            self.anchor = None;
            self.want = None;
            self.reveal();
        }
    }

    /// Delete the selection (if any); true if something was deleted.
    fn delete_selection(&mut self) -> bool {
        match self.selection() {
            Some((a, b)) => {
                self.cur = self.buf.delete(a, b);
                self.anchor = None;
                true
            }
            None => {
                self.anchor = None;
                false
            }
        }
    }

    /// Type (or paste) text over the selection.
    pub fn insert(&mut self, s: &str) {
        self.snapshot();
        self.delete_selection();
        let s = s.replace("\r\n", "\n");
        self.cur = self.buf.insert(self.cur, &s);
        self.want = None;
        self.reveal();
    }

    pub fn backspace(&mut self) {
        self.snapshot();
        if !self.delete_selection() {
            let p = self.buf.prev(self.cur);
            self.cur = self.buf.delete(p, self.cur);
        }
        self.want = None;
        self.reveal();
    }

    pub fn delete(&mut self) {
        self.snapshot();
        if !self.delete_selection() {
            let n = self.buf.next(self.cur);
            self.cur = self.buf.delete(self.cur, n);
        }
        self.want = None;
        self.reveal();
    }

    /// Cut: the selection's text, removed.
    pub fn cut(&mut self) -> Option<String> {
        let t = self.selected_text()?;
        self.snapshot();
        self.delete_selection();
        self.reveal();
        Some(t)
    }

    pub fn select_all(&mut self) {
        let last = self.buf.lines.len() - 1;
        self.anchor = Some(Pos::new(0, 0));
        self.cur = Pos::new(last, self.buf.line_len(last));
        self.reveal();
    }

    /// Move the cursor; with `extend` the selection grows (Shift).
    pub fn go(&mut self, m: Move, extend: bool) {
        if extend {
            self.anchor.get_or_insert(self.cur);
        } else if let Some((a, b)) = self.selection() {
            // Left/Right without Shift collapse the selection to its edge.
            self.anchor = None;
            match m {
                Move::Left => {
                    self.cur = a;
                    return self.reveal();
                }
                Move::Right => {
                    self.cur = b;
                    return self.reveal();
                }
                _ => {}
            }
        } else {
            self.anchor = None;
        }
        let b = &self.buf;
        let last = b.lines.len() - 1;
        let vertical = matches!(m, Move::Up | Move::Down | Move::PageUp | Move::PageDown);
        let want = self
            .want
            .unwrap_or_else(|| b.display_col(self.cur.line, self.cur.col));
        let at_line = |line: usize| Pos::new(line, b.col_for_display(line, want));
        let page = self.rows.max(2) - 1;
        self.cur = match m {
            Move::Left => b.prev(self.cur),
            Move::Right => b.next(self.cur),
            Move::Up => at_line(self.cur.line.saturating_sub(1)),
            Move::Down => at_line((self.cur.line + 1).min(last)),
            Move::PageUp => {
                self.top = self.top.saturating_sub(page);
                at_line(self.cur.line.saturating_sub(page))
            }
            Move::PageDown => {
                self.top = (self.top + page).min(last);
                at_line((self.cur.line + page).min(last))
            }
            Move::Home => {
                // First Home: to the first non-blank; again: column 0.
                let line = &b.lines[self.cur.line];
                let indent = line.iter().take_while(|c| c.is_whitespace()).count();
                let col = if self.cur.col == indent { 0 } else { indent };
                Pos::new(self.cur.line, col)
            }
            Move::End => Pos::new(self.cur.line, b.line_len(self.cur.line)),
            Move::DocStart => Pos::new(0, 0),
            Move::DocEnd => Pos::new(last, b.line_len(last)),
            Move::WordLeft => b.word_left(self.cur),
            Move::WordRight => b.word_right(self.cur),
        };
        self.want = vertical.then_some(want);
        if self.anchor == Some(self.cur) && !extend {
            self.anchor = None;
        }
        self.reveal();
    }

    /// The text position at view cell (`row`, `col`).
    pub fn pos_at(&self, row: usize, col: usize) -> Pos {
        let line = (self.top + row).min(self.buf.lines.len() - 1);
        Pos::new(line, self.buf.col_for_display(line, self.left + col))
    }

    /// A click (or a drag with `extend`) at a view cell.
    pub fn click(&mut self, row: usize, col: usize, extend: bool) {
        let p = self.pos_at(row, col);
        if extend {
            self.anchor.get_or_insert(self.cur);
        } else {
            self.anchor = None;
        }
        self.cur = p;
        self.want = None;
        if self.anchor == Some(self.cur) {
            self.anchor = None;
        }
        self.reveal();
    }

    /// Scroll the view so the cursor shows.
    pub fn reveal(&mut self) {
        let rows = self.rows.max(1);
        if self.cur.line < self.top {
            self.top = self.cur.line;
        } else if self.cur.line >= self.top + rows {
            self.top = self.cur.line + 1 - rows;
        }
        let d = self.buf.display_col(self.cur.line, self.cur.col);
        let cols = self.cols.max(1);
        if d < self.left {
            self.left = d.saturating_sub(cols / 4);
        } else if d >= self.left + cols {
            self.left = d + 1 - cols + cols / 4;
        }
    }

    /// Scroll by whole lines (wheel, scroll bar) without moving the cursor.
    pub fn scroll_lines(&mut self, d: isize) {
        let max = self.buf.lines.len().saturating_sub(1);
        self.top = (self.top as isize + d).clamp(0, max as isize) as usize;
    }

    pub fn scroll_cols(&mut self, d: isize) {
        let max = self.longest();
        self.left = (self.left as isize + d).clamp(0, max as isize) as usize;
    }

    /// The widest line, in display columns (for the horizontal scroll bar).
    pub fn longest(&self) -> usize {
        (0..self.buf.lines.len())
            .map(|l| self.buf.display_col(l, self.buf.line_len(l)))
            .max()
            .unwrap_or(0)
    }

    /// Find `needle` after the cursor (or the selection), wrapping; selects it.
    pub fn find(&mut self, needle: &str, case: bool) -> bool {
        let from = self.selection().map_or(self.cur, |(a, _)| self.buf.next(a));
        let Some(p) = self.buf.find(needle, from, case) else {
            return false;
        };
        self.anchor = Some(p);
        self.cur = Pos::new(p.line, p.col + needle.chars().count());
        self.want = None;
        self.reveal();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ed(text: &str) -> Editor {
        Editor::new(Buffer::from_text(text))
    }

    #[test]
    fn typing_replaces_the_selection_and_undoes() {
        let mut e = ed("hello world\n");
        e.go(Move::WordRight, true);
        // Like Windows: Ctrl+Shift+Right takes the word and its space.
        assert_eq!(e.selected_text().as_deref(), Some("hello "));
        e.insert("goodbye ");
        assert_eq!(e.buf.text(), "goodbye world\n");
        e.undo();
        assert_eq!(e.buf.text(), "hello world\n");
    }

    #[test]
    fn cut_paste_and_lines() {
        let mut e = ed("one\ntwo\n");
        e.go(Move::End, true);
        assert_eq!(e.cut().as_deref(), Some("one"));
        e.go(Move::Down, false);
        e.go(Move::End, false);
        e.insert(" one\nthree");
        assert_eq!(e.buf.text(), "\ntwo one\nthree\n");
        assert_eq!(e.cur, Pos::new(2, 5));
    }

    #[test]
    fn up_down_keep_the_column() {
        let mut e = ed("abcdef\nab\nabcdef\n");
        e.go(Move::End, false);
        e.go(Move::Down, false);
        assert_eq!(e.cur, Pos::new(1, 2));
        e.go(Move::Down, false);
        assert_eq!(e.cur, Pos::new(2, 6), "back to the wanted column");
    }

    #[test]
    fn backspace_joins_lines_and_shift_left_right_collapse() {
        let mut e = ed("ab\ncd\n");
        e.go(Move::Down, false);
        e.go(Move::Home, false);
        e.backspace();
        assert_eq!(e.buf.text(), "abcd\n");
        e.go(Move::DocStart, false);
        e.go(Move::Right, true);
        e.go(Move::Right, true);
        e.go(Move::Left, false);
        assert_eq!((e.cur, e.anchor), (Pos::new(0, 0), None));
    }

    #[test]
    fn the_view_follows_the_cursor() {
        let mut e = ed(&"x\n".repeat(100));
        e.rows = 10;
        e.go(Move::DocEnd, false);
        assert!(e.top + 10 > e.cur.line && e.cur.line >= e.top);
        e.go(Move::DocStart, false);
        assert_eq!(e.top, 0);
    }

    #[test]
    fn find_selects_and_wraps() {
        let mut e = ed("cat dog cat\n");
        assert!(e.find("CAT", false));
        assert_eq!(e.selection(), Some((Pos::new(0, 0), Pos::new(0, 3))));
        assert!(e.find("cat", true));
        assert_eq!(e.selection(), Some((Pos::new(0, 8), Pos::new(0, 11))));
        assert!(e.find("cat", true), "wraps to the first");
        assert_eq!(e.cur, Pos::new(0, 3));
        assert!(!e.find("bird", false));
    }

    #[test]
    fn clicks_map_cells_through_tabs() {
        let mut e = ed("\tab\n");
        e.click(0, 9, false);
        assert_eq!(e.cur, Pos::new(0, 2));
        e.click(0, 0, true);
        assert_eq!(e.selected_text().as_deref(), Some("\ta"));
    }
}

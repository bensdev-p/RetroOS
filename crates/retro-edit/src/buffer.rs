//! The text being edited: lines of chars, positions as (line, char index).

use std::path::{Path, PathBuf};

pub const TAB: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

impl Pos {
    pub fn new(line: usize, col: usize) -> Pos {
        Pos { line, col }
    }
}

pub struct Buffer {
    pub lines: Vec<Vec<char>>,
    pub path: Option<PathBuf>,
    /// Save with CR LF line ends (the file had them).
    pub crlf: bool,
    /// The file ended without a final newline.
    pub no_final_newline: bool,
    pub modified: bool,
}

impl Default for Buffer {
    fn default() -> Buffer {
        Buffer {
            lines: vec![Vec::new()],
            path: None,
            crlf: false,
            no_final_newline: false,
            modified: false,
        }
    }
}

impl Buffer {
    pub fn from_text(text: &str) -> Buffer {
        let crlf = text.contains("\r\n");
        let no_final_newline = !text.is_empty() && !text.ends_with('\n');
        let mut lines: Vec<Vec<char>> = text
            .split('\n')
            .map(|l| l.strip_suffix('\r').unwrap_or(l).chars().collect())
            .collect();
        if text.ends_with('\n') {
            lines.pop();
        }
        if lines.is_empty() {
            lines.push(Vec::new());
        }
        Buffer {
            lines,
            path: None,
            crlf,
            no_final_newline,
            modified: false,
        }
    }

    /// Open a file; a missing file gives an empty buffer with that name.
    pub fn open(path: &Path) -> std::io::Result<Buffer> {
        let mut b = match std::fs::read(path) {
            Ok(bytes) => Buffer::from_text(&String::from_utf8_lossy(&bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Buffer::default(),
            Err(e) => return Err(e),
        };
        b.path = Some(path.to_path_buf());
        Ok(b)
    }

    pub fn text(&self) -> String {
        let nl = if self.crlf { "\r\n" } else { "\n" };
        let mut s = self
            .lines
            .iter()
            .map(|l| l.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join(nl);
        if !self.no_final_newline && !(self.lines.len() == 1 && self.lines[0].is_empty()) {
            s.push_str(nl);
        }
        s
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        let path = self.path.clone().ok_or(std::io::ErrorKind::InvalidInput)?;
        // Write next to the file and rename, so a full disk never eats the original.
        let tmp = path.with_file_name(format!(
            ".{}.edit-tmp",
            path.file_name().unwrap_or_default().to_string_lossy()
        ));
        std::fs::write(&tmp, self.text())?;
        if let Ok(m) = std::fs::metadata(&path) {
            let _ = std::fs::set_permissions(&tmp, m.permissions());
        }
        std::fs::rename(&tmp, &path)?;
        self.modified = false;
        Ok(())
    }

    pub fn line_len(&self, line: usize) -> usize {
        self.lines.get(line).map_or(0, Vec::len)
    }

    pub fn clamp(&self, p: Pos) -> Pos {
        let line = p.line.min(self.lines.len() - 1);
        Pos::new(line, p.col.min(self.line_len(line)))
    }

    pub fn insert(&mut self, p: Pos, s: &str) -> Pos {
        let p = self.clamp(p);
        let tail: Vec<char> = self.lines[p.line].split_off(p.col);
        let mut line = p.line;
        for (i, part) in s.split('\n').enumerate() {
            if i > 0 {
                line += 1;
                self.lines.insert(line, Vec::new());
            }
            self.lines[line].extend(part.chars().filter(|c| *c != '\r'));
        }
        let col = self.lines[line].len();
        self.lines[line].extend(tail);
        self.modified = true;
        Pos::new(line, col)
    }

    /// Text between two positions (any order).
    pub fn get(&self, a: Pos, b: Pos) -> String {
        let (a, b) = (self.clamp(a.min(b)), self.clamp(a.max(b)));
        if a.line == b.line {
            return self.lines[a.line][a.col..b.col].iter().collect();
        }
        let mut s: String = self.lines[a.line][a.col..].iter().collect();
        for l in a.line + 1..b.line {
            s.push('\n');
            s.extend(self.lines[l].iter());
        }
        s.push('\n');
        s.extend(self.lines[b.line][..b.col].iter());
        s
    }

    /// Delete between two positions; returns the start.
    pub fn delete(&mut self, a: Pos, b: Pos) -> Pos {
        let (a, b) = (self.clamp(a.min(b)), self.clamp(a.max(b)));
        if a == b {
            return a;
        }
        let tail: Vec<char> = self.lines[b.line][b.col..].to_vec();
        self.lines[a.line].truncate(a.col);
        self.lines[a.line].extend(tail);
        self.lines.drain(a.line + 1..=b.line);
        self.modified = true;
        a
    }

    /// The position one character before/after `p`, crossing line ends.
    pub fn prev(&self, p: Pos) -> Pos {
        if p.col > 0 {
            Pos::new(p.line, p.col - 1)
        } else if p.line > 0 {
            Pos::new(p.line - 1, self.line_len(p.line - 1))
        } else {
            p
        }
    }

    pub fn next(&self, p: Pos) -> Pos {
        if p.col < self.line_len(p.line) {
            Pos::new(p.line, p.col + 1)
        } else if p.line + 1 < self.lines.len() {
            Pos::new(p.line + 1, 0)
        } else {
            p
        }
    }

    /// Start of the previous/next word (Ctrl+Left/Right).
    pub fn word_left(&self, mut p: Pos) -> Pos {
        p = self.prev(p);
        while p != Pos::default() && !self.is_word(p) {
            p = self.prev(p);
        }
        while p.col > 0 && self.is_word(Pos::new(p.line, p.col - 1)) {
            p.col -= 1;
        }
        p
    }

    pub fn word_right(&self, mut p: Pos) -> Pos {
        let end = Pos::new(self.lines.len() - 1, self.line_len(self.lines.len() - 1));
        while p != end && self.is_word(p) {
            p = self.next(p);
        }
        while p != end && !self.is_word(p) {
            p = self.next(p);
        }
        p
    }

    fn is_word(&self, p: Pos) -> bool {
        self.lines[p.line]
            .get(p.col)
            .is_some_and(|c| c.is_alphanumeric() || *c == '_')
    }

    /// Screen column of a char index (tabs expand to stops of 8).
    pub fn display_col(&self, line: usize, col: usize) -> usize {
        let mut d = 0;
        for c in self.lines[line].iter().take(col) {
            d = if *c == '\t' {
                (d / TAB + 1) * TAB
            } else {
                d + 1
            };
        }
        d
    }

    /// Char index for a screen column.
    pub fn col_for_display(&self, line: usize, want: usize) -> usize {
        let mut d = 0;
        for (i, c) in self.lines[line].iter().enumerate() {
            let nd = if *c == '\t' {
                (d / TAB + 1) * TAB
            } else {
                d + 1
            };
            if nd > want {
                return i;
            }
            d = nd;
        }
        self.line_len(line)
    }

    /// Find `needle` after `from` (wrapping); case-insensitive unless `case`.
    pub fn find(&self, needle: &str, from: Pos, case: bool) -> Option<Pos> {
        if needle.is_empty() || needle.contains('\n') {
            return None;
        }
        let norm = |c: char| {
            if case {
                c
            } else {
                c.to_lowercase().next().unwrap_or(c)
            }
        };
        let n: Vec<char> = needle.chars().map(norm).collect();
        let hit = |line: usize, from_col: usize, to_col: usize| -> Option<usize> {
            let hay: Vec<char> = self.lines[line].iter().map(|c| norm(*c)).collect();
            let last = to_col.min(hay.len().checked_sub(n.len())?);
            (from_col..=last).find(|&c| hay[c..c + n.len()] == n[..])
        };
        // From the cursor to the end, then wrap around to the cursor.
        for line in from.line..self.lines.len() {
            let start = if line == from.line { from.col } else { 0 };
            if let Some(c) = hit(line, start, usize::MAX) {
                return Some(Pos::new(line, c));
            }
        }
        for line in 0..=from.line {
            let end = if line == from.line {
                from.col.saturating_sub(1)
            } else {
                usize::MAX
            };
            if line == from.line && from.col == 0 {
                break;
            }
            if let Some(c) = hit(line, 0, end) {
                return Some(Pos::new(line, c));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_line_endings() {
        for t in ["a\nb\n", "a\r\nb\r\n", "no newline", "", "x\n\ny\n"] {
            assert_eq!(Buffer::from_text(t).text(), t, "{t:?}");
        }
    }

    #[test]
    fn insert_delete_get() {
        let mut b = Buffer::from_text("hello world\n");
        let p = b.insert(Pos::new(0, 5), ",\nnew");
        assert_eq!(p, Pos::new(1, 3));
        assert_eq!(b.text(), "hello,\nnew world\n");
        assert_eq!(b.get(Pos::new(1, 0), Pos::new(0, 4)), "o,\n");
        let s = b.delete(Pos::new(0, 5), Pos::new(1, 3));
        assert_eq!(s, Pos::new(0, 5));
        assert_eq!(b.text(), "hello world\n");
        assert!(b.modified);
    }

    #[test]
    fn tabs_words_find() {
        let b = Buffer::from_text("\tab cd\nEnd here\n");
        assert_eq!(b.display_col(0, 1), 8);
        assert_eq!(b.col_for_display(0, 9), 2);
        assert_eq!(b.col_for_display(0, 3), 0);
        assert_eq!(b.word_right(Pos::new(0, 1)), Pos::new(0, 4));
        assert_eq!(b.word_left(Pos::new(0, 6)), Pos::new(0, 4));
        assert_eq!(b.find("end", Pos::new(0, 0), false), Some(Pos::new(1, 0)));
        assert_eq!(b.find("end", Pos::new(0, 0), true), None);
        assert_eq!(b.find("ab", Pos::new(1, 2), false), Some(Pos::new(0, 1)));
    }
}

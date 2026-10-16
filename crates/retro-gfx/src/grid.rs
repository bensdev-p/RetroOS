//! A simple text-mode cell grid rendered with a bitmap font at an integer
//! scale, centred on the screen with an overscan border. Used by screens that
//! run without a terminal emulator (POST, Startup Menu, Setup).

use crate::font::Font;
use crate::palette::Scheme;
use crate::surface::Canvas;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub fg: u8,
    pub bg: u8,
}

pub struct Grid {
    pub cols: u32,
    pub rows: u32,
    pub cells: Vec<Cell>,
    pub scheme: Scheme,
    pub fg: u8,
    pub bg: u8,
}

/// Pick the largest integer scale at which `min_cols`×`min_rows` cells fit.
pub fn fit_scale(
    width: u32,
    height: u32,
    cell_w: u32,
    cell_h: u32,
    min_cols: u32,
    min_rows: u32,
) -> u32 {
    let sx = width / (cell_w * min_cols);
    let sy = height / (cell_h * min_rows);
    sx.min(sy).max(1)
}

impl Grid {
    pub fn new(cols: u32, rows: u32, scheme: &Scheme) -> Grid {
        let blank = Cell {
            ch: ' ',
            fg: scheme.fg,
            bg: scheme.bg,
        };
        Grid {
            cols,
            rows,
            cells: vec![blank; (cols * rows) as usize],
            scheme: scheme.clone(),
            fg: scheme.fg,
            bg: scheme.bg,
        }
    }

    pub fn clear(&mut self) {
        let blank = Cell {
            ch: ' ',
            fg: self.fg,
            bg: self.bg,
        };
        self.cells.fill(blank);
    }

    pub fn set(&mut self, x: u32, y: u32, c: Cell) {
        if x < self.cols && y < self.rows {
            self.cells[(y * self.cols + x) as usize] = c;
        }
    }

    /// Write text at (x, y) with colours; returns the column after the text.
    pub fn put(&mut self, x: u32, y: u32, s: &str, fg: u8, bg: u8) -> u32 {
        let mut cx = x;
        for ch in s.chars() {
            self.set(cx, y, Cell { ch, fg, bg });
            cx += 1;
        }
        cx
    }

    /// Text in the default colours.
    pub fn text(&mut self, x: u32, y: u32, s: &str) -> u32 {
        self.put(x, y, s, self.fg, self.bg)
    }

    pub fn fill(&mut self, x: u32, y: u32, w: u32, h: u32, ch: char, fg: u8, bg: u8) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.set(xx, yy, Cell { ch, fg, bg });
            }
        }
    }

    /// Double-line box with a one-cell drop shadow, like a DOS dialog.
    pub fn dialog(
        &mut self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        title: &str,
        fg: u8,
        bg: u8,
        shadow: bool,
    ) {
        self.fill(x, y, w, h, ' ', fg, bg);
        for xx in x + 1..x + w - 1 {
            self.set(xx, y, Cell { ch: '═', fg, bg });
            self.set(xx, y + h - 1, Cell { ch: '═', fg, bg });
        }
        for yy in y + 1..y + h - 1 {
            self.set(x, yy, Cell { ch: '║', fg, bg });
            self.set(x + w - 1, yy, Cell { ch: '║', fg, bg });
        }
        self.set(x, y, Cell { ch: '╔', fg, bg });
        self.set(x + w - 1, y, Cell { ch: '╗', fg, bg });
        self.set(x, y + h - 1, Cell { ch: '╚', fg, bg });
        self.set(x + w - 1, y + h - 1, Cell { ch: '╝', fg, bg });
        if !title.is_empty() {
            let t = format!(" {title} ");
            let tx = x + (w.saturating_sub(t.chars().count() as u32)) / 2;
            self.put(tx, y, &t, fg, bg);
        }
        if shadow {
            for yy in y + 1..=y + h {
                for dx in 0..2 {
                    self.shade(x + w + dx, yy);
                }
            }
            for xx in x + 2..x + w + 2 {
                self.shade(xx, y + h);
            }
        }
    }

    fn shade(&mut self, x: u32, y: u32) {
        if x < self.cols && y < self.rows {
            let c = &mut self.cells[(y * self.cols + x) as usize];
            c.bg = 0;
            c.fg = 8;
        }
    }

    /// Render centred into `canvas` at `scale`, filling the rest with the border colour.
    pub fn render(&self, font: &Font, canvas: &mut Canvas, scale: u32) {
        let cw = font.cell_w as u32 * scale;
        let ch = font.cell_h as u32 * scale;
        let ox = (canvas.width.saturating_sub(self.cols * cw) / 2) as i32;
        let oy = (canvas.height.saturating_sub(self.rows * ch) / 2) as i32;
        let mut s = canvas.surface();
        s.clear(self.scheme.color(self.scheme.border));
        for y in 0..self.rows {
            for x in 0..self.cols {
                let c = self.cells[(y * self.cols + x) as usize];
                let px = ox + (x * cw) as i32;
                let py = oy + (y * ch) as i32;
                let g = font.glyph_or_replacement(c.ch);
                s.fill_rect(px, py, cw, ch, self.scheme.color(c.bg));
                s.glyph(g, px, py, scale, self.scheme.color(c.fg), None);
            }
        }
    }

    /// Origin and cell size in pixels for overlay drawing (logos etc.).
    pub fn geometry(&self, font: &Font, canvas: &Canvas, scale: u32) -> (i32, i32, u32, u32) {
        let cw = font.cell_w as u32 * scale;
        let ch = font.cell_h as u32 * scale;
        let ox = (canvas.width.saturating_sub(self.cols * cw) / 2) as i32;
        let oy = (canvas.height.saturating_sub(self.rows * ch) / 2) as i32;
        (ox, oy, cw, ch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_selection() {
        assert_eq!(fit_scale(1920, 1080, 8, 16, 80, 25), 2);
        assert_eq!(fit_scale(3840, 2160, 8, 16, 80, 25), 5);
        assert_eq!(fit_scale(1024, 768, 8, 16, 80, 25), 1);
        assert_eq!(fit_scale(640, 400, 8, 16, 80, 25), 1);
    }
}

//! Where Program Manager's group windows and icons go (art pixels, in the
//! workspace under the menu bar, before scrolling). Pure, so it is tested
//! without a compositor.

use retro_ui::Rect;

/// Menu bar height, including its 1-pixel line.
pub const MENU_H: i32 = 20;
/// Space between group windows and around them.
pub const GAP: i32 = 6;
/// A group window's frame: 1-pixel outline plus face colour.
pub const BORDER: i32 = 3;
/// A group window's title bar.
pub const TITLE: i32 = 18;
/// Padding inside a group's client area.
pub const PAD: i32 = 2;
/// One icon with its label.
pub const CELL_W: i32 = 76;
pub const CELL_H: i32 = 54;
/// A group window is at least this many icons wide.
pub const MIN_PER_ROW: i32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupBox {
    pub rect: Rect,
    /// Icons per row in this group.
    pub per_row: i32,
}

/// Group windows tiled in rows across `width` (Window → Tile): as many
/// columns as fit groups of at least [`MIN_PER_ROW`] icons, each group as
/// tall as its icons need. Returns the boxes and the content height.
pub fn tile(width: i32, counts: &[usize]) -> (Vec<GroupBox>, i32) {
    if counts.is_empty() {
        return (Vec::new(), 0);
    }
    let min_w = MIN_PER_ROW * CELL_W + 2 * (BORDER + PAD);
    let cols = ((width - GAP) / (min_w + GAP)).clamp(1, counts.len() as i32);
    let gw = ((width - GAP * (cols + 1)) / cols).max(min_w);
    let per_row = ((gw - 2 * (BORDER + PAD)) / CELL_W).max(1);
    let mut boxes = Vec::with_capacity(counts.len());
    let mut y = GAP;
    for row in counts.chunks(cols as usize) {
        let mut row_h = 0;
        for (c, &n) in row.iter().enumerate() {
            let rows = ((n as i32 + per_row - 1) / per_row).max(1);
            let h = 2 * BORDER + TITLE + 1 + 2 * PAD + rows * CELL_H;
            row_h = row_h.max(h);
            let x = GAP + c as i32 * (gw + GAP);
            boxes.push(GroupBox {
                rect: Rect::new(x, y, gw, h),
                per_row,
            });
        }
        y += row_h + GAP;
    }
    (boxes, y)
}

/// The cell of icon `i` in a group window.
pub fn icon_rect(b: &GroupBox, i: usize) -> Rect {
    let i = i as i32;
    Rect::new(
        b.rect.x + BORDER + PAD + (i % b.per_row) * CELL_W,
        b.rect.y + BORDER + TITLE + 1 + PAD + (i / b.per_row) * CELL_H,
        CELL_W,
        CELL_H,
    )
}

/// The title bar of a group window.
pub fn title_rect(b: &GroupBox) -> Rect {
    Rect::new(
        b.rect.x + BORDER,
        b.rect.y + BORDER,
        b.rect.w - 2 * BORDER,
        TITLE,
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Icon(usize, usize),
    Group(usize),
}

/// What is at workspace point (x, y) (scroll already added).
pub fn hit(boxes: &[GroupBox], counts: &[usize], x: i32, y: i32) -> Option<Hit> {
    let g = boxes.iter().position(|b| b.rect.contains(x, y))?;
    let icon = (0..counts[g]).find(|&i| icon_rect(&boxes[g], i).contains(x, y));
    Some(icon.map_or(Hit::Group(g), |i| Hit::Icon(g, i)))
}

/// The icon reached from `i` by moving (dx, dy) in a grid of `per_row`
/// columns and `n` icons; stays put at the edges.
pub fn step(i: usize, n: usize, per_row: i32, dx: i32, dy: i32) -> usize {
    if n == 0 {
        return 0;
    }
    let (per_row, i) = (per_row.max(1), i as i32);
    let (col, row) = (i % per_row, i / per_row);
    let last = n as i32 - 1;
    let to = if dx != 0 {
        let c = (col + dx).clamp(0, per_row - 1);
        (row * per_row + c).min(last)
    } else {
        let r = (row + dy).max(0);
        let t = r * per_row + col;
        if t > last {
            // Down from a row above a short last row: its last icon.
            if r * per_row <= last {
                last
            } else {
                i
            }
        } else {
            t
        }
    };
    to as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_tile_in_columns_that_fit() {
        // 500 px: two columns of groups at least three icons wide.
        let (b, h) = tile(500, &[2, 7, 1]);
        assert_eq!(b.len(), 3);
        assert_eq!(b[0].rect.y, b[1].rect.y, "first two side by side");
        assert!(b[2].rect.y > b[0].rect.y, "third on the next row");
        assert!(b[0].rect.x + b[0].rect.w + GAP <= b[1].rect.x);
        assert!(b[1].rect.x + b[1].rect.w <= 500 - GAP);
        assert_eq!(b[0].per_row, 3);
        // Seven icons need three rows; the row is as tall as its tallest group.
        assert_eq!(b[1].rect.h, 2 * BORDER + TITLE + 1 + 2 * PAD + 3 * CELL_H);
        assert_eq!(h, b[2].rect.y + b[2].rect.h + GAP);
    }

    #[test]
    fn narrow_windows_get_one_column() {
        let (b, _) = tile(200, &[3, 3]);
        assert_eq!(b[0].rect.x, b[1].rect.x);
        assert!(b[0].per_row >= 1);
    }

    #[test]
    fn hits_icons_and_groups() {
        let counts = [2, 4];
        let (b, _) = tile(500, &counts);
        let r = icon_rect(&b[1], 3);
        assert_eq!(hit(&b, &counts, r.x + 5, r.y + 5), Some(Hit::Icon(1, 3)));
        let t = title_rect(&b[0]);
        assert_eq!(hit(&b, &counts, t.x + 5, t.y + 5), Some(Hit::Group(0)));
        assert_eq!(hit(&b, &counts, 1, 1), None);
    }

    #[test]
    fn arrows_move_in_the_grid() {
        // 3 per row, 7 icons: rows [0 1 2] [3 4 5] [6].
        assert_eq!(step(0, 7, 3, 1, 0), 1);
        assert_eq!(step(2, 7, 3, 1, 0), 2, "right edge");
        assert_eq!(step(1, 7, 3, 0, 1), 4);
        assert_eq!(step(5, 7, 3, 0, 1), 6, "down to the short last row");
        assert_eq!(step(6, 7, 3, 0, 1), 6);
        assert_eq!(step(4, 7, 3, 0, -1), 1);
        assert_eq!(step(0, 7, 3, 0, -1), 0);
    }
}

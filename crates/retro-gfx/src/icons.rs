//! Icons drawn in code until the icon set (HANDOFF §7 step 7): the 32×32
//! pictures Station shows for minimised windows and Program Manager shows
//! for programs, in the colours of the current [`Theme`].

use crate::palette::{BLACK, COBALT, LEMON, LIME, OCHRE, SILVER, SLATE, WHITE};
use crate::{Rgb, Surface, Theme};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconArt {
    /// A generic program window.
    Program,
    /// A DOS Prompt (the RetroDOS console).
    Dos,
    /// The computer itself (About This Computer).
    Computer,
    /// A folder (Ochre, a tab at the top left).
    Folder,
    /// A document: a white page with its corner folded.
    Document,
}

/// The 32×32 picture of an icon at physical (x, y), in the theme's colours.
pub fn icon_picture(sf: &mut Surface, art: IconArt, x: i32, y: i32, p: i32, th: &Theme) {
    let mut at = |ax: i32, ay: i32, w: i32, h: i32, col: Rgb| {
        let col = th.picture(col);
        sf.fill_rect(x + ax * p, y + ay * p, (w * p) as u32, (h * p) as u32, col)
    };
    if art == IconArt::Folder {
        at(2, 6, 12, 4, BLACK);
        at(3, 7, 10, 3, OCHRE);
        at(2, 9, 28, 19, BLACK);
        at(3, 10, 26, 17, OCHRE);
        at(3, 10, 26, 1, LEMON);
        at(3, 10, 1, 17, LEMON);
        at(4, 26, 25, 1, SLATE);
        return;
    }
    if art == IconArt::Document {
        at(7, 2, 18, 28, BLACK);
        at(8, 3, 16, 26, WHITE);
        // The folded corner: a light triangle over the page's corner.
        at(20, 2, 5, 1, BLACK);
        for i in 0..5 {
            at(19 + i, 3 + i, 1, 1, BLACK);
            at(20 + i, 3 + i, 4 - i, 1, SILVER);
        }
        for (y, w) in [(11, 12), (14, 13), (17, 10), (20, 12), (23, 8)] {
            at(10, y, w, 1, SLATE);
        }
        return;
    }
    if art == IconArt::Computer {
        // A monitor on a desktop case, light from the top left.
        at(5, 1, 22, 18, BLACK);
        at(6, 2, 20, 16, SILVER);
        at(8, 4, 16, 11, COBALT);
        at(9, 5, 5, 1, WHITE);
        at(13, 19, 6, 2, BLACK);
        at(2, 21, 28, 9, BLACK);
        at(3, 22, 26, 7, SILVER);
        at(3, 22, 26, 1, WHITE);
        at(17, 25, 9, 1, BLACK);
        at(5, 25, 2, 1, LIME);
        at(3, 28, 26, 1, SLATE);
        return;
    }
    // A small window: shadow, outline, title bar, client area.
    at(4, 5, 27, 24, SLATE);
    at(2, 3, 27, 24, BLACK);
    at(3, 4, 25, 4, COBALT);
    at(3, 8, 25, 1, BLACK);
    match art {
        IconArt::Program => {
            at(3, 9, 25, 17, WHITE);
            at(5, 11, 12, 1, SLATE);
            at(5, 13, 18, 1, SLATE);
            at(5, 15, 15, 1, SLATE);
        }
        IconArt::Computer | IconArt::Folder | IconArt::Document => {}
        IconArt::Dos => {
            at(3, 9, 25, 17, BLACK);
            // "C:\>_" in tiny letters, 5 pixels tall.
            const LETTERS: [[&str; 5]; 5] = [
                ["###", "#..", "#..", "#..", "###"],
                [".", "#", ".", "#", "."],
                ["#..", "#..", ".#.", "..#", "..#"],
                ["#..", ".#.", "..#", ".#.", "#.."],
                ["...", "...", "...", "...", "###"],
            ];
            let mut lx = 5;
            for l in LETTERS {
                for (ry, row) in l.iter().enumerate() {
                    for (rx, ch) in row.chars().enumerate() {
                        if ch == '#' {
                            at(lx + rx as i32, 12 + ry as i32, 1, 1, SILVER);
                        }
                    }
                }
                lx += l[0].len() as i32 + 1;
            }
        }
    }
}

/// The 16×16 picture of an icon (lists, menus) at physical (x, y).
pub fn small_icon(sf: &mut Surface, art: IconArt, x: i32, y: i32, p: i32, th: &Theme) {
    let mut at = |ax: i32, ay: i32, w: i32, h: i32, col: Rgb| {
        let col = th.picture(col);
        sf.fill_rect(x + ax * p, y + ay * p, (w * p) as u32, (h * p) as u32, col)
    };
    match art {
        IconArt::Folder => {
            at(1, 3, 6, 3, BLACK);
            at(2, 4, 4, 2, OCHRE);
            at(1, 5, 14, 9, BLACK);
            at(2, 6, 12, 7, OCHRE);
            at(2, 6, 12, 1, LEMON);
        }
        IconArt::Document => {
            at(3, 1, 10, 14, BLACK);
            at(4, 2, 8, 12, WHITE);
            at(9, 2, 3, 3, SILVER);
            for (yy, w) in [(6, 6), (8, 6), (10, 4)] {
                at(5, yy, w, 1, SLATE);
            }
        }
        _ => {
            at(1, 2, 14, 12, BLACK);
            at(2, 3, 12, 2, COBALT);
            at(2, 6, 12, 7, WHITE);
        }
    }
}

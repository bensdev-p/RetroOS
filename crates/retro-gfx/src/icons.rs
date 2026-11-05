//! Icons drawn in code until the icon set (HANDOFF §7 step 7): the 32×32
//! pictures Station shows for minimised windows and Program Manager shows
//! for programs, in the colours of the current [`Theme`].

use crate::palette::{BLACK, COBALT, LIME, SILVER, SLATE, WHITE};
use crate::{Rgb, Surface, Theme};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconArt {
    /// A generic program window.
    Program,
    /// A DOS Prompt (the RetroDOS console).
    Dos,
    /// The computer itself (About This Computer).
    Computer,
}

/// The 32×32 picture of an icon at physical (x, y), in the theme's colours.
pub fn icon_picture(sf: &mut Surface, art: IconArt, x: i32, y: i32, p: i32, th: &Theme) {
    let mut at = |ax: i32, ay: i32, w: i32, h: i32, col: Rgb| {
        let col = th.picture(col);
        sf.fill_rect(x + ax * p, y + ay * p, (w * p) as u32, (h * p) as u32, col)
    };
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
        IconArt::Computer => {}
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

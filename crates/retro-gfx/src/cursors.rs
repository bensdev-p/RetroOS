//! The RetroOS pointers (docs/STYLE.md §5): 1-pixel Black outline, White
//! fill. Station draws them itself; the same art is written as the
//! "Phosphor" Xcursor theme (tools/retro-cursors) so that Firefox, GTK, Qt
//! and X11 programs show them too.

use crate::palette::{BLACK, WHITE};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cursor {
    Arrow,
    Text,
    Busy,
    Crosshair,
    Move,
    SizeWE,
    SizeNS,
    SizeNWSE,
    SizeNESW,
    Hand,
    NotAllowed,
}

impl Cursor {
    pub const ALL: [Cursor; 11] = [
        Cursor::Arrow,
        Cursor::Text,
        Cursor::Busy,
        Cursor::Crosshair,
        Cursor::Move,
        Cursor::SizeWE,
        Cursor::SizeNS,
        Cursor::SizeNWSE,
        Cursor::SizeNESW,
        Cursor::Hand,
        Cursor::NotAllowed,
    ];

    /// Cursor-theme names this pointer answers to (CSS names first, then
    /// the X11 and older toolkit names).
    pub fn names(self) -> &'static [&'static str] {
        match self {
            Cursor::Arrow => &[
                "default",
                "left_ptr",
                "arrow",
                "top_left_arrow",
                "context-menu",
                "help",
                "question_arrow",
                "copy",
                "alias",
                "dnd-copy",
                "dnd-move",
                "dnd-link",
            ],
            Cursor::Text => &["text", "xterm", "ibeam", "vertical-text"],
            Cursor::Busy => &["wait", "watch", "progress", "left_ptr_watch", "half-busy"],
            Cursor::Crosshair => &["crosshair", "cross", "tcross", "cell", "plus"],
            Cursor::Move => &[
                "move",
                "fleur",
                "all-scroll",
                "grabbing",
                "grab",
                "openhand",
                "closedhand",
                "size_all",
            ],
            Cursor::SizeWE => &[
                "ew-resize",
                "col-resize",
                "sb_h_double_arrow",
                "e-resize",
                "w-resize",
                "right_side",
                "left_side",
                "h_double_arrow",
                "size_hor",
                "split_h",
            ],
            Cursor::SizeNS => &[
                "ns-resize",
                "row-resize",
                "sb_v_double_arrow",
                "n-resize",
                "s-resize",
                "top_side",
                "bottom_side",
                "v_double_arrow",
                "size_ver",
                "split_v",
            ],
            Cursor::SizeNWSE => &[
                "nwse-resize",
                "nw-resize",
                "se-resize",
                "bd_double_arrow",
                "top_left_corner",
                "bottom_right_corner",
                "size_fdiag",
            ],
            Cursor::SizeNESW => &[
                "nesw-resize",
                "ne-resize",
                "sw-resize",
                "fd_double_arrow",
                "top_right_corner",
                "bottom_left_corner",
                "size_bdiag",
            ],
            Cursor::Hand => &["pointer", "hand1", "hand2", "pointing_hand", "hand"],
            Cursor::NotAllowed => &[
                "not-allowed",
                "no-drop",
                "forbidden",
                "crossed_circle",
                "circle",
                "dnd-no-drop",
            ],
        }
    }

    /// Art rows ('X' outline, 'o' fill, '.' clear) and the hotspot.
    pub fn art(self) -> (Vec<String>, (i32, i32)) {
        let rows = |r: &[&str]| r.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        match self {
            Cursor::Arrow => (rows(&ARROW), (0, 0)),
            Cursor::Hand => (rows(&HAND), (5, 0)),
            Cursor::Busy => (rows(&BUSY), (8, 10)),
            _ => self.drawn(),
        }
    }

    /// Shapes made of a filled area, outlined where it meets the outside.
    fn drawn(self) -> (Vec<String>, (i32, i32)) {
        let (w, h): (i32, i32) = match self {
            Cursor::SizeWE => (17, 9),
            Cursor::SizeNS => (9, 17),
            Cursor::Text => (7, 16),
            Cursor::Crosshair | Cursor::Move | Cursor::NotAllowed => (15, 15),
            _ => (13, 13),
        };
        let inside = |x: i32, y: i32| -> bool {
            match self {
                Cursor::SizeWE => {
                    let d = (y - 4).abs();
                    d <= x.min(16 - x) && (d <= 1 || x <= 4 || x >= 12)
                }
                Cursor::SizeNS => {
                    let d = (x - 4).abs();
                    d <= y.min(16 - y) && (d <= 1 || y <= 4 || y >= 12)
                }
                Cursor::SizeNWSE => x + y <= 5 || x + y >= 19 || (x - y).abs() <= 1,
                Cursor::SizeNESW => {
                    let x = 12 - x;
                    x + y <= 5 || x + y >= 19 || (x - y).abs() <= 1
                }
                // An I-beam: a stem with serifs top and bottom.
                Cursor::Text => x == 3 || ((y == 0 || y == 15) && x != 3),
                // A cross with a gap in the middle (the hot pixel shows).
                Cursor::Crosshair => {
                    (x == 7 || y == 7) && !((x - 7).abs() <= 1 && (y - 7).abs() <= 1)
                }
                // Four arrows on a cross.
                Cursor::Move => {
                    let (dx, dy) = ((x - 7).abs(), (y - 7).abs());
                    let (ty, tx) = (y.min(14 - y), x.min(14 - x));
                    x == 7 || y == 7 || (ty <= 3 && dx <= ty) || (tx <= 3 && dy <= tx)
                }
                // A ring with a bar from top left to bottom right.
                Cursor::NotAllowed => {
                    let (dx, dy) = (x as f32 - 7.0, y as f32 - 7.0);
                    let d = (dx * dx + dy * dy).sqrt();
                    (5.0..=7.2).contains(&d) || (d < 5.5 && (dx - dy).abs() <= 1.5)
                }
                _ => false,
            }
        };
        let ok = |x: i32, y: i32| x >= 0 && y >= 0 && x < w && y < h && inside(x, y);
        let mut out = Vec::new();
        for y in -1..=h {
            let mut r = String::new();
            for x in -1..=w {
                r.push(if ok(x, y) {
                    'o'
                } else if ok(x - 1, y) || ok(x + 1, y) || ok(x, y - 1) || ok(x, y + 1) {
                    'X'
                } else {
                    '.'
                });
            }
            out.push(r);
        }
        (out, (w / 2 + 1, h / 2 + 1))
    }

    /// ARGB pixels at a whole-pixel `scale`: (width, height, hotspot, pixels).
    pub fn image(self, scale: u32) -> (u32, u32, (u32, u32), Vec<u32>) {
        let (rows, (hx, hy)) = self.art();
        let s = scale.max(1);
        let w = rows.iter().map(|r| r.len()).max().unwrap_or(0) as u32;
        let h = rows.len() as u32;
        let mut px = vec![0u32; (w * s * h * s) as usize];
        for (y, row) in rows.iter().enumerate() {
            for (x, c) in row.chars().enumerate() {
                let v = match c {
                    'X' => 0xFF00_0000 | BLACK,
                    'o' => 0xFF00_0000 | WHITE,
                    _ => continue,
                };
                for dy in 0..s {
                    for dx in 0..s {
                        let (px_x, px_y) = (x as u32 * s + dx, y as u32 * s + dy);
                        px[(px_y * w * s + px_x) as usize] = v;
                    }
                }
            }
        }
        (w * s, h * s, (hx as u32 * s, hy as u32 * s), px)
    }
}

/// An Xcursor file holding the pointer at each scale (nominal size 16 per
/// scale step: 16, 32, 48), as libXcursor and every toolkit read it.
pub fn xcursor(c: Cursor, scales: &[u32]) -> Vec<u8> {
    const IMAGE: u32 = 0xfffd_0002;
    let mut out = Vec::new();
    let put = |out: &mut Vec<u8>, v: u32| out.extend_from_slice(&v.to_le_bytes());
    let n = scales.len() as u32;
    out.extend_from_slice(b"Xcur");
    put(&mut out, 16); // header size
    put(&mut out, 0x0001_0000); // version
    put(&mut out, n);
    let images: Vec<_> = scales.iter().map(|&s| (s, c.image(s))).collect();
    let mut pos = 16 + 12 * n;
    for (s, (w, h, _, _)) in &images {
        put(&mut out, IMAGE);
        put(&mut out, 16 * s);
        put(&mut out, pos);
        pos += 36 + 4 * w * h;
    }
    for (s, (w, h, (hx, hy), px)) in images {
        for v in [36, IMAGE, 16 * s, 1, w, h, hx, hy, 0] {
            put(&mut out, v);
        }
        for p in px {
            put(&mut out, p);
        }
    }
    out
}

/// The RetroOS arrow.
const ARROW: [&str; 19] = [
    "X...........",
    "XX..........",
    "XoX.........",
    "XooX........",
    "XoooX.......",
    "XooooX......",
    "XoooooX.....",
    "XooooooX....",
    "XoooooooX...",
    "XooooooooX..",
    "XoooooooooX.",
    "XooooooXXXXX",
    "XoooXooX....",
    "XooXXooX....",
    "XoX..XooX...",
    "XX...XooX...",
    "X.....XooX..",
    "......XooX..",
    ".......XX...",
];

/// A pointing hand (links).
const HAND: [&str; 17] = [
    "....XX..........",
    "...XooX.........",
    "...XooX.........",
    "...XooX.........",
    "...XooXXX.......",
    "...XooXooXXX....",
    "...XooXooXooXX..",
    "XXXXooXooXooXoX.",
    "XooXooooooooXoX.",
    "XooXoooooooooooX",
    ".XoXoooooooooooX",
    "..XooooooooooooX",
    "..XoooooooooooX.",
    "...XoooooooooX..",
    "....XooooooooX..",
    "....XooooooooX..",
    "....XXXXXXXXXX..",
];

/// Busy: the wristwatch-hourglass (an hourglass inside a watch).
const BUSY: [&str; 20] = [
    "....XXXXXXXX....",
    "....XooooooX....",
    "....XooooooX....",
    "..XXXXXXXXXXXX..",
    ".XXooooooooooXX.",
    "XXoXXXXXXXXXXoXX",
    "XooXooooooooXooX",
    "XoooXooooooXoooX",
    "XooooXooooXooooX",
    "XoooooXooXoooooX",
    "XoooooXXXXoooooX",
    "XooooXXXXXXooooX",
    "XoooXXXXXXXXoooX",
    "XooXXXXXXXXXXooX",
    "XXoXXXXXXXXXXoXX",
    ".XXooooooooooXX.",
    "..XXXXXXXXXXXX..",
    "....XooooooX....",
    "....XooooooX....",
    "....XXXXXXXX....",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pointer_is_outlined_and_rectangular() {
        for c in Cursor::ALL {
            let (rows, (hx, hy)) = c.art();
            let w = rows[0].len();
            assert!(rows.iter().all(|r| r.len() == w), "{c:?} rows differ");
            assert!(hx >= 0 && hy >= 0 && (hx as usize) < w && (hy as usize) < rows.len());
            let all: String = rows.concat();
            assert!(all.contains('X') && all.contains('o'), "{c:?}");
        }
    }

    #[test]
    fn an_xcursor_file_has_its_images_where_the_table_says() {
        let f = xcursor(Cursor::Text, &[1, 2]);
        let u = |i: usize| u32::from_le_bytes(f[i..i + 4].try_into().unwrap());
        assert_eq!(&f[0..4], b"Xcur");
        assert_eq!((u(4), u(12)), (16, 2));
        let (first, second) = (u(16 + 8) as usize, u(28 + 8) as usize);
        assert_eq!((u(first), u(first + 8)), (36, 16), "first image, size 16");
        assert_eq!(u(second + 8), 32);
        let (w, h) = (u(second + 16), u(second + 20));
        assert_eq!(f.len(), second + 36 + 4 * (w * h) as usize);
    }
}

#[cfg(test)]
mod show {
    #[test]
    #[ignore = "prints the pointers: cargo test -p retro-gfx show_cursors -- --ignored --nocapture"]
    fn show_cursors() {
        for c in super::Cursor::ALL {
            let (rows, hot) = c.art();
            println!("{c:?} {hot:?}");
            for r in rows {
                println!("  {}", r.replace('.', " "));
            }
        }
    }
}

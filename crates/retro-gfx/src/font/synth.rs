//! Rule-based glyph synthesis for Phosphor Mono.
//!
//! Box drawing and block elements must line up perfectly across cells, so they
//! are generated from geometry rather than drawn by hand. Accented Latin-1
//! letters are composed from the hand-drawn base letters plus diacritic marks.

use super::{Font, Glyph};

/// Fill in every synthesised glyph that the hand-drawn source didn't provide.
pub fn complete(f: &mut Font) {
    let (w, h) = (f.cell_w, f.cell_h);
    for cp in 0x2500u32..=0x259F {
        let ch = char::from_u32(cp).unwrap();
        if let Some(g) = box_or_block(cp, w, h) {
            f.insert_if_missing(ch, g);
        }
    }
    compose_latin1(f);
    derived(f);
}

// ---------------------------------------------------------------------------
// Box drawing

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum W {
    N, // none
    L, // light
    H, // heavy
    D, // double
}

/// (up, down, left, right) for U+2500..=U+257F line glyphs.
fn box_arms(cp: u32) -> Option<[W; 4]> {
    use W::*;
    // Light/heavy corners, tees and crosses U+250C..=U+254B, in Unicode order.
    const LH: &[(u32, [W; 4])] = &[
        (0x2500, [N, N, L, L]),
        (0x2501, [N, N, H, H]),
        (0x2502, [L, L, N, N]),
        (0x2503, [H, H, N, N]),
        (0x250C, [N, L, N, L]),
        (0x250D, [N, L, N, H]),
        (0x250E, [N, H, N, L]),
        (0x250F, [N, H, N, H]),
        (0x2510, [N, L, L, N]),
        (0x2511, [N, L, H, N]),
        (0x2512, [N, H, L, N]),
        (0x2513, [N, H, H, N]),
        (0x2514, [L, N, N, L]),
        (0x2515, [L, N, N, H]),
        (0x2516, [H, N, N, L]),
        (0x2517, [H, N, N, H]),
        (0x2518, [L, N, L, N]),
        (0x2519, [L, N, H, N]),
        (0x251A, [H, N, L, N]),
        (0x251B, [H, N, H, N]),
        (0x251C, [L, L, N, L]),
        (0x251D, [L, L, N, H]),
        (0x251E, [H, L, N, L]),
        (0x251F, [L, H, N, L]),
        (0x2520, [H, H, N, L]),
        (0x2521, [H, L, N, H]),
        (0x2522, [L, H, N, H]),
        (0x2523, [H, H, N, H]),
        (0x2524, [L, L, L, N]),
        (0x2525, [L, L, H, N]),
        (0x2526, [H, L, L, N]),
        (0x2527, [L, H, L, N]),
        (0x2528, [H, H, L, N]),
        (0x2529, [H, L, H, N]),
        (0x252A, [L, H, H, N]),
        (0x252B, [H, H, H, N]),
        (0x252C, [N, L, L, L]),
        (0x252D, [N, L, H, L]),
        (0x252E, [N, L, L, H]),
        (0x252F, [N, L, H, H]),
        (0x2530, [N, H, L, L]),
        (0x2531, [N, H, H, L]),
        (0x2532, [N, H, L, H]),
        (0x2533, [N, H, H, H]),
        (0x2534, [L, N, L, L]),
        (0x2535, [L, N, H, L]),
        (0x2536, [L, N, L, H]),
        (0x2537, [L, N, H, H]),
        (0x2538, [H, N, L, L]),
        (0x2539, [H, N, H, L]),
        (0x253A, [H, N, L, H]),
        (0x253B, [H, N, H, H]),
        (0x253C, [L, L, L, L]),
        (0x253D, [L, L, H, L]),
        (0x253E, [L, L, L, H]),
        (0x253F, [L, L, H, H]),
        (0x2540, [H, L, L, L]),
        (0x2541, [L, H, L, L]),
        (0x2542, [H, H, L, L]),
        (0x2543, [H, L, H, L]),
        (0x2544, [H, L, L, H]),
        (0x2545, [L, H, H, L]),
        (0x2546, [L, H, L, H]),
        (0x2547, [H, L, H, H]),
        (0x2548, [L, H, H, H]),
        (0x2549, [H, H, H, L]),
        (0x254A, [H, H, L, H]),
        (0x254B, [H, H, H, H]),
        // Double and mixed single/double U+2550..=U+256C.
        (0x2550, [N, N, D, D]),
        (0x2551, [D, D, N, N]),
        (0x2552, [N, L, N, D]),
        (0x2553, [N, D, N, L]),
        (0x2554, [N, D, N, D]),
        (0x2555, [N, L, D, N]),
        (0x2556, [N, D, L, N]),
        (0x2557, [N, D, D, N]),
        (0x2558, [L, N, N, D]),
        (0x2559, [D, N, N, L]),
        (0x255A, [D, N, N, D]),
        (0x255B, [L, N, D, N]),
        (0x255C, [D, N, L, N]),
        (0x255D, [D, N, D, N]),
        (0x255E, [L, L, N, D]),
        (0x255F, [D, D, N, L]),
        (0x2560, [D, D, N, D]),
        (0x2561, [L, L, D, N]),
        (0x2562, [D, D, L, N]),
        (0x2563, [D, D, D, N]),
        (0x2564, [N, L, D, D]),
        (0x2565, [N, D, L, L]),
        (0x2566, [N, D, D, D]),
        (0x2567, [L, N, D, D]),
        (0x2568, [D, N, L, L]),
        (0x2569, [D, N, D, D]),
        (0x256A, [L, L, D, D]),
        (0x256B, [D, D, L, L]),
        (0x256C, [D, D, D, D]),
        // Half lines and light/heavy joins U+2574..=U+257F.
        (0x2574, [N, N, L, N]),
        (0x2575, [L, N, N, N]),
        (0x2576, [N, N, N, L]),
        (0x2577, [N, L, N, N]),
        (0x2578, [N, N, H, N]),
        (0x2579, [H, N, N, N]),
        (0x257A, [N, N, N, H]),
        (0x257B, [N, H, N, N]),
        (0x257C, [N, N, L, H]),
        (0x257D, [L, H, N, N]),
        (0x257E, [N, N, H, L]),
        (0x257F, [H, L, N, N]),
    ];
    LH.iter().find(|(c, _)| *c == cp).map(|(_, a)| *a)
}

fn rect(g: &mut Glyph, x0: i32, y0: i32, x1: i32, y1: i32) {
    for y in y0.min(y1)..=y0.max(y1) {
        for x in x0.min(x1)..=x0.max(x1) {
            g.set(x, y);
        }
    }
}

fn draw_box(arms: [W; 4], w: u8, h: u8) -> Glyph {
    use W::*;
    let [u, d, l, r] = arms;
    let mut g = Glyph::blank(w, h);
    let (wi, hi) = (w as i32, h as i32);
    let cx = wi / 2 - 1; // 3
    let cy = hi / 2 - 1; // 7
    let (xl, xr, yt, yb) = (cx - 1, cx + 1, cy - 1, cy + 1);
    let vd = u == D || d == D;
    let hd = l == D || r == D;
    let thick = |a: W| if a == H { 1 } else { 0 };

    // Horizontal arms.
    match l {
        N => {}
        D => {
            let et = if u == D {
                xl
            } else if d == D {
                xr
            } else {
                cx
            };
            let eb = if d == D {
                xl
            } else if u == D {
                xr
            } else {
                cx
            };
            rect(&mut g, 0, yt, et, yt);
            rect(&mut g, 0, yb, eb, yb);
        }
        _ => {
            let e = if u == D && d == D {
                xl
            } else if vd {
                xr
            } else {
                cx + thick(u.max_w(d))
            };
            rect(&mut g, 0, cy, e, cy + thick(l));
        }
    }
    match r {
        N => {}
        D => {
            let st = if u == D {
                xr
            } else if d == D {
                xl
            } else {
                cx
            };
            let sb = if d == D {
                xr
            } else if u == D {
                xl
            } else {
                cx
            };
            rect(&mut g, st, yt, wi - 1, yt);
            rect(&mut g, sb, yb, wi - 1, yb);
        }
        _ => {
            let s = if u == D && d == D {
                xr
            } else if vd {
                xl
            } else {
                cx
            };
            rect(&mut g, s, cy, wi - 1, cy + thick(r));
        }
    }
    // Vertical arms.
    match u {
        N => {}
        D => {
            let el = if l == D {
                yt
            } else if r == D {
                yb
            } else {
                cy
            };
            let er = if r == D {
                yt
            } else if l == D {
                yb
            } else {
                cy
            };
            rect(&mut g, xl, 0, xl, el);
            rect(&mut g, xr, 0, xr, er);
        }
        _ => {
            let e = if l == D && r == D {
                yt
            } else if hd {
                yb
            } else {
                cy + thick(l.max_w(r))
            };
            rect(&mut g, cx, 0, cx + thick(u), e);
        }
    }
    match d {
        N => {}
        D => {
            let sl = if l == D {
                yb
            } else if r == D {
                yt
            } else {
                cy
            };
            let sr = if r == D {
                yb
            } else if l == D {
                yt
            } else {
                cy
            };
            rect(&mut g, xl, sl, xl, hi - 1);
            rect(&mut g, xr, sr, xr, hi - 1);
        }
        _ => {
            let s = if l == D && r == D {
                yb
            } else if hd {
                yt
            } else {
                cy
            };
            rect(&mut g, cx, s, cx + thick(d), hi - 1);
        }
    }
    g
}

impl W {
    /// The heavier of two non-double weights (used to size joints).
    fn max_w(self, o: W) -> W {
        if self == W::H || o == W::H {
            W::H
        } else {
            W::L
        }
    }
}

fn dashes(horizontal: bool, heavy: bool, n: i32, w: u8, h: u8) -> Glyph {
    let mut g = Glyph::blank(w, h);
    let (wi, hi) = (w as i32, h as i32);
    let (cx, cy) = (wi / 2 - 1, hi / 2 - 1);
    let t = heavy as i32;
    let len = if horizontal { wi } else { hi };
    for i in 0..n {
        let a = i * len / n;
        let b = (i + 1) * len / n;
        let gap = ((b - a) / 3).max(1);
        let e = b - gap - 1;
        if horizontal {
            rect(&mut g, a, cy, e, cy + t);
        } else {
            rect(&mut g, cx, a + gap / 2, cx + t, e + gap / 2);
        }
    }
    g
}

fn box_or_block(cp: u32, w: u8, h: u8) -> Option<Glyph> {
    let (wi, hi) = (w as i32, h as i32);
    let (cx, cy) = (wi / 2 - 1, hi / 2 - 1);
    if let Some(arms) = box_arms(cp) {
        return Some(draw_box(arms, w, h));
    }
    let mut g = Glyph::blank(w, h);
    match cp {
        0x2504 => return Some(dashes(true, false, 3, w, h)),
        0x2505 => return Some(dashes(true, true, 3, w, h)),
        0x2506 => return Some(dashes(false, false, 3, w, h)),
        0x2507 => return Some(dashes(false, true, 3, w, h)),
        0x2508 => return Some(dashes(true, false, 4, w, h)),
        0x2509 => return Some(dashes(true, true, 4, w, h)),
        0x250A => return Some(dashes(false, false, 4, w, h)),
        0x250B => return Some(dashes(false, true, 4, w, h)),
        0x254C => return Some(dashes(true, false, 2, w, h)),
        0x254D => return Some(dashes(true, true, 2, w, h)),
        0x254E => return Some(dashes(false, false, 2, w, h)),
        0x254F => return Some(dashes(false, true, 2, w, h)),
        // Rounded corners.
        0x256D => {
            rect(&mut g, cx, cy + 2, cx, hi - 1);
            rect(&mut g, cx + 2, cy, wi - 1, cy);
            g.set(cx + 1, cy + 1);
        }
        0x256E => {
            rect(&mut g, cx, cy + 2, cx, hi - 1);
            rect(&mut g, 0, cy, cx - 2, cy);
            g.set(cx - 1, cy + 1);
        }
        0x256F => {
            rect(&mut g, cx, 0, cx, cy - 2);
            rect(&mut g, 0, cy, cx - 2, cy);
            g.set(cx - 1, cy - 1);
        }
        0x2570 => {
            rect(&mut g, cx, 0, cx, cy - 2);
            rect(&mut g, cx + 2, cy, wi - 1, cy);
            g.set(cx + 1, cy - 1);
        }
        // Diagonals.
        0x2571..=0x2573 => {
            for y in 0..hi {
                let x = y * wi / hi;
                if cp != 0x2572 {
                    g.set(wi - 1 - x, y);
                }
                if cp != 0x2571 {
                    g.set(x, y);
                }
            }
        }
        // Block elements.
        0x2580 => rect(&mut g, 0, 0, wi - 1, hi / 2 - 1),
        0x2581..=0x2588 => {
            let k = (cp - 0x2580) as i32; // eighths from the bottom
            rect(&mut g, 0, hi - hi * k / 8, wi - 1, hi - 1);
        }
        0x2589..=0x258F => {
            let k = (0x2590 - cp) as i32; // eighths from the left (7..1)
            rect(&mut g, 0, 0, wi * k / 8 - 1, hi - 1);
        }
        0x2590 => rect(&mut g, wi / 2, 0, wi - 1, hi - 1),
        0x2591..=0x2593 => {
            for y in 0..hi {
                for x in 0..wi {
                    let quarter = (x + 2 * (y % 2)) % 4 == 0;
                    let on = match cp {
                        0x2591 => quarter,
                        0x2592 => (x + y) % 2 == 0,
                        _ => !quarter,
                    };
                    if on {
                        g.set(x, y);
                    }
                }
            }
        }
        0x2594 => rect(&mut g, 0, 0, wi - 1, hi / 8 - 1),
        0x2595 => rect(&mut g, wi - wi / 8, 0, wi - 1, hi - 1),
        0x2596..=0x259F => {
            // Quadrants: bit 0 = upper-left, 1 = upper-right, 2 = lower-left, 3 = lower-right.
            let q = match cp {
                0x2596 => 0b0100,
                0x2597 => 0b1000,
                0x2598 => 0b0001,
                0x2599 => 0b1101,
                0x259A => 0b1001,
                0x259B => 0b0111,
                0x259C => 0b1011,
                0x259D => 0b0010,
                0x259E => 0b0110,
                _ => 0b1110,
            };
            let (hw, hh) = (wi / 2, hi / 2);
            for (bit, (x, y)) in [(0, 0), (hw, 0), (0, hh), (hw, hh)].into_iter().enumerate() {
                if q & (1 << bit) != 0 {
                    rect(&mut g, x, y, x + hw - 1, y + hh - 1);
                }
            }
        }
        _ => return None,
    }
    Some(g)
}

// ---------------------------------------------------------------------------
// Latin-1 composition

#[derive(Clone, Copy)]
enum Mark {
    Grave,
    Acute,
    Circumflex,
    Tilde,
    Diaeresis,
    Ring,
    Cedilla,
}

fn mark_rows(m: Mark, upper: bool) -> &'static [&'static str] {
    use Mark::*;
    match (m, upper) {
        (Grave, _) => &["..##....", "...##..."],
        (Acute, _) => &["....##..", "...##..."],
        (Circumflex, _) => &["..###...", ".##.##.."],
        (Tilde, _) => &[".###.##.", "##.###.."],
        (Diaeresis, _) => &[".##..##.", ".##..##."],
        // Upper-case ring sits on the apex of A and closes against it.
        (Ring, true) => &["..###...", "..#.#..."],
        (Ring, false) => &["..###...", ".##.##..", "..###..."],
        (Cedilla, _) => &["...##...", "..##...."],
    }
}

fn stamp(g: &mut Glyph, rows: &[&str], top: i32) {
    for (dy, row) in rows.iter().enumerate() {
        for (x, c) in row.chars().enumerate() {
            if c == '#' {
                g.set(x as i32, top + dy as i32);
            }
        }
    }
}

/// Make room above a capital: drop one duplicated row and push the top down.
fn squash_capital(mut g: Glyph) -> Glyph {
    for r in 3..=10usize {
        if g.rows[r] == g.rows[r + 1] && g.rows[r] != 0 {
            for i in (3..=r).rev() {
                g.rows[i] = g.rows[i - 1];
            }
            g.rows[2] = 0;
            break;
        }
    }
    g
}

fn compose_latin1(f: &mut Font) {
    use Mark::*;
    let table: &[(char, char, Mark)] = &[
        ('À', 'A', Grave),
        ('Á', 'A', Acute),
        ('Â', 'A', Circumflex),
        ('Ã', 'A', Tilde),
        ('Ä', 'A', Diaeresis),
        ('Å', 'A', Ring),
        ('Ç', 'C', Cedilla),
        ('È', 'E', Grave),
        ('É', 'E', Acute),
        ('Ê', 'E', Circumflex),
        ('Ë', 'E', Diaeresis),
        ('Ì', 'I', Grave),
        ('Í', 'I', Acute),
        ('Î', 'I', Circumflex),
        ('Ï', 'I', Diaeresis),
        ('Ñ', 'N', Tilde),
        ('Ò', 'O', Grave),
        ('Ó', 'O', Acute),
        ('Ô', 'O', Circumflex),
        ('Õ', 'O', Tilde),
        ('Ö', 'O', Diaeresis),
        ('Ù', 'U', Grave),
        ('Ú', 'U', Acute),
        ('Û', 'U', Circumflex),
        ('Ü', 'U', Diaeresis),
        ('Ý', 'Y', Acute),
        ('à', 'a', Grave),
        ('á', 'a', Acute),
        ('â', 'a', Circumflex),
        ('ã', 'a', Tilde),
        ('ä', 'a', Diaeresis),
        ('å', 'a', Ring),
        ('ç', 'c', Cedilla),
        ('è', 'e', Grave),
        ('é', 'e', Acute),
        ('ê', 'e', Circumflex),
        ('ë', 'e', Diaeresis),
        ('ì', 'ı', Grave),
        ('í', 'ı', Acute),
        ('î', 'ı', Circumflex),
        ('ï', 'ı', Diaeresis),
        ('ñ', 'n', Tilde),
        ('ò', 'o', Grave),
        ('ó', 'o', Acute),
        ('ô', 'o', Circumflex),
        ('õ', 'o', Tilde),
        ('ö', 'o', Diaeresis),
        ('ù', 'u', Grave),
        ('ú', 'u', Acute),
        ('û', 'u', Circumflex),
        ('ü', 'u', Diaeresis),
        ('ý', 'y', Acute),
        ('ÿ', 'y', Diaeresis),
    ];
    // Dotless i first: it is the base for accented lower-case i.
    if let Some(&i) = f.own_glyph('i') {
        let mut g = i;
        for r in 0..5 {
            g.rows[r] = 0;
        }
        f.insert_if_missing('ı', g);
    }
    for &(ch, base, mark) in table {
        let Some(&b) = f.own_glyph(base) else {
            continue;
        };
        let upper = base.is_uppercase();
        let mut g = match mark {
            Mark::Cedilla => b,
            Mark::Ring if upper => b,
            _ if upper => squash_capital(b),
            _ => b,
        };
        let top = match (mark, upper) {
            (Mark::Cedilla, _) => 12,
            (Mark::Ring, false) => 1,
            (_, true) => 0,
            (_, false) => 2,
        };
        stamp(&mut g, mark_rows(mark, upper), top);
        f.insert_if_missing(ch, g);
    }
}

// ---------------------------------------------------------------------------
// Small derived glyphs

fn mirror(g: &Glyph, ink_w: i32) -> Glyph {
    let mut m = Glyph::blank(g.width, g.height);
    for y in 0..g.height as i32 {
        for x in 0..ink_w {
            if g.get(x, y) {
                m.set(ink_w - 1 - x, y);
            }
        }
    }
    m
}

fn derived(f: &mut Font) {
    if let Some(&dot) = f.own_glyph('·') {
        f.insert_if_missing('∙', dot);
    }
    if let Some(&not) = f.own_glyph('¬') {
        f.insert_if_missing('⌐', mirror(&not, 7));
    }
    if let Some(&c) = f.own_glyph('○') {
        let mut g = Glyph::blank(c.width, c.height);
        for y in 0..c.height as usize {
            g.rows[y] = !c.rows[y] & (0xFFFFu16 << (16 - c.width));
        }
        f.insert_if_missing('◙', g);
    }
    let mut n = Glyph::blank(f.cell_w, f.cell_h);
    stamp(&mut n, &["####....", "##.##...", "##.##...", "##.##..."], 1);
    f.insert_if_missing('ⁿ', n);
    let mut a = Glyph::blank(f.cell_w, f.cell_h);
    stamp(
        &mut a,
        &[
            ".###....", "...##...", ".####...", "##.##...", ".####...", "........", "#####...",
        ],
        2,
    );
    f.insert_if_missing('ª', a);
    let mut o = Glyph::blank(f.cell_w, f.cell_h);
    stamp(
        &mut o,
        &[
            ".###....", "##.##...", "##.##...", "##.##...", ".###....", "........", "#####...",
        ],
        2,
    );
    f.insert_if_missing('º', o);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::phosphor_mono;

    fn art(ch: char) -> String {
        phosphor_mono().own_glyph(ch).unwrap().to_art()
    }

    #[test]
    fn double_corner_shape() {
        let a = art('╔');
        let rows: Vec<&str> = a.lines().collect();
        assert_eq!(rows[6], "..######");
        assert_eq!(rows[7], "..#.....");
        assert_eq!(rows[8], "..#.####");
        assert_eq!(rows[15], "..#.#...");
        assert_eq!(rows[0], "........");
    }

    #[test]
    fn lines_tile_across_cells() {
        let f = phosphor_mono();
        // Every horizontal line glyph touches both cell edges on its line rows.
        for ch in ['─', '═', '┼', '╬', '╦', '╤'] {
            let g = f.own_glyph(ch).unwrap();
            let ok = (0..16).any(|y| g.get(0, y) && g.get(7, y));
            assert!(ok, "{ch} does not span the cell");
        }
        // Vertical lines reach top and bottom.
        for ch in ['│', '║', '┼', '╬', '╠'] {
            let g = f.own_glyph(ch).unwrap();
            assert!((0..8).any(|x| g.get(x, 0) && g.get(x, 15)), "{ch}");
        }
    }

    #[test]
    fn shades_have_expected_density() {
        let f = phosphor_mono();
        let count = |ch| {
            let g = f.own_glyph(ch).unwrap();
            (0..16).map(|y| g.rows[y].count_ones()).sum::<u32>()
        };
        assert_eq!(count('░'), 32);
        assert_eq!(count('▒'), 64);
        assert_eq!(count('▓'), 96);
        assert_eq!(count('█'), 128);
    }

    #[test]
    fn accented_capitals_have_room() {
        let g = phosphor_mono().own_glyph('É').unwrap();
        assert_eq!(g.rows[2], 0, "gap row between accent and capital");
        assert_ne!(g.rows[0] | g.rows[1], 0);
    }
}

//! Accented Latin-1 letters for the proportional Lantern fonts, composed
//! from the drawn base letter and a drawn mark (the monospace font has its
//! own fixed-cell composition in [`super::synth`]).
//!
//! Marks are centred over the base letter's ink (a glyph's last column is
//! its spacing). Lower-case marks sit above the x-height with one blank row;
//! a capital loses `cap_squash` of its repeated rows (stems, straight sides)
//! so that its mark fits in the rows above it.

use super::{Font, Glyph};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    Grave,
    Acute,
    Circumflex,
    Tilde,
    Diaeresis,
    Ring,
    Cedilla,
}

/// How one font places its marks.
pub struct Style {
    /// First row of the x-height (lower-case letters start here).
    pub x_top: usize,
    /// First row of the capitals.
    pub cap_top: usize,
    /// Rows a capital gives up to make room for its mark.
    pub cap_squash: usize,
    /// Row of the baseline's last ink row (cedillas go below it).
    pub base_row: usize,
    /// The art of each mark, as '#'/'.' rows, for lower case (false) or a
    /// capital (true). Where the rows above a letter are too few for the mark
    /// and a blank row, the mark touches the letter.
    pub art: fn(Mark, bool) -> &'static [&'static str],
}

const TABLE: &[(char, char, Mark)] = {
    use Mark::*;
    &[
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
    ]
};

/// Characters that look like another one in these fonts.
const ALIASES: &[(char, char)] = &[('\u{AD}', '-'), ('\u{2010}', '-'), ('\u{2011}', '-')];

/// Add the accented letters (and aliases) the font does not draw itself.
pub fn complete(f: &mut Font, s: &Style) {
    // Dotless i: the base for accented lower-case i.
    if let Some(&i) = f.own_glyph('i') {
        let mut g = i;
        for r in g.rows.iter_mut().take(s.x_top) {
            *r = 0;
        }
        f.insert_if_missing('ı', g);
    }
    for &(ch, base, mark) in TABLE {
        let Some(&b) = f.own_glyph(base) else {
            continue;
        };
        let upper = base.is_uppercase();
        let below = mark == Mark::Cedilla;
        let base = if upper && !below {
            squash(b, s.cap_top, s.base_row, s.cap_squash)
        } else {
            b
        };
        let art = (s.art)(mark, upper);
        let letter_top = if upper {
            s.cap_top + s.cap_squash
        } else {
            s.x_top
        };
        // One blank row between mark and letter, except that a ring sits on
        // the apex of A.
        let gap = usize::from(!(mark == Mark::Ring && upper));
        let top = if below {
            s.base_row + 1
        } else {
            letter_top.saturating_sub(art.len() + gap)
        };
        f.insert_if_missing(ch, place(&base, art, top, below));
    }
    for &(ch, like) in ALIASES {
        if let Some(&g) = f.own_glyph(like) {
            f.insert_if_missing(ch, g);
        }
    }
}

/// Remove `n` rows between `first` and `last` (inclusive), preferring a row
/// that repeats the one below it, nearest the middle; the top moves down.
fn squash(mut g: Glyph, first: usize, last: usize, n: usize) -> Glyph {
    for _ in 0..n {
        let mid = (first + last) / 2;
        let mut rows: Vec<usize> = (first..last).collect();
        rows.sort_by_key(|&r| r.abs_diff(mid));
        let r = rows
            .iter()
            .copied()
            .find(|&r| g.rows[r] != 0 && g.rows[r] == g.rows[r + 1])
            .unwrap_or(mid);
        for i in (first + 1..=r).rev() {
            g.rows[i] = g.rows[i - 1];
        }
        g.rows[first] = 0;
    }
    g
}

/// The base glyph with `art` stamped from row `top`, centred over its ink;
/// the glyph widens when the mark is wider than the letter (i).
fn place(base: &Glyph, art: &[&str], top: usize, below: bool) -> Glyph {
    let ink = base.width.saturating_sub(1) as usize;
    let mw = art.iter().map(|r| r.len()).max().unwrap_or(0);
    let new_ink = ink.max(mw);
    let base_off = (new_ink - ink) / 2;
    // A cedilla hangs from the left half of C/c, as in print.
    let mark_off = if below {
        (new_ink.saturating_sub(mw)) / 2
    } else {
        (new_ink - mw).div_ceil(2)
    };
    let mut g = Glyph::blank((new_ink + 1) as u8, base.height);
    for (y, row) in base.rows.iter().enumerate() {
        g.rows[y] = row >> base_off;
    }
    for (dy, row) in art.iter().enumerate() {
        for (x, c) in row.chars().enumerate() {
            if c == '#' {
                g.set((mark_off + x) as i32, (top + dy) as i32);
            }
        }
    }
    g
}

#[cfg(test)]
mod tests {
    use crate::font::lantern_12;

    #[test]
    fn every_latin1_letter_is_composed() {
        let f = lantern_12();
        for &(ch, base, _) in super::TABLE {
            assert!(f.has_own(ch), "{ch} (from {base}) missing");
        }
        assert!(f.has_own('\u{AD}'));
    }

    #[test]
    fn marks_do_not_touch_the_letter() {
        let f = lantern_12();
        // é: mark rows above the x-height, then one blank row.
        let g = f.own_glyph('é').unwrap();
        assert_eq!(g.rows[3], 0, "{}", g.to_art());
        assert_ne!(g.rows[1] | g.rows[2], 0, "{}", g.to_art());
        // É: the capital keeps a blank row under its mark.
        let g = f.own_glyph('É').unwrap();
        assert_eq!(g.rows[2], 0, "{}", g.to_art());
        assert_ne!(g.rows[0] | g.rows[1], 0, "{}", g.to_art());
        assert_eq!(
            g.rows[9],
            f.own_glyph('E').unwrap().rows[9],
            "same baseline"
        );
    }

    #[test]
    fn accented_i_is_wide_enough_for_its_mark() {
        let f = lantern_12();
        let g = f.own_glyph('î').unwrap();
        assert!(g.width >= 5, "{}", g.to_art());
        assert_eq!(g.rows[3], 0, "blank row under the mark: {}", g.to_art());
        let dotless = f.own_glyph('ı').unwrap();
        assert_eq!(dotless.rows[2], 0, "the dot of i is gone");
    }

    #[test]
    fn lantern_9_has_its_accents() {
        let f = crate::font::lantern_9();
        for &(ch, base, _) in super::TABLE {
            assert!(f.has_own(ch), "{ch} (from {base}) missing in Lantern 9");
        }
        let e = f.own_glyph('é').unwrap();
        assert_eq!(e.rows[2], 0, "{}", e.to_art());
        assert_eq!(e.rows[6], f.own_glyph('e').unwrap().rows[6]);
    }

    #[test]
    #[ignore = "prints glyphs for a look: cargo test -p retro-gfx show_accents -- --ignored --nocapture"]
    fn show_accents() {
        for f in [lantern_12(), crate::font::lantern_9()] {
            for line in [
                "Café Ñoño Ärger Øre",
                "“Größe” – © ® 50 € ± ¿½?",
                "Ça va? àéîõü ÆæßÐ",
            ] {
                let glyphs: Vec<_> = line.chars().map(|c| f.glyph_or_replacement(c)).collect();
                for y in 0..f.cell_h as i32 {
                    let row: String = glyphs
                        .iter()
                        .flat_map(|g| {
                            (0..g.width as i32).map(move |x| if g.get(x, y) { '#' } else { ' ' })
                        })
                        .collect();
                    println!("{row}");
                }
            }
        }
    }
}

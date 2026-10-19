//! Bitmap fonts.
//!
//! The primary font, Phosphor Mono 8x16, is drawn by hand in
//! `assets/fonts/phosphor-mono-8x16.glyphs` and completed by rule-based
//! synthesis ([`synth`]): box drawing, block elements and accented letters.
//! Glyphs we have not drawn can be filled in from GNU Unifont's `.hex` format.

pub mod synth;

use std::collections::HashMap;
use std::sync::OnceLock;

/// One glyph. Row `y` bit `0x8000 >> x` is pixel (x, y).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glyph {
    pub width: u8,
    pub height: u8,
    pub rows: [u16; 16],
}

impl Glyph {
    pub const fn blank(width: u8, height: u8) -> Glyph {
        Glyph {
            width,
            height,
            rows: [0; 16],
        }
    }

    #[inline]
    pub fn get(&self, x: i32, y: i32) -> bool {
        x >= 0
            && y >= 0
            && x < self.width as i32
            && y < self.height as i32
            && self.rows[y as usize] & (0x8000 >> x) != 0
    }

    #[inline]
    pub fn set(&mut self, x: i32, y: i32) {
        if x >= 0 && y >= 0 && x < self.width as i32 && y < self.height as i32 {
            self.rows[y as usize] |= 0x8000 >> x;
        }
    }

    /// Is this a double-width (two cell) glyph?
    pub fn is_wide(&self, cell_w: u8) -> bool {
        self.width > cell_w
    }

    /// Render as '#'/'.' art (for tests and debugging).
    pub fn to_art(&self) -> String {
        let mut s = String::new();
        for y in 0..self.height as i32 {
            for x in 0..self.width as i32 {
                s.push(if self.get(x, y) { '#' } else { '.' });
            }
            s.push('\n');
        }
        s
    }
}

#[derive(Clone, Debug)]
pub struct Font {
    pub name: String,
    pub cell_w: u8,
    pub cell_h: u8,
    glyphs: HashMap<char, Glyph>,
    fallback: HashMap<char, Glyph>,
    replacement: Glyph,
}

impl Font {
    pub fn new(name: &str, cell_w: u8, cell_h: u8) -> Font {
        // Replacement glyph: a hollow box, like an unprintable character on a 1990 terminal.
        let mut replacement = Glyph::blank(cell_w, cell_h);
        let (x0, x1, y0, y1) = (1, cell_w as i32 - 3, 3, cell_h as i32 - 5);
        for x in x0..=x1 {
            replacement.set(x, y0);
            replacement.set(x, y1);
        }
        for y in y0..=y1 {
            replacement.set(x0, y);
            replacement.set(x1, y);
        }
        Font {
            name: name.to_string(),
            cell_w,
            cell_h,
            glyphs: HashMap::new(),
            fallback: HashMap::new(),
            replacement,
        }
    }

    /// Parse the `.glyphs` source format (see the header of the font file).
    pub fn parse_glyphs(name: &str, cell_w: u8, cell_h: u8, src: &str) -> Result<Font, String> {
        Font::parse(name, cell_w, cell_h, src, false)
    }

    /// The same format for a proportional font: each glyph is as wide as its
    /// rows (all rows of a glyph the same length, at most 16), or `w<N>` in
    /// its header line for glyphs without rows (space). `cell_w` becomes the
    /// width of the replacement glyph.
    pub fn parse_proportional(name: &str, cell_h: u8, src: &str) -> Result<Font, String> {
        Font::parse(name, 8, cell_h, src, true)
    }

    fn parse(
        name: &str,
        cell_w: u8,
        cell_h: u8,
        src: &str,
        proportional: bool,
    ) -> Result<Font, String> {
        let mut font = Font::new(name, cell_w, cell_h);
        let mut cur: Option<(char, Glyph, usize, usize)> = None; // (ch, glyph, next row, line no.)
        let finish = |font: &mut Font, cur: &mut Option<(char, Glyph, usize, usize)>| {
            if let Some((ch, g, _, line)) = cur.take() {
                if font.glyphs.insert(ch, g).is_some() {
                    return Err(format!("line {line}: duplicate glyph U+{:04X}", ch as u32));
                }
            }
            Ok(())
        };
        for (i, raw) in src.lines().enumerate() {
            let line_no = i + 1;
            let line = raw.trim_end();
            // Comments are "#" alone or "# text"; anything else made of '#'/'.' is pixel art.
            let comment = line == "#" || line.starts_with("# ");
            let art_only =
                !comment && !line.is_empty() && line.chars().all(|c| c == '#' || c == '.');
            let len = line.chars().count();
            let is_row = art_only
                && if proportional {
                    len <= 16
                } else {
                    len == cell_w as usize
                };
            if comment || (line.starts_with('#') && !art_only) {
                continue; // comment
            }
            if let Some(rest) = line.strip_prefix('@') {
                finish(&mut font, &mut cur)?;
                let mut parts = rest.split_whitespace();
                let cp = parts
                    .next()
                    .ok_or(format!("line {line_no}: missing codepoint"))?;
                let cp = u32::from_str_radix(cp, 16)
                    .map_err(|e| format!("line {line_no}: bad codepoint {cp}: {e}"))?;
                let ch = char::from_u32(cp).ok_or(format!("line {line_no}: invalid char"))?;
                let mut top = 0usize;
                let mut width = if proportional { 0 } else { cell_w };
                for p in parts {
                    if let Some(n) = p.strip_prefix('+') {
                        top = n
                            .parse()
                            .map_err(|_| format!("line {line_no}: bad offset {p}"))?;
                    }
                    let digits = |n: &&str| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit());
                    if let Some(n) = p.strip_prefix('w').filter(digits).filter(|_| proportional) {
                        width = n
                            .parse()
                            .ok()
                            .filter(|w| *w <= 16)
                            .ok_or(format!("line {line_no}: bad width {p}"))?;
                    }
                }
                cur = Some((ch, Glyph::blank(width, cell_h), top, line_no));
                continue;
            }
            if line.is_empty() {
                continue;
            }
            let Some((_, g, row, _)) = cur.as_mut() else {
                return Err(format!("line {line_no}: pixel row outside a glyph"));
            };
            if !is_row {
                return Err(format!(
                    "line {line_no}: row must be exactly {cell_w} of '#' or '.': {line:?}"
                ));
            }
            if proportional {
                if g.width == 0 {
                    g.width = len as u8;
                } else if g.width as usize != len {
                    return Err(format!(
                        "line {line_no}: rows of a glyph must all be {} wide: {line:?}",
                        g.width
                    ));
                }
            }
            if *row >= cell_h as usize {
                return Err(format!("line {line_no}: too many rows"));
            }
            for (x, c) in line.chars().enumerate() {
                if c == '#' {
                    g.set(x as i32, *row as i32);
                }
            }
            *row += 1;
        }
        finish(&mut font, &mut cur)?;
        Ok(font)
    }

    pub fn insert(&mut self, ch: char, g: Glyph) {
        self.glyphs.insert(ch, g);
    }

    /// Insert only if no glyph exists yet (hand-drawn glyphs win over synthesis).
    pub fn insert_if_missing(&mut self, ch: char, g: Glyph) {
        self.glyphs.entry(ch).or_insert(g);
    }

    pub fn has_own(&self, ch: char) -> bool {
        self.glyphs.contains_key(&ch)
    }

    pub fn own_glyph(&self, ch: char) -> Option<&Glyph> {
        self.glyphs.get(&ch)
    }

    /// Own glyph, else fallback glyph.
    pub fn glyph(&self, ch: char) -> Option<&Glyph> {
        self.glyphs.get(&ch).or_else(|| self.fallback.get(&ch))
    }

    pub fn glyph_or_replacement(&self, ch: char) -> &Glyph {
        self.glyph(ch).unwrap_or(&self.replacement)
    }

    pub fn len(&self) -> usize {
        self.glyphs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    /// Sorted list of characters with own (non-fallback) glyphs.
    pub fn chars(&self) -> Vec<char> {
        let mut v: Vec<char> = self.glyphs.keys().copied().collect();
        v.sort();
        v
    }

    /// Load GNU Unifont `.hex` data as fallback glyphs ("XXXX:<32 or 64 hex digits>").
    /// Only 16-row glyphs are accepted, and only for fonts with 16-row cells.
    pub fn load_hex_fallback(&mut self, hex: &str) -> usize {
        if self.cell_h != 16 {
            return 0;
        }
        let mut n = 0;
        for line in hex.lines() {
            let Some((cp, data)) = line.split_once(':') else {
                continue;
            };
            let Ok(cp) = u32::from_str_radix(cp.trim(), 16) else {
                continue;
            };
            let Some(ch) = char::from_u32(cp) else {
                continue;
            };
            let data = data.trim();
            let width = match data.len() {
                32 => 8u8,
                64 => 16u8,
                _ => continue,
            };
            let digits = data.len() / 16;
            let mut g = Glyph::blank(width, 16);
            let mut ok = true;
            for y in 0..16 {
                match u16::from_str_radix(&data[y * digits..(y + 1) * digits], 16) {
                    Ok(v) => g.rows[y] = if width == 8 { v << 8 } else { v },
                    Err(_) => ok = false,
                }
            }
            if ok {
                self.fallback.insert(ch, g);
                n += 1;
            }
        }
        n
    }

    /// Try the usual system locations for Unifont and load it as fallback.
    pub fn load_system_fallback(&mut self) -> usize {
        for p in [
            "/usr/share/unifont/unifont.hex",
            "/usr/share/retroos/fonts/unifont.hex",
        ] {
            if let Ok(s) = std::fs::read_to_string(p) {
                return self.load_hex_fallback(&s);
            }
        }
        0
    }

    /// Encode as a PSF2 console font (for the Linux VT in Safe Mode).
    /// Takes at most 512 glyphs; includes a Unicode mapping table.
    pub fn to_psf2(&self, chars: &[char]) -> Vec<u8> {
        let chars: Vec<char> = chars.iter().copied().take(512).collect();
        let bytes_per_row = (self.cell_w as usize).div_ceil(8);
        let charsize = bytes_per_row * self.cell_h as usize;
        let mut out = Vec::new();
        let hdr: [u32; 8] = [
            0x864A_B572,        // magic
            0,                  // version
            32,                 // header size
            1,                  // flags: has unicode table
            chars.len() as u32, // length
            charsize as u32,    // bytes per glyph
            self.cell_h as u32, // height
            self.cell_w as u32, // width
        ];
        for v in hdr {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for &ch in &chars {
            let g = self.glyph_or_replacement(ch);
            for y in 0..self.cell_h as usize {
                let row = g.rows[y];
                for b in 0..bytes_per_row {
                    out.push((row >> (8 * (1 - b.min(1)))) as u8);
                }
            }
        }
        for &ch in &chars {
            let mut buf = [0u8; 4];
            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            out.push(0xFF);
        }
        out
    }
}

const PHOSPHOR_MONO_SRC: &str = include_str!("../../../../assets/fonts/phosphor-mono-8x16.glyphs");

/// Build Phosphor Mono 8x16 (hand-drawn glyphs + synthesis), without fallback.
pub fn build_phosphor_mono() -> Result<Font, String> {
    let mut f = Font::parse_glyphs("Phosphor Mono 8x16", 8, 16, PHOSPHOR_MONO_SRC)?;
    synth::complete(&mut f);
    Ok(f)
}

/// The shared Phosphor Mono 8x16 instance (no Unifont fallback loaded).
pub fn phosphor_mono() -> &'static Font {
    static FONT: OnceLock<Font> = OnceLock::new();
    FONT.get_or_init(|| build_phosphor_mono().expect("Phosphor Mono source is valid"))
}

const LANTERN_12_SRC: &str = include_str!("../../../../assets/fonts/lantern-12.glyphs");

/// Lantern 12, the proportional system font of the Desktop (menus, title
/// bars, buttons, dialogs). 13 rows tall.
pub fn lantern_12() -> &'static Font {
    static FONT: OnceLock<Font> = OnceLock::new();
    FONT.get_or_init(|| {
        Font::parse_proportional("Lantern 12", 13, LANTERN_12_SRC)
            .expect("Lantern 12 source is valid")
    })
}

impl Font {
    /// Width in pixels of `s` set in this font (at scale 1).
    pub fn text_width(&self, s: &str) -> u32 {
        s.chars()
            .map(|c| self.glyph_or_replacement(c).width as u32)
            .sum()
    }
}

/// CP437 code page → Unicode, for the DOS character set and PSF export ordering.
pub const CP437: [char; 256] = {
    const HI: [char; 128] = [
        'Ç', 'ü', 'é', 'â', 'ä', 'à', 'å', 'ç', 'ê', 'ë', 'è', 'ï', 'î', 'ì', 'Ä', 'Å', 'É', 'æ',
        'Æ', 'ô', 'ö', 'ò', 'û', 'ù', 'ÿ', 'Ö', 'Ü', '¢', '£', '¥', '₧', 'ƒ', 'á', 'í', 'ó', 'ú',
        'ñ', 'Ñ', 'ª', 'º', '¿', '⌐', '¬', '½', '¼', '¡', '«', '»', '░', '▒', '▓', '│', '┤', '╡',
        '╢', '╖', '╕', '╣', '║', '╗', '╝', '╜', '╛', '┐', '└', '┴', '┬', '├', '─', '┼', '╞', '╟',
        '╚', '╔', '╩', '╦', '╠', '═', '╬', '╧', '╨', '╤', '╥', '╙', '╘', '╒', '╓', '╫', '╪', '┘',
        '┌', '█', '▄', '▌', '▐', '▀', 'α', 'ß', 'Γ', 'π', 'Σ', 'σ', 'µ', 'τ', 'Φ', 'Θ', 'Ω', 'δ',
        '∞', 'φ', 'ε', '∩', '≡', '±', '≥', '≤', '⌠', '⌡', '÷', '≈', '°', '∙', '·', '√', 'ⁿ', '²',
        '■', '\u{a0}',
    ];
    const LO: [char; 32] = [
        ' ', '☺', '☻', '♥', '♦', '♣', '♠', '•', '◘', '○', '◙', '♂', '♀', '♪', '♫', '☼', '►', '◄',
        '↕', '‼', '¶', '§', '▬', '↨', '↑', '↓', '→', '←', '∟', '↔', '▲', '▼',
    ];
    let mut t = [' '; 256];
    let mut i = 0;
    while i < 256 {
        t[i] = if i < 32 {
            LO[i]
        } else if i < 127 {
            // SAFETY-free const conversion: ASCII range is always a valid char.
            i as u8 as char
        } else if i == 127 {
            '⌂'
        } else {
            HI[i - 128]
        };
        i += 1;
    }
    t
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_completes() {
        let f = build_phosphor_mono().unwrap();
        // Every printable ASCII character is hand-drawn.
        for c in ' '..='~' {
            assert!(f.has_own(c), "missing ASCII {c:?}");
        }
        // Ink stays in columns 0-6 for letters and digits (column 7 is spacing).
        for c in ('A'..='Z').chain('a'..='z').chain('0'..='9') {
            let g = f.own_glyph(c).unwrap();
            for y in 0..16 {
                assert!(!g.get(7, y), "{c:?} has ink in column 7, row {y}");
            }
            assert_eq!(g.rows[15], 0, "{c:?} uses row 15 (reserved for the cursor)");
        }
    }

    #[test]
    fn cp437_coverage() {
        let f = phosphor_mono();
        let missing: Vec<String> = CP437
            .iter()
            .filter(|c| !f.has_own(**c))
            .map(|c| format!("U+{:04X}", *c as u32))
            .collect();
        // A few rarely used CP437 symbols are allowed to come from Unifont.
        assert!(
            missing.len() <= 12,
            "too many CP437 glyphs missing: {missing:?}"
        );
    }

    #[test]
    fn rejects_bad_rows() {
        assert!(Font::parse_glyphs("t", 8, 16, "@ 41 A\n##\n").is_err());
        assert!(Font::parse_glyphs("t", 8, 16, "@ 41 A\n@ 41 A\n").is_err());
    }

    #[test]
    fn hex_fallback() {
        let mut f = Font::new("t", 8, 16);
        let n = f.load_hex_fallback("4E00:0000000000000000000000000000000000000000000000000000000000007FFE\n0041:0000000018242442427E424242420000\n");
        assert_eq!(n, 2);
        assert!(f.glyph('一').unwrap().is_wide(8));
        assert!(f.glyph('A').unwrap().get(3, 4));
    }

    #[test]
    fn psf2_roundtrip_header() {
        let f = phosphor_mono();
        let psf = f.to_psf2(&CP437);
        assert_eq!(&psf[0..4], &0x864A_B572u32.to_le_bytes());
        assert_eq!(u32::from_le_bytes(psf[16..20].try_into().unwrap()), 256);
        assert!(psf.len() > 32 + 256 * 16);
    }
}

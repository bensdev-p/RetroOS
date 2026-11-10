//! A pixel font as a TrueType font (D-056): every lit pixel of a bitmap
//! glyph becomes a square of the outline (runs on a row merged into one
//! rectangle), 128 units to the pixel. At the design size (13 px for
//! Lantern 12, and 26, 39 … px) every square falls on whole screen pixels,
//! so GTK, Firefox and Qt draw the RetroOS system font as crisply as our
//! own programs do. No hinting is needed.
//!
//! Only the tables FreeType and fontconfig need: head, hhea, maxp, OS/2,
//! hmtx, cmap (format 4), loca (long), glyf, name, post (version 3).

use retro_gfx::font::{Font, Glyph};

/// Font units per bitmap pixel.
pub const UNIT: i32 = 128;

pub struct Names<'a> {
    pub family: &'a str,
    pub copyright: &'a str,
    pub version: &'a str,
}

/// The glyph's outline: one rectangle per run of lit pixels on a row,
/// clockwise (TrueType's outer contours), in font units with the baseline
/// at y = 0 below row `baseline_row`.
pub fn rectangles(g: &Glyph, baseline_row: i32) -> Vec<[(i32, i32); 4]> {
    let mut out = Vec::new();
    for y in 0..g.height as i32 {
        let mut x = 0;
        while x < g.width as i32 {
            if !g.get(x, y) {
                x += 1;
                continue;
            }
            let x0 = x;
            while x < g.width as i32 && g.get(x, y) {
                x += 1;
            }
            let top = (baseline_row + 1 - y) * UNIT;
            let bottom = top - UNIT;
            out.push([
                (x0 * UNIT, top),
                (x * UNIT, top),
                (x * UNIT, bottom),
                (x0 * UNIT, bottom),
            ]);
        }
    }
    out
}

struct Out(Vec<u8>);

impl Out {
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn i16(&mut self, v: i16) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn pad4(&mut self) {
        while !self.0.len().is_multiple_of(4) {
            self.0.push(0);
        }
    }
}

fn checksum(data: &[u8]) -> u32 {
    let mut sum = 0u32;
    for chunk in data.chunks(4) {
        let mut b = [0u8; 4];
        b[..chunk.len()].copy_from_slice(chunk);
        sum = sum.wrapping_add(u32::from_be_bytes(b));
    }
    sum
}

struct Shape {
    advance: i32,
    rects: Vec<[(i32, i32); 4]>,
}

impl Shape {
    fn bbox(&self) -> (i32, i32, i32, i32) {
        let mut b = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for r in &self.rects {
            for &(x, y) in r {
                b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
            }
        }
        if self.rects.is_empty() {
            (0, 0, 0, 0)
        } else {
            b
        }
    }
}

/// Write `font` (cell height `cell_h`, baseline under row `baseline_row`,
/// x-height and cap height in pixels) as a TrueType file.
pub fn write(
    font: &Font,
    baseline_row: i32,
    x_height: i32,
    cap_height: i32,
    names: &Names,
) -> Vec<u8> {
    let cell_h = font.cell_h as i32;
    let ascent = (baseline_row + 1) * UNIT;
    let descent = (cell_h - baseline_row - 1) * UNIT;
    // Glyph 0 is .notdef (the font's replacement box); then every own
    // glyph in code point order (BMP only: cmap format 4).
    let mut chars: Vec<char> = font
        .chars()
        .into_iter()
        .filter(|c| (*c as u32) < 0xFFFF && font.has_own(*c))
        .collect();
    chars.sort();
    let mut shapes = vec![{
        let g = font.glyph_or_replacement('\u{FFFF}');
        Shape {
            advance: g.width as i32 * UNIT,
            rects: rectangles(g, baseline_row),
        }
    }];
    for c in &chars {
        let g = font.own_glyph(*c).expect("own glyph");
        shapes.push(Shape {
            advance: g.width as i32 * UNIT,
            rects: rectangles(g, baseline_row),
        });
    }
    let n = shapes.len();

    // glyf + loca (long offsets).
    let mut glyf = Out(Vec::new());
    let mut loca = Out(Vec::new());
    let (mut max_points, mut max_contours) = (0usize, 0usize);
    for s in &shapes {
        loca.u32(glyf.0.len() as u32);
        if s.rects.is_empty() {
            continue;
        }
        let (x0, y0, x1, y1) = s.bbox();
        glyf.i16(s.rects.len() as i16);
        glyf.i16(x0 as i16);
        glyf.i16(y0 as i16);
        glyf.i16(x1 as i16);
        glyf.i16(y1 as i16);
        for i in 0..s.rects.len() {
            glyf.u16((i * 4 + 3) as u16);
        }
        glyf.u16(0); // no instructions
        let points: Vec<(i32, i32)> = s.rects.iter().flatten().copied().collect();
        glyf.0.extend(std::iter::repeat_n(1u8, points.len())); // on-curve
        let mut last = 0;
        for &(x, _) in &points {
            glyf.i16((x - last) as i16);
            last = x;
        }
        last = 0;
        for &(_, y) in &points {
            glyf.i16((y - last) as i16);
            last = y;
        }
        glyf.pad4();
        max_points = max_points.max(points.len());
        max_contours = max_contours.max(s.rects.len());
    }
    loca.u32(glyf.0.len() as u32);

    let bboxes: Vec<_> = shapes.iter().map(Shape::bbox).collect();
    let x_min = bboxes.iter().map(|b| b.0).min().unwrap_or(0);
    let y_min = bboxes.iter().map(|b| b.1).min().unwrap_or(0);
    let x_max = bboxes.iter().map(|b| b.2).max().unwrap_or(0);
    let y_max = bboxes.iter().map(|b| b.3).max().unwrap_or(0);
    let adv_max = shapes.iter().map(|s| s.advance).max().unwrap_or(0);
    let avg = shapes.iter().map(|s| s.advance).sum::<i32>() / n as i32;

    // hmtx: advance and left side bearing.
    let mut hmtx = Out(Vec::new());
    for (s, b) in shapes.iter().zip(&bboxes) {
        hmtx.u16(s.advance as u16);
        hmtx.i16(b.0 as i16);
    }

    // head (checkSumAdjustment filled in at the end).
    let mut head = Out(Vec::new());
    head.u32(0x0001_0000);
    head.u32(0x0001_0000); // fontRevision 1.0
    head.u32(0); // checkSumAdjustment
    head.u32(0x5F0F_3CF5);
    head.u16(0x000B); // baseline at 0, lsb at 0, integer ppem
    head.u16((cell_h * UNIT) as u16);
    head.0.extend_from_slice(&[0u8; 16]); // created, modified: fixed (reproducible)
    head.i16(x_min as i16);
    head.i16(y_min as i16);
    head.i16(x_max as i16);
    head.i16(y_max as i16);
    head.u16(0); // macStyle
    head.u16(cell_h as u16); // lowestRecPPEM
    head.i16(2);
    head.i16(1); // long loca
    head.i16(0);

    let mut hhea = Out(Vec::new());
    hhea.u32(0x0001_0000);
    hhea.i16(ascent as i16);
    hhea.i16(-descent as i16);
    hhea.i16(0);
    hhea.u16(adv_max as u16);
    hhea.i16(0); // minLeftSideBearing
    hhea.i16(0); // minRightSideBearing
    hhea.i16(x_max as i16);
    hhea.i16(1);
    hhea.i16(0);
    hhea.i16(0);
    hhea.0.extend_from_slice(&[0u8; 8]);
    hhea.i16(0);
    hhea.u16(n as u16);

    let mut maxp = Out(Vec::new());
    maxp.u32(0x0001_0000);
    maxp.u16(n as u16);
    maxp.u16(max_points as u16);
    maxp.u16(max_contours as u16);
    maxp.u16(0);
    maxp.u16(0);
    maxp.u16(2); // maxZones
    for _ in 0..8 {
        maxp.u16(0);
    }

    let first = chars.first().map_or(0x20, |c| *c as u32) as u16;
    let last = chars.last().map_or(0x20, |c| *c as u32) as u16;
    let mut os2 = Out(Vec::new());
    os2.u16(4);
    os2.i16(avg as i16);
    os2.u16(400);
    os2.u16(5);
    os2.u16(0); // fsType: installable
                // Sub- and superscript sizes and offsets, in thousandths of the em.
    let em = cell_h * UNIT;
    for v in [650, 600, 0, 75, 650, 600, 0, 350] {
        os2.i16((em * v / 1000) as i16);
    }
    os2.i16(UNIT as i16); // strikeout size
    os2.i16((x_height * UNIT / 2) as i16); // strikeout position
    os2.i16(0); // sFamilyClass
    os2.0.extend_from_slice(&[0u8; 10]); // panose
    os2.u32(0x0000_0003); // Basic Latin, Latin-1 Supplement
    os2.u32(0);
    os2.u32(0);
    os2.u32(0);
    os2.0.extend_from_slice(b"RTRO");
    os2.u16(0x0040); // REGULAR
    os2.u16(first);
    os2.u16(last);
    os2.i16(ascent as i16);
    os2.i16(-descent as i16);
    os2.i16(0);
    os2.u16(ascent as u16);
    os2.u16(descent as u16);
    os2.u32(1); // Latin 1
    os2.u32(0);
    os2.i16((x_height * UNIT) as i16);
    os2.i16((cap_height * UNIT) as i16);
    os2.u16(0);
    os2.u16(0x20);
    os2.u16(0);

    // cmap: format 4, one segment per run of consecutive code points.
    let mut segs: Vec<(u16, u16, u16)> = Vec::new(); // (start, end, first glyph)
    for (i, c) in chars.iter().enumerate() {
        let cp = *c as u32 as u16;
        let gid = (i + 1) as u16;
        match segs.last_mut() {
            Some(s) if s.1 + 1 == cp => s.1 = cp,
            _ => segs.push((cp, cp, gid)),
        }
    }
    segs.push((0xFFFF, 0xFFFF, 0));
    let segx2 = (segs.len() * 2) as u16;
    let mut pow = 1u16;
    let mut log = 0u16;
    while pow * 2 <= segs.len() as u16 {
        pow *= 2;
        log += 1;
    }
    let mut sub = Out(Vec::new());
    sub.u16(4);
    sub.u16((16 + segs.len() * 8) as u16);
    sub.u16(0);
    sub.u16(segx2);
    sub.u16(pow * 2);
    sub.u16(log);
    sub.u16(segx2 - pow * 2);
    for s in &segs {
        sub.u16(s.1);
    }
    sub.u16(0);
    for s in &segs {
        sub.u16(s.0);
    }
    for s in &segs {
        // idDelta: glyph = code + delta (mod 65536); the last segment maps to 0... as 1.
        let delta = if s.0 == 0xFFFF {
            1u16
        } else {
            s.2.wrapping_sub(s.0)
        };
        sub.u16(delta);
    }
    for _ in &segs {
        sub.u16(0);
    }
    let mut cmap = Out(Vec::new());
    cmap.u16(0);
    cmap.u16(2);
    for (pid, eid) in [(0u16, 3u16), (3, 1)] {
        cmap.u16(pid);
        cmap.u16(eid);
        cmap.u32(4 + 2 * 8);
    }
    cmap.0.extend_from_slice(&sub.0);

    let records: Vec<(u16, String)> = vec![
        (0, names.copyright.to_string()),
        (1, names.family.to_string()),
        (2, "Regular".into()),
        (3, format!("RetroOS: {} {}", names.family, names.version)),
        (4, names.family.to_string()),
        (5, format!("Version {}", names.version)),
        (6, format!("{}-Regular", names.family.replace(' ', ""))),
    ];
    let mut name = Out(Vec::new());
    let mut strings = Vec::new();
    name.u16(0);
    name.u16(records.len() as u16);
    name.u16((6 + records.len() * 12) as u16);
    for (id, text) in &records {
        let utf16: Vec<u8> = text.encode_utf16().flat_map(|u| u.to_be_bytes()).collect();
        name.u16(3);
        name.u16(1);
        name.u16(0x0409);
        name.u16(*id);
        name.u16(utf16.len() as u16);
        name.u16(strings.len() as u16);
        strings.extend(utf16);
    }
    name.0.extend(strings);

    let mut post = Out(Vec::new());
    post.u32(0x0003_0000);
    post.u32(0);
    post.i16(-(UNIT as i16));
    post.i16(UNIT as i16);
    post.u32(0);
    post.0.extend_from_slice(&[0u8; 16]);

    let mut tables: Vec<(&[u8; 4], Vec<u8>)> = vec![
        (b"OS/2", os2.0),
        (b"cmap", cmap.0),
        (b"glyf", glyf.0),
        (b"head", head.0),
        (b"hhea", hhea.0),
        (b"hmtx", hmtx.0),
        (b"loca", loca.0),
        (b"maxp", maxp.0),
        (b"name", name.0),
        (b"post", post.0),
    ];
    tables.sort_by_key(|(t, _)| **t);
    let nt = tables.len() as u16;
    let mut pow = 1u16;
    let mut log = 0u16;
    while pow * 2 <= nt {
        pow *= 2;
        log += 1;
    }
    let mut out = Out(Vec::new());
    out.u32(0x0001_0000);
    out.u16(nt);
    out.u16(pow * 16);
    out.u16(log);
    out.u16(nt * 16 - pow * 16);
    let mut offset = 12 + 16 * tables.len();
    let mut body = Vec::new();
    let mut head_at = 0;
    for (tag, data) in &tables {
        out.0.extend_from_slice(*tag);
        out.u32(checksum(data));
        out.u32(offset as u32);
        out.u32(data.len() as u32);
        if *tag == b"head" {
            head_at = offset;
        }
        let mut d = data.clone();
        while d.len() % 4 != 0 {
            d.push(0);
        }
        offset += d.len();
        body.extend(d);
    }
    out.0.extend(body);
    let adj = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out.0));
    out.0[head_at + 8..head_at + 12].copy_from_slice(&adj.to_be_bytes());
    out.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_become_rectangles() {
        let mut g = Glyph::blank(4, 3);
        for x in 0..3 {
            g.set(x, 0);
        }
        g.set(1, 2);
        let r = rectangles(&g, 1);
        assert_eq!(r.len(), 2, "one per run");
        // Row 0 sits on top: its top at 2 pixels above the baseline.
        assert_eq!(r[0], [(0, 256), (384, 256), (384, 128), (0, 128)]);
        assert_eq!(r[1][3], (128, -128), "row 2 hangs below the baseline");
    }

    #[test]
    fn a_whole_font_is_well_formed() {
        let f = retro_gfx::font::lantern_12();
        let ttf = write(
            f,
            9,
            6,
            9,
            &Names {
                family: "Lantern",
                copyright: "(C) RetroOS contributors. SIL OFL 1.1.",
                version: "1.0",
            },
        );
        assert_eq!(&ttf[..4], &[0, 1, 0, 0]);
        assert_eq!(checksum(&ttf), 0xB1B0_AFBA, "whole-file checksum");
        assert!(ttf.windows(4).any(|w| w == b"glyf"));
    }
}

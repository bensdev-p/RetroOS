//! The RetroOS icon set (docs/STYLE.md §5): 32×32 pictures drawn as text in
//! `assets/icons/*.icon` (one Phosphor16 letter per pixel), and every other
//! program's own icon "retro-fied": scaled down, dithered to Phosphor16 and
//! outlined in Black, so that a Program Manager group looks like one set.

use crate::palette::*;
use crate::{Rgb, Surface, Theme};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// A picture: `w`×`h` pixels, each 0 (clear) or 0xFF000000 | RGB.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Icon {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u32>,
}

const OPAQUE: u32 = 0xFF00_0000;

fn letter(c: char) -> Option<Rgb> {
    Some(match c {
        'K' => BLACK,
        'B' => COBALT,
        'G' => FERN,
        'T' => TEAL,
        'R' => BRICK,
        'P' => PLUM,
        'O' => OCHRE,
        'S' => SILVER,
        'D' => SLATE,
        'Y' => SKY,
        'L' => LIME,
        'A' => AQUA,
        'C' => CORAL,
        'H' => ORCHID,
        'E' => LEMON,
        'W' => WHITE,
        _ => return None,
    })
}

/// Parse `.icon` text: '#' lines are comments, then one row per line.
pub fn parse(src: &str) -> Result<Icon, String> {
    let rows: Vec<&str> = src
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .collect();
    let h = rows.len() as u32;
    let w = rows.first().map_or(0, |r| r.chars().count()) as u32;
    if w == 0 || h == 0 {
        return Err("empty icon".into());
    }
    let mut px = Vec::with_capacity((w * h) as usize);
    for (y, r) in rows.iter().enumerate() {
        if r.chars().count() as u32 != w {
            return Err(format!("row {} is not {w} wide", y + 1));
        }
        for c in r.chars() {
            px.push(match c {
                '.' => 0,
                c => OPAQUE | letter(c).ok_or(format!("row {}: unknown colour {c:?}", y + 1))?,
            });
        }
    }
    Ok(Icon { w, h, px })
}

const BUILTIN: &[(&str, &str)] = &[
    ("jot", include_str!("../../../assets/icons/jot.icon")),
    (
        "file-cabinet",
        include_str!("../../../assets/icons/file-cabinet.icon"),
    ),
    (
        "web-browser",
        include_str!("../../../assets/icons/web-browser.icon"),
    ),
    (
        "control-panel",
        include_str!("../../../assets/icons/control-panel.icon"),
    ),
    (
        "cp-display",
        include_str!("../../../assets/icons/cp-display.icon"),
    ),
    (
        "cp-clock",
        include_str!("../../../assets/icons/cp-clock.icon"),
    ),
    (
        "cp-keyboard",
        include_str!("../../../assets/icons/cp-keyboard.icon"),
    ),
    (
        "cp-network",
        include_str!("../../../assets/icons/cp-network.icon"),
    ),
    (
        "cp-startup",
        include_str!("../../../assets/icons/cp-startup.icon"),
    ),
    (
        "cp-users",
        include_str!("../../../assets/icons/cp-users.icon"),
    ),
];

/// Theme icon names (from .desktop files) that our own pictures stand for.
const ALIASES: &[(&str, &str)] = &[
    ("accessories-text-editor", "jot"),
    ("text-editor", "jot"),
    ("system-file-manager", "file-cabinet"),
    ("folder", "file-cabinet"),
    ("firefox", "web-browser"),
    ("firefox-esr", "web-browser"),
    ("org.mozilla.firefox", "web-browser"),
    ("chromium", "web-browser"),
    ("chromium-browser", "web-browser"),
    ("web-browser", "web-browser"),
    ("preferences-system", "control-panel"),
];

/// One of our own icons by name ("jot", "file-cabinet", "web-browser").
pub fn builtin(name: &str) -> Option<&'static Icon> {
    static SET: OnceLock<Vec<(&'static str, Icon)>> = OnceLock::new();
    let set = SET.get_or_init(|| {
        BUILTIN
            .iter()
            .map(|(n, src)| (*n, parse(src).expect("assets/icons are valid")))
            .collect()
    });
    let name = ALIASES
        .iter()
        .find(|(a, _)| *a == name)
        .map_or(name, |(_, n)| *n);
    set.iter().find(|(n, _)| *n == name).map(|(_, i)| i)
}

/// The icon for a program's `Icon=` value: ours, else its own retro-fied,
/// else None (the caller draws the generic program window).
pub fn for_app(icon: &str) -> Option<Icon> {
    if let Some(i) = builtin(icon) {
        return Some(i.clone());
    }
    let path = if icon.starts_with('/') {
        Some(PathBuf::from(icon)).filter(|p| p.is_file())
    } else {
        find_png(icon)
    }?;
    let (w, h, px) = load_rgba(&path)?;
    Some(retrofy(w, h, &px))
}

/// A PNG for a theme icon name: hicolor, nearest to 32 px (larger first),
/// then /usr/share/pixmaps.
fn find_png(name: &str) -> Option<PathBuf> {
    let mut bases: Vec<PathBuf> = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into())
        .split(':')
        .map(PathBuf::from)
        .collect();
    if let Some(h) = std::env::var_os("HOME") {
        bases.insert(0, PathBuf::from(h).join(".local/share"));
    }
    for size in [
        "32x32", "48x48", "64x64", "128x128", "256x256", "24x24", "22x22", "16x16",
    ] {
        for b in &bases {
            let p = b
                .join("icons/hicolor")
                .join(size)
                .join("apps")
                .join(format!("{name}.png"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    bases
        .iter()
        .map(|b| b.join("pixmaps").join(format!("{name}.png")))
        .find(|p| p.is_file())
}

/// A PNG as straight-alpha ARGB pixels.
fn load_rgba(path: &Path) -> Option<(u32, u32, Vec<u32>)> {
    let mut dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).ok()?));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut rd = dec.read_info().ok()?;
    let mut buf = vec![0; rd.output_buffer_size()];
    let info = rd.next_frame(&mut buf).ok()?;
    let ch = match info.color_type {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return None,
    };
    let px = buf[..info.buffer_size()]
        .chunks(ch)
        .map(|c| {
            let (r, g, b, a) = match ch {
                1 => (c[0], c[0], c[0], 255),
                2 => (c[0], c[0], c[0], c[1]),
                3 => (c[0], c[1], c[2], 255),
                _ => (c[0], c[1], c[2], c[3]),
            };
            (a as u32) << 24 | (r as u32) << 16 | (g as u32) << 8 | b as u32
        })
        .collect();
    Some((info.width, info.height, px))
}

/// Make any picture one of ours: fit it into 30×30 (box filter), threshold
/// its alpha, dither its colours to Phosphor16 (4×4 ordered dither) and put a
/// 1-pixel Black outline around it, centred in 32×32.
pub fn retrofy(w: u32, h: u32, src: &[u32]) -> Icon {
    const N: u32 = 32;
    const FIT: f32 = 30.0;
    let scale = (FIT / w.max(h).max(1) as f32).min(1.0);
    let (dw, dh) = (
        ((w as f32 * scale).round() as u32).max(1),
        ((h as f32 * scale).round() as u32).max(1),
    );
    let (ox, oy) = ((N - dw) / 2, (N - dh) / 2);
    const BAYER: [[f32; 4]; 4] = [
        [0.0, 8.0, 2.0, 10.0],
        [12.0, 4.0, 14.0, 6.0],
        [3.0, 11.0, 1.0, 9.0],
        [15.0, 7.0, 13.0, 5.0],
    ];
    let mut px = vec![0u32; (N * N) as usize];
    for y in 0..dh {
        for x in 0..dw {
            // The source pixels this one covers, averaged (alpha-weighted).
            let x0 = (x * w) / dw;
            let x1 = (((x + 1) * w) / dw).max(x0 + 1).min(w);
            let y0 = (y * h) / dh;
            let y1 = (((y + 1) * h) / dh).max(y0 + 1).min(h);
            let (mut r, mut g, mut b, mut a, mut n) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let p = src[(sy * w + sx) as usize];
                    let pa = (p >> 24) as f32 / 255.0;
                    r += ((p >> 16) & 0xFF) as f32 * pa;
                    g += ((p >> 8) & 0xFF) as f32 * pa;
                    b += (p & 0xFF) as f32 * pa;
                    a += pa;
                    n += 1.0;
                }
            }
            if a / n < 0.5 {
                continue;
            }
            let t = (BAYER[(y % 4) as usize][(x % 4) as usize] - 7.5) * 4.0;
            let c = [r / a + t, g / a + t, b / a + t];
            px[((oy + y) * N + ox + x) as usize] = OPAQUE | nearest(c);
        }
    }
    // Outline: clear pixels next to the picture become Black.
    let at = |px: &[u32], x: i32, y: i32| {
        x >= 0
            && y >= 0
            && x < N as i32
            && y < N as i32
            && px[(y as u32 * N + x as u32) as usize] != 0
    };
    let mut out = px.clone();
    for y in 0..N as i32 {
        for x in 0..N as i32 {
            if !at(&px, x, y)
                && (at(&px, x - 1, y)
                    || at(&px, x + 1, y)
                    || at(&px, x, y - 1)
                    || at(&px, x, y + 1))
            {
                out[(y as u32 * N + x as u32) as usize] = OPAQUE | BLACK;
            }
        }
    }
    Icon {
        w: N,
        h: N,
        px: out,
    }
}

fn nearest(c: [f32; 3]) -> Rgb {
    *PHOSPHOR16
        .iter()
        .min_by(|a, b| {
            let d = |p: &Rgb| {
                let q = [
                    ((p >> 16) & 0xFF) as f32,
                    ((p >> 8) & 0xFF) as f32,
                    (p & 0xFF) as f32,
                ];
                // Weighted by how much the eye cares (green most).
                3.0 * (q[0] - c[0]).powi(2)
                    + 4.0 * (q[1] - c[1]).powi(2)
                    + 2.0 * (q[2] - c[2]).powi(2)
            };
            d(a).total_cmp(&d(b))
        })
        .expect("16 colours")
}

/// Draw an icon at physical (x, y), `p` physical pixels per icon pixel, in
/// the theme's colours (monochrome schemes map it to their four steps).
pub fn draw(sf: &mut Surface, icon: &Icon, x: i32, y: i32, p: i32, th: &Theme) {
    for iy in 0..icon.h {
        for ix in 0..icon.w {
            let v = icon.px[(iy * icon.w + ix) as usize];
            if v == 0 {
                continue;
            }
            let c = th.picture(v & 0xFF_FFFF);
            sf.fill_rect(x + ix as i32 * p, y + iy as i32 * p, p as u32, p as u32, c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_set_parses_and_is_32_square() {
        for (n, _) in BUILTIN {
            let i = builtin(n).unwrap();
            assert_eq!((i.w, i.h), (32, 32), "{n}");
        }
        assert!(builtin("accessories-text-editor").is_some());
        assert!(parse("KX\n").is_err());
    }

    #[test]
    fn retrofied_icons_use_the_palette_and_have_an_outline() {
        // A soft orange disc on a clear background, 64×64.
        let mut src = vec![0u32; 64 * 64];
        for y in 0..64 {
            for x in 0..64 {
                if (x as f32 - 31.5).hypot(y as f32 - 31.5) < 28.0 {
                    src[y * 64 + x] = 0xFF00_0000 | 0xE08030;
                }
            }
        }
        let i = retrofy(64, 64, &src);
        assert_eq!((i.w, i.h), (32, 32));
        assert!(i
            .px
            .iter()
            .all(|p| *p == 0 || PHOSPHOR16.contains(&(p & 0xFF_FFFF))));
        // The outermost opaque pixel on the middle row is the outline.
        let row = &i.px[16 * 32..17 * 32];
        let first = row.iter().position(|p| *p != 0).unwrap();
        assert_eq!(row[first] & 0xFF_FFFF, BLACK);
        assert_eq!(i.px[0], 0, "corners stay clear");
    }
}

#[cfg(test)]
mod sheet {
    /// RETROFY_OUT=sheet.png cargo test -p retro-gfx retrofy_sheet -- --ignored
    #[test]
    #[ignore = "writes a contact sheet of retro-fied icons found on this machine"]
    fn retrofy_sheet() {
        let names = [
            "libreoffice-writer",
            "libreoffice-calc",
            "libreoffice-impress",
            "display-im6.q16",
            "firefox",
            "gimp",
            "vlc",
        ];
        let found: Vec<super::Icon> = names.iter().filter_map(|n| super::for_app(n)).collect();
        let mut c = crate::Canvas::new(40 * found.len().max(1) as u32 * 3, 40 * 3, 0x1F3BB3);
        let th = crate::Theme::default();
        for (i, icon) in found.iter().enumerate() {
            super::draw(&mut c.surface(), icon, (i as i32 * 40 + 4) * 3, 12, 3, &th);
        }
        c.save_png(std::env::var("RETROFY_OUT").unwrap_or("sheet.png".into()))
            .unwrap();
        println!("{} icons", found.len());
    }
}

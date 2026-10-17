//! What the Desktop personality draws itself (docs/STYLE.md §4): window
//! frames, title bars, the dithered desktop, minimised-window icons, the
//! window menu, the Alt+Tab box and message boxes.
//!
//! Sizes are in art pixels ("logical pixels" in STYLE.md), multiplied by
//! [`Metrics`]: `u` logical pixels per art pixel for geometry, `px` physical
//! pixels for drawing, so frames stay pixel-exact at every output scale.
//! Text uses Phosphor Mono until the Lantern UI fonts exist (D-030).

use retro_gfx::font::phosphor_mono;
use retro_gfx::palette::{BLACK, COBALT, SILVER, SKY, SLATE, WHITE};
use retro_gfx::{Canvas, Rgb, Surface};
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::memory::MemoryRenderBuffer;
use smithay::utils::{Logical, Rectangle, Size, Transform};

/// Frame border, title bar and the line under it (art px).
pub const BORDER: i32 = 4;
pub const TITLE: i32 = 18;
/// Corner segments of the frame (marked by notches) resize diagonally.
pub const CORNER: i32 = TITLE + BORDER;
/// Minimised-window icon cell: 32×32 icon over a one-line label.
pub const ICON_W: i32 = 80;
pub const ICON_H: i32 = 52;
/// A pixel value meaning "transparent" in canvases that need it.
pub const CLEAR_PX: Rgb = 0x0100_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metrics {
    /// Logical pixels per art pixel.
    pub u: i32,
    /// Physical pixels per art pixel.
    pub px: i32,
    /// Output scale (physical pixels per logical pixel).
    pub scale: i32,
}

impl Metrics {
    /// Our art at `art_scale` physical pixels per art pixel, rounded down to
    /// whole logical pixels so frames line up with client surfaces.
    pub fn new(scale: i32, art_scale: i32) -> Metrics {
        let scale = scale.max(1);
        let u = (art_scale / scale).max(1);
        Metrics {
            u,
            px: u * scale,
            scale,
        }
    }

    pub fn border(&self) -> i32 {
        BORDER * self.u
    }

    /// From the frame's top edge to the client area.
    pub fn top(&self) -> i32 {
        (BORDER + TITLE + 1) * self.u
    }

    /// The frame around a client rectangle.
    pub fn outer(&self, client: Rectangle<i32, Logical>) -> Rectangle<i32, Logical> {
        let b = self.border();
        Rectangle::new(
            (client.loc.x - b, client.loc.y - self.top()).into(),
            (client.size.w + 2 * b, client.size.h + self.top() + b).into(),
        )
    }

    /// The client rectangle inside a frame.
    pub fn inner(&self, outer: Rectangle<i32, Logical>) -> Rectangle<i32, Logical> {
        let b = self.border();
        Rectangle::new(
            (outer.loc.x + b, outer.loc.y + self.top()).into(),
            (
                (outer.size.w - 2 * b).max(1),
                (outer.size.h - self.top() - b).max(1),
            )
                .into(),
        )
    }

    /// Smallest client size that still shows the title bar's controls.
    pub fn min_client(&self) -> Size<i32, Logical> {
        ((3 * TITLE + 40) * self.u, 2 * self.u).into()
    }
}

// ------------------------------------------------------------------ frames

/// The frame's flat parts, front to back, relative to the frame's top-left:
/// notches, then the inner line, the Silver body and the Black outer line.
/// They are strips, so nothing is drawn under the client's surface.
pub fn frame_rects(w: i32, h: i32, m: &Metrics) -> Vec<(Rectangle<i32, Logical>, Rgb)> {
    let u = m.u;
    let r =
        |x: i32, y: i32, w: i32, h: i32| Rectangle::new((x, y).into(), (w.max(0), h.max(0)).into());
    let c = CORNER * u;
    let mut v = Vec::with_capacity(20);
    // Notches across the body where the corner segments end.
    for (x, y, nw, nh) in [
        (c, u, u, 2 * u),
        (w - c - u, u, u, 2 * u),
        (c, h - 3 * u, u, 2 * u),
        (w - c - u, h - 3 * u, u, 2 * u),
        (u, c, 2 * u, u),
        (u, h - c - u, 2 * u, u),
        (w - 3 * u, c, 2 * u, u),
        (w - 3 * u, h - c - u, 2 * u, u),
    ] {
        v.push((r(x, y, nw, nh), BLACK));
    }
    // Lines and bands as four strips each: (inset, thickness, colour).
    for (i, t, col) in [(3 * u, u, BLACK), (u, 2 * u, SILVER), (0, u, BLACK)] {
        v.push((r(i, i, w - 2 * i, t), col));
        v.push((r(i, h - i - t, w - 2 * i, t), col));
        v.push((r(i, i + t, t, h - 2 * i - 2 * t), col));
        v.push((r(w - i - t, i + t, t, h - 2 * i - 2 * t), col));
    }
    v
}

/// Resize edges (bit set).
pub const LEFT: u8 = 1;
pub const RIGHT: u8 = 2;
pub const TOP: u8 = 4;
pub const BOTTOM: u8 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// Frame border; the edges it resizes (0 = not resizable).
    Border(u8),
    Caption,
    Control,
    Minimize,
    Maximize,
    Client,
}

/// What is at `(x, y)` (relative to the frame's top-left) in a `w`×`h` frame.
pub fn frame_hit(
    w: i32,
    h: i32,
    m: &Metrics,
    x: i32,
    y: i32,
    resizable: bool,
    buttons: bool,
) -> Option<Part> {
    if x < 0 || y < 0 || x >= w || y >= h {
        return None;
    }
    let (u, b, c) = (m.u, m.border(), CORNER * m.u);
    let in_x = x < b || x >= w - b;
    let in_y = y < b || y >= h - b;
    if in_x || in_y {
        if !resizable {
            return Some(Part::Border(0));
        }
        let mut e = 0;
        if x < b || (in_y && x < c) {
            e |= LEFT;
        }
        if x >= w - b || (in_y && x >= w - c) {
            e |= RIGHT;
        }
        if y < b || (in_x && y < c) {
            e |= TOP;
        }
        if y >= h - b || (in_x && y >= h - c) {
            e |= BOTTOM;
        }
        return Some(Part::Border(e));
    }
    if y < b + TITLE * u {
        let tx = x - b;
        let tw = w - 2 * b;
        if tx < TITLE * u {
            return Some(Part::Control);
        }
        if buttons {
            if tx >= tw - TITLE * u {
                return Some(Part::Maximize);
            }
            if tx >= tw - (2 * TITLE + 1) * u && tx < tw - (TITLE + 1) * u {
                return Some(Part::Minimize);
            }
        }
        return Some(Part::Caption);
    }
    if y < m.top() {
        return Some(Part::Caption);
    }
    Some(Part::Client)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TitleStyle {
    pub title: String,
    pub active: bool,
    pub maximized: bool,
    /// Minimise and maximise buttons (dialogs have none).
    pub buttons: bool,
    pub pressed: Option<Part>,
}

/// The title bar and the line under it, `w` logical pixels wide.
pub fn draw_title(w: i32, m: &Metrics, s: &TitleStyle) -> Canvas {
    let p = m.px;
    let wp = (w * m.scale).max(1) as u32;
    let hp = ((TITLE + 1) * p) as u32;
    let (bg, fg) = if s.active {
        (COBALT, WHITE)
    } else {
        (SILVER, SLATE)
    };
    let mut c = Canvas::new(wp, hp, bg);
    let mut sf = c.surface();
    let wp = wp as i32;
    let t = TITLE * p;
    // Control-menu box and its separator.
    sf.fill_rect(0, 0, t as u32, t as u32, SILVER);
    sf.fill_rect(t, 0, p as u32, t as u32, BLACK);
    let (bx, by) = (3 * p, 7 * p);
    sf.fill_rect(bx + p, by + p, (12 * p) as u32, (4 * p) as u32, SLATE);
    sf.frame(bx, by, (12 * p) as u32, (4 * p) as u32, p as u32, BLACK);
    sf.fill_rect(bx + p, by + p, (10 * p) as u32, (2 * p) as u32, WHITE);
    let mut text_right = wp;
    if s.buttons {
        let max_x = wp - t;
        let min_x = wp - 2 * t - p;
        sf.fill_rect(max_x - p, 0, p as u32, t as u32, BLACK);
        sf.fill_rect(min_x - p, 0, p as u32, t as u32, BLACK);
        let restore = s.maximized;
        button(
            &mut sf,
            min_x,
            t,
            p,
            s.pressed == Some(Part::Minimize),
            |sf, cx, cy| triangle(sf, cx, cy, p, false, 4),
        );
        button(
            &mut sf,
            max_x,
            t,
            p,
            s.pressed == Some(Part::Maximize),
            |sf, cx, cy| {
                if restore {
                    triangle(sf, cx, cy - 2 * p, p, true, 3);
                    triangle(sf, cx, cy + 2 * p, p, false, 3);
                } else {
                    triangle(sf, cx, cy, p, true, 4);
                }
            },
        );
        text_right = min_x - p;
    }
    // Centred title.
    let font = phosphor_mono();
    let left = t + p;
    let avail = ((text_right - left) / (8 * p)).max(0) as usize;
    let title = ellipsize(&s.title, avail);
    let tw = title.chars().count() as i32 * 8 * p;
    let tx = left + ((text_right - left) - tw) / 2;
    sf.text(font, tx, p, p as u32, fg, None, &title);
    sf.fill_rect(0, t, wp as u32, p as u32, BLACK);
    c
}

/// A bevelled title-bar button at `x` (art: `t`×`t`), with its glyph.
fn button(
    sf: &mut Surface,
    x: i32,
    t: i32,
    p: i32,
    pressed: bool,
    glyph: impl Fn(&mut Surface, i32, i32),
) {
    sf.fill_rect(x, 0, t as u32, t as u32, SILVER);
    let (hi, lo) = if pressed {
        (SLATE, WHITE)
    } else {
        (WHITE, SLATE)
    };
    sf.fill_rect(x, 0, t as u32, p as u32, hi);
    sf.fill_rect(x, 0, p as u32, t as u32, hi);
    sf.fill_rect(x, t - p, t as u32, p as u32, lo);
    sf.fill_rect(x + t - p, 0, p as u32, t as u32, lo);
    let off = if pressed { p } else { 0 };
    glyph(sf, x + t / 2 + off, t / 2 + off);
}

/// A Black triangle of `rows` art rows (widths 1, 3, 5…), centred on (cx, cy).
fn triangle(sf: &mut Surface, cx: i32, cy: i32, p: i32, up: bool, rows: i32) {
    for r in 0..rows {
        let half = if up { r } else { rows - 1 - r };
        let y = cy - rows * p / 2 + r * p;
        sf.fill_rect(
            cx - half * p - p / 2,
            y,
            ((2 * half + 1) * p) as u32,
            p as u32,
            BLACK,
        );
    }
}

/// `s` cut to `n` characters, ending in "..." when cut.
pub fn ellipsize(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    if n <= 3 {
        return s.chars().take(n).collect();
    }
    let mut out: String = s.chars().take(n - 3).collect();
    out.push_str("...");
    out
}

/// The move/resize outline: a Black line inside a White one, visible on any
/// background (an XOR outline needs to read the screen back).
pub fn outline_rects(
    r: Rectangle<i32, Logical>,
    m: &Metrics,
) -> Vec<(Rectangle<i32, Logical>, Rgb)> {
    let u = m.u;
    let mut v = Vec::with_capacity(8);
    for (i, col) in [(u, BLACK), (0, WHITE)] {
        let (x, y, w, h) = (r.loc.x + i, r.loc.y + i, r.size.w - 2 * i, r.size.h - 2 * i);
        let rr = |x: i32, y: i32, w: i32, h: i32| {
            Rectangle::new((x, y).into(), (w.max(0), h.max(0)).into())
        };
        v.push((rr(x, y, w, u), col));
        v.push((rr(x, y + h - u, w, u), col));
        v.push((rr(x, y + u, u, h - 2 * u), col));
        v.push((rr(x + w - u, y + u, u, h - 2 * u), col));
    }
    v
}

// ----------------------------------------------------------------- desktop

/// The desktop pattern at a physical pixel: an 8×8 two-colour dither
/// (STYLE.md §4), here a 50 % Cobalt/Black checkerboard.
pub fn pattern_at(x: i32, y: i32, m: &Metrics) -> Rgb {
    if ((x / m.px) + (y / m.px)) & 1 == 0 {
        COBALT
    } else {
        BLACK
    }
}

/// A full-width band of the desktop pattern, `rows` art pixels tall.
pub fn pattern_band(width_phys: u32, rows: i32, m: &Metrics) -> Canvas {
    let h = (rows * m.px) as u32;
    let mut c = Canvas::new(width_phys, h, 0);
    for y in 0..h {
        for x in 0..width_phys {
            c.px[(y * width_phys + x) as usize] = pattern_at(x as i32, y as i32, m);
        }
    }
    c
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconArt {
    /// A generic program window.
    Program,
    /// A DOS Prompt (the RetroDOS console).
    Dos,
}

/// A minimised window: icon and label over the desktop pattern, drawn at
/// physical position (`ox`, `oy`) so the pattern lines up.
pub fn draw_icon(
    art: IconArt,
    label: &str,
    selected: bool,
    m: &Metrics,
    ox: i32,
    oy: i32,
) -> Canvas {
    let p = m.px;
    let (w, h) = ((ICON_W * p) as u32, (ICON_H * p) as u32);
    let mut c = Canvas::new(w, h, 0);
    for y in 0..h {
        for x in 0..w {
            c.px[(y * w + x) as usize] = pattern_at(ox + x as i32, oy + y as i32, m);
        }
    }
    let mut sf = c.surface();
    icon_picture(&mut sf, art, (ICON_W - 32) / 2 * p, 0, p);
    // Label: White on Black, Sky when selected.
    let font = phosphor_mono();
    let text = ellipsize(label, (ICON_W / 8) as usize - 1);
    let tw = text.chars().count() as i32 * 8;
    let lx = (ICON_W - tw - 4) / 2;
    let bg = if selected { SKY } else { BLACK };
    sf.fill_rect(lx * p, 34 * p, ((tw + 4) * p) as u32, (18 * p) as u32, bg);
    sf.text(font, (lx + 2) * p, 35 * p, p as u32, WHITE, None, &text);
    c
}

/// The 32×32 picture of an icon at physical (x, y).
pub fn icon_picture(sf: &mut Surface, art: IconArt, x: i32, y: i32, p: i32) {
    let mut at = |ax: i32, ay: i32, w: i32, h: i32, col: Rgb| {
        sf.fill_rect(x + ax * p, y + ay * p, (w * p) as u32, (h * p) as u32, col)
    };
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

// ------------------------------------------------------------------- menus

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuItem {
    /// `&` marks the mnemonic letter; an empty label is a separator.
    pub label: String,
    pub shortcut: String,
    pub enabled: bool,
}

impl MenuItem {
    pub fn new(label: &str, shortcut: &str, enabled: bool) -> MenuItem {
        MenuItem {
            label: label.into(),
            shortcut: shortcut.into(),
            enabled,
        }
    }

    pub fn separator() -> MenuItem {
        MenuItem::new("", "", false)
    }

    pub fn is_separator(&self) -> bool {
        self.label.is_empty()
    }

    pub fn mnemonic(&self) -> Option<char> {
        let i = self.label.find('&')?;
        self.label[i + 1..]
            .chars()
            .next()
            .map(|c| c.to_ascii_lowercase())
    }
}

const ITEM_H: i32 = 18;
const SEP_H: i32 = 8;

/// Menu size (art px, without the shadow) and each item's (y, height).
pub fn menu_layout(items: &[MenuItem]) -> (i32, i32, Vec<(i32, i32)>) {
    let mut y = 2;
    let mut rows = Vec::new();
    let mut cols = 0;
    for it in items {
        let h = if it.is_separator() { SEP_H } else { ITEM_H };
        rows.push((y, h));
        y += h;
        let label = it.label.replace('&', "").chars().count();
        let sc = it.shortcut.chars().count();
        cols = cols.max(label + if sc > 0 { sc + 3 } else { 0 });
    }
    ((cols as i32) * 8 + 32, y + 2, rows)
}

/// The menu with a 1-px drop shadow (right and bottom), selection `sel`.
pub fn draw_menu(items: &[MenuItem], sel: Option<usize>, m: &Metrics) -> Canvas {
    let p = m.px;
    let (w, h, rows) = menu_layout(items);
    let mut c = Canvas::new(((w + 1) * p) as u32, ((h + 1) * p) as u32, CLEAR_PX);
    let mut sf = c.surface();
    sf.fill_rect(p, p, (w * p) as u32, (h * p) as u32, BLACK);
    sf.fill_rect(0, 0, (w * p) as u32, (h * p) as u32, SILVER);
    sf.frame(0, 0, (w * p) as u32, (h * p) as u32, p as u32, BLACK);
    let font = phosphor_mono();
    for (i, (it, (y, ih))) in items.iter().zip(rows).enumerate() {
        if it.is_separator() {
            sf.fill_rect(p, (y + 3) * p, ((w - 2) * p) as u32, p as u32, SLATE);
            sf.fill_rect(p, (y + 4) * p, ((w - 2) * p) as u32, p as u32, WHITE);
            continue;
        }
        let selected = sel == Some(i);
        let fg = if !it.enabled {
            SLATE
        } else if selected {
            WHITE
        } else {
            BLACK
        };
        if selected {
            sf.fill_rect(2 * p, y * p, ((w - 4) * p) as u32, (ih * p) as u32, SKY);
        }
        let ty = (y + 1) * p;
        let mut x = 16 * p;
        let mut underline = false;
        for ch in it.label.chars() {
            if ch == '&' {
                underline = true;
                continue;
            }
            let g = font.glyph_or_replacement(ch);
            sf.glyph(g, x, ty, p as u32, fg, None);
            if underline {
                sf.fill_rect(x, ty + 13 * p, (7 * p) as u32, p as u32, fg);
                underline = false;
            }
            x += 8 * p;
        }
        if !it.shortcut.is_empty() {
            let sx = (w - 8 - it.shortcut.chars().count() as i32 * 8) * p;
            sf.text(font, sx, ty, p as u32, fg, None, &it.shortcut);
        }
    }
    c
}

/// The menu item at art position (x, y) inside the menu, if selectable.
pub fn menu_hit(items: &[MenuItem], x: i32, y: i32) -> Option<usize> {
    let (w, _, rows) = menu_layout(items);
    if x < 0 || x >= w {
        return None;
    }
    rows.iter()
        .position(|(ry, rh)| y >= *ry && y < ry + rh)
        .filter(|i| items[*i].enabled && !items[*i].is_separator())
}

// ------------------------------------------------------------------- boxes

/// The Alt+Tab box: icon and title of the window that will be switched to.
pub fn draw_switch_box(art: IconArt, title: &str, m: &Metrics) -> Canvas {
    let p = m.px;
    let (w, h) = (320, 52);
    let mut c = Canvas::new(((w + 1) * p) as u32, ((h + 1) * p) as u32, CLEAR_PX);
    let mut sf = c.surface();
    sf.fill_rect(p, p, (w * p) as u32, (h * p) as u32, BLACK);
    sf.fill_rect(0, 0, (w * p) as u32, (h * p) as u32, SILVER);
    sf.frame(0, 0, (w * p) as u32, (h * p) as u32, p as u32, BLACK);
    sf.frame(
        2 * p,
        2 * p,
        ((w - 4) * p) as u32,
        ((h - 4) * p) as u32,
        p as u32,
        COBALT,
    );
    icon_picture(&mut sf, art, 8 * p, 10 * p, p);
    let font = phosphor_mono();
    let text = ellipsize(title, ((w - 64) / 8) as usize);
    sf.text(font, 52 * p, 18 * p, p as u32, BLACK, None, &text);
    c
}

/// A message box: title bar, lines of text and one OK button.
/// Returns the canvas and the OK button's art rectangle (x, y, w, h).
pub fn draw_message(title: &str, lines: &[String], m: &Metrics) -> (Canvas, (i32, i32, i32, i32)) {
    let p = m.px;
    let cols = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as i32;
    let w = (cols * 8 + 32).max(200);
    let h = TITLE + 1 + 16 + lines.len() as i32 * 16 + 16 + 24 + 12;
    let mut c = Canvas::new(((w + 1) * p) as u32, ((h + 1) * p) as u32, CLEAR_PX);
    let mut sf = c.surface();
    sf.fill_rect(p, p, (w * p) as u32, (h * p) as u32, BLACK);
    sf.fill_rect(0, 0, (w * p) as u32, (h * p) as u32, SILVER);
    sf.frame(0, 0, (w * p) as u32, (h * p) as u32, p as u32, BLACK);
    let tb = draw_title(
        (w - 2) * m.u,
        m,
        &TitleStyle {
            title: title.into(),
            active: true,
            maximized: false,
            buttons: false,
            pressed: None,
        },
    );
    sf.blit(&tb, p, p);
    let font = phosphor_mono();
    let mut y = TITLE + 1 + 16;
    for l in lines {
        sf.text(font, 16 * p, y * p, p as u32, BLACK, None, l);
        y += 16;
    }
    // OK, the default button: an extra Black ring around the bevelled face.
    let (bw, bh) = (64, 24);
    let (bx, by) = ((w - bw) / 2, h - bh - 10);
    sf.frame(
        bx * p,
        by * p,
        (bw * p) as u32,
        (bh * p) as u32,
        p as u32,
        BLACK,
    );
    sf.frame(
        (bx + 1) * p,
        (by + 1) * p,
        ((bw - 2) * p) as u32,
        ((bh - 2) * p) as u32,
        p as u32,
        BLACK,
    );
    sf.fill_rect(
        (bx + 2) * p,
        (by + 2) * p,
        ((bw - 4) * p) as u32,
        p as u32,
        WHITE,
    );
    sf.fill_rect(
        (bx + 2) * p,
        (by + 2) * p,
        p as u32,
        ((bh - 4) * p) as u32,
        WHITE,
    );
    sf.fill_rect(
        (bx + 2) * p,
        (by + bh - 3) * p,
        ((bw - 4) * p) as u32,
        p as u32,
        SLATE,
    );
    sf.fill_rect(
        (bx + bw - 3) * p,
        (by + 2) * p,
        p as u32,
        ((bh - 4) * p) as u32,
        SLATE,
    );
    sf.text(
        font,
        (bx + bw / 2 - 8) * p,
        (by + 4) * p,
        p as u32,
        BLACK,
        None,
        "OK",
    );
    (c, (bx, by, bw, bh))
}

/// A canvas as a render buffer at output scale `scale`; [`CLEAR_PX`] pixels
/// become transparent.
pub fn to_buffer(c: &Canvas, scale: i32) -> MemoryRenderBuffer {
    let has_alpha = c.px.contains(&CLEAR_PX);
    let mut px = Vec::with_capacity(c.px.len() * 4);
    for &v in &c.px {
        let argb = if v == CLEAR_PX { 0 } else { v | 0xFF00_0000 };
        px.extend_from_slice(&argb.to_le_bytes());
    }
    MemoryRenderBuffer::from_slice(
        &px,
        if has_alpha {
            Fourcc::Argb8888
        } else {
            Fourcc::Xrgb8888
        },
        (c.width as i32, c.height as i32),
        scale,
        Transform::Normal,
        (!has_alpha).then(|| {
            vec![Rectangle::from_size(
                (c.width as i32, c.height as i32).into(),
            )]
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m2() -> Metrics {
        Metrics::new(1, 2)
    }

    #[test]
    fn metrics() {
        assert_eq!(
            Metrics::new(1, 2),
            Metrics {
                u: 2,
                px: 2,
                scale: 1
            }
        );
        assert_eq!(
            Metrics::new(2, 4),
            Metrics {
                u: 2,
                px: 4,
                scale: 2
            }
        );
        assert_eq!(
            Metrics::new(2, 2),
            Metrics {
                u: 1,
                px: 2,
                scale: 2
            }
        );
        let m = m2();
        let c = Rectangle::new((100, 100).into(), (640, 400).into());
        let o = m.outer(c);
        assert_eq!(o.loc, (92, 54).into());
        assert_eq!(o.size, (656, 454).into());
        assert_eq!(m.inner(o), c);
    }

    #[test]
    fn frame_rects_stay_in_the_frame() {
        let m = m2();
        for (r, _) in frame_rects(300, 200, &m) {
            assert!(r.loc.x >= 0 && r.loc.y >= 0, "{r:?}");
            assert!(
                r.loc.x + r.size.w <= 300 && r.loc.y + r.size.h <= 200,
                "{r:?}"
            );
        }
    }

    #[test]
    fn hit_test() {
        let m = m2();
        let (w, h) = (400, 300);
        let hit = |x, y| frame_hit(w, h, &m, x, y, true, true);
        assert_eq!(hit(-1, 5), None);
        assert_eq!(hit(1, 150), Some(Part::Border(LEFT)));
        assert_eq!(hit(200, 1), Some(Part::Border(TOP)));
        assert_eq!(hit(1, 1), Some(Part::Border(LEFT | TOP)));
        // Corner segments run along the edge up to the notch.
        assert_eq!(hit(30, 1), Some(Part::Border(LEFT | TOP)));
        assert_eq!(hit(60, 1), Some(Part::Border(TOP)));
        assert_eq!(hit(399, 299), Some(Part::Border(RIGHT | BOTTOM)));
        assert_eq!(hit(398, 290), Some(Part::Border(RIGHT | BOTTOM)));
        assert_eq!(hit(10, 10), Some(Part::Control));
        assert_eq!(hit(200, 10), Some(Part::Caption));
        assert_eq!(hit(390, 10), Some(Part::Maximize));
        assert_eq!(hit(330, 10), Some(Part::Minimize));
        assert_eq!(hit(355, 10), Some(Part::Caption)); // the separator
        assert_eq!(hit(200, 45), Some(Part::Caption));
        assert_eq!(hit(200, 100), Some(Part::Client));
        assert_eq!(
            frame_hit(w, h, &m, 390, 10, true, false),
            Some(Part::Caption)
        );
        assert_eq!(
            frame_hit(w, h, &m, 1, 1, false, true),
            Some(Part::Border(0))
        );
    }

    #[test]
    fn title_bar_pixels() {
        let m = m2();
        let s = TitleStyle {
            title: "DOS Prompt".into(),
            active: true,
            maximized: false,
            buttons: true,
            pressed: None,
        };
        let c = draw_title(400, &m, &s);
        assert_eq!((c.width, c.height), (400, 38));
        // Cobalt caption, Silver control box, Black line underneath.
        assert_eq!(c.get(100, 2), COBALT);
        assert_eq!(c.get(2, 20), SILVER);
        assert_eq!(c.get(200, 37), BLACK);
        // Bevel of the maximise button: White top-left.
        assert_eq!(c.get(400 - 36, 0), WHITE);
        let inactive = draw_title(400, &m, &TitleStyle { active: false, ..s });
        assert_eq!(inactive.get(100, 2), SILVER);
    }

    #[test]
    fn menus() {
        let items = vec![
            MenuItem::new("&Restore", "", false),
            MenuItem::new("&Move", "", true),
            MenuItem::separator(),
            MenuItem::new("&Close", "Alt+F4", true),
        ];
        assert_eq!(items[3].mnemonic(), Some('c'));
        let (w, h, rows) = menu_layout(&items);
        assert_eq!(rows.len(), 4);
        assert_eq!(h, 2 + 3 * 18 + 8 + 2);
        assert!(w >= 8 * 12);
        assert_eq!(menu_hit(&items, 20, 5), None); // disabled
        assert_eq!(menu_hit(&items, 20, 25), Some(1));
        assert_eq!(menu_hit(&items, 20, 42), None); // separator
        assert_eq!(menu_hit(&items, 20, 50), Some(3));
        let c = draw_menu(&items, Some(1), &m2());
        assert_eq!(c.get(c.width - 1, 0), CLEAR_PX);
        assert_eq!(c.get(10, 22 * 2), SKY);
        assert_eq!(ellipsize("Program Manager", 10), "Program...");
    }
}

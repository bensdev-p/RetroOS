//! The widgets every RetroOS GUI shares (docs/STYLE.md §4): Lantern 12
//! text, menus, bevelled buttons. Sizes are art pixels; `p` is the number
//! of output pixels per art pixel. Station draws its window menu with these,
//! retro-ui programs (the System Bar) their menus.

use crate::font::lantern_12;
use crate::palette::{BLACK, SILVER, SKY, SLATE, WHITE};
use crate::{Canvas, Rgb, Surface};

/// A pixel value meaning "transparent" in canvases that need it.
pub const CLEAR_PX: Rgb = 0x0100_0000;

/// Where Lantern 12 text sits in an 18-pixel bar (title bars, menu items).
pub const TEXT_Y: i32 = 3;

/// Width of `s` in Lantern 12 (art px).
pub fn text_width(s: &str) -> i32 {
    lantern_12().text_width(s) as i32
}

/// `s` cut to fit `max` art pixels, ending in "…" when cut.
pub fn ellipsize(s: &str, max: i32) -> String {
    if text_width(s) <= max {
        return s.to_string();
    }
    let mut out = String::new();
    for ch in s.chars() {
        let mut next = out.clone();
        next.push(ch);
        if text_width(&next) + text_width("…") > max {
            break;
        }
        out = next;
    }
    out.push('…');
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuItem {
    /// `&` marks the mnemonic letter; an empty label is a separator.
    pub label: String,
    pub shortcut: String,
    pub enabled: bool,
    /// A check mark in front (the System Bar's Window menu).
    pub checked: bool,
}

impl MenuItem {
    pub fn new(label: &str, shortcut: &str, enabled: bool) -> MenuItem {
        MenuItem {
            label: label.into(),
            shortcut: shortcut.into(),
            enabled,
            checked: false,
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
    let mut tw = 0;
    for it in items {
        let h = if it.is_separator() { SEP_H } else { ITEM_H };
        rows.push((y, h));
        y += h;
        let label = text_width(&it.label.replace('&', ""));
        let sc = text_width(&it.shortcut);
        tw = tw.max(label + if sc > 0 { sc + 24 } else { 0 });
    }
    (tw + 32, y + 2, rows)
}

/// The menu with a 1-px drop shadow (right and bottom), selection `sel`.
pub fn draw_menu(items: &[MenuItem], sel: Option<usize>, p: i32) -> Canvas {
    let (w, h, rows) = menu_layout(items);
    let mut c = Canvas::new(((w + 1) * p) as u32, ((h + 1) * p) as u32, CLEAR_PX);
    let mut sf = c.surface();
    sf.fill_rect(p, p, (w * p) as u32, (h * p) as u32, BLACK);
    sf.fill_rect(0, 0, (w * p) as u32, (h * p) as u32, SILVER);
    sf.frame(0, 0, (w * p) as u32, (h * p) as u32, p as u32, BLACK);
    let font = lantern_12();
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
        let ty = (y + TEXT_Y) * p;
        if it.checked {
            sf.text(font, 4 * p, ty, p as u32, fg, None, "✓");
        }
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
                // Under the baseline, the width of the letter's ink.
                sf.fill_rect(
                    x,
                    ty + 11 * p,
                    ((g.width as i32 - 1) * p) as u32,
                    p as u32,
                    fg,
                );
                underline = false;
            }
            x += g.width as i32 * p;
        }
        if !it.shortcut.is_empty() {
            let sx = (w - 12 - text_width(&it.shortcut)) * p;
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

/// A bevelled face (1 px White top-left, Slate bottom-right; inverted when
/// pressed), `w`×`h` art pixels at (x, y) art pixels.
pub fn bevel(sf: &mut Surface, x: i32, y: i32, w: i32, h: i32, p: i32, pressed: bool) {
    let (hi, lo) = if pressed {
        (SLATE, WHITE)
    } else {
        (WHITE, SLATE)
    };
    sf.fill_rect(x * p, y * p, (w * p) as u32, (h * p) as u32, SILVER);
    sf.fill_rect(x * p, y * p, (w * p) as u32, p as u32, hi);
    sf.fill_rect(x * p, y * p, p as u32, (h * p) as u32, hi);
    sf.fill_rect(x * p, (y + h - 1) * p, (w * p) as u32, p as u32, lo);
    sf.fill_rect((x + w - 1) * p, y * p, p as u32, (h * p) as u32, lo);
}

/// A push button with a Black outline (two lines for the default button)
/// and a centred Lantern 12 label.
pub fn push_button(
    sf: &mut Surface,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    p: i32,
    label: &str,
    default: bool,
    pressed: bool,
) {
    let ring = if default { 2 } else { 1 };
    for i in 0..ring {
        sf.frame(
            (x + i) * p,
            (y + i) * p,
            ((w - 2 * i) * p) as u32,
            ((h - 2 * i) * p) as u32,
            p as u32,
            BLACK,
        );
    }
    bevel(
        sf,
        x + ring,
        y + ring,
        w - 2 * ring,
        h - 2 * ring,
        p,
        pressed,
    );
    let off = if pressed { 1 } else { 0 };
    let tx = x + (w - text_width(label)) / 2 + off;
    let ty = y + (h - 13) / 2 + off;
    sf.text(lantern_12(), tx * p, ty * p, p as u32, BLACK, None, label);
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(w >= text_width("Close") + text_width("Alt+F4") + 32);
        assert_eq!(menu_hit(&items, 20, 5), None); // disabled
        assert_eq!(menu_hit(&items, 20, 25), Some(1));
        assert_eq!(menu_hit(&items, 20, 42), None); // separator
        assert_eq!(menu_hit(&items, 20, 50), Some(3));
        let c = draw_menu(&items, Some(1), 2);
        assert_eq!(c.get(c.width - 1, 0), CLEAR_PX);
        assert_eq!(c.get(10, 22 * 2), SKY);
        let cut = ellipsize("Program Manager", 60);
        assert!(cut.ends_with('…') && text_width(&cut) <= 60, "{cut}");
        assert_eq!(ellipsize("DOS", 60), "DOS");
    }

    #[test]
    fn buttons() {
        let mut c = Canvas::new(80, 30, 0);
        push_button(&mut c.surface(), 0, 0, 40, 15, 2, "OK", true, false);
        assert_eq!(c.get(0, 0), BLACK);
        assert_eq!(c.get(2, 2), BLACK); // the default button's second ring
        assert_eq!(c.get(4, 4), WHITE); // bevel highlight
    }
}

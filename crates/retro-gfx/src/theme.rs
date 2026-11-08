//! The look of the GUI in each colour scheme (docs/STYLE.md §2, "GUI"
//! column): one scheme drives the text screens and the Desktop alike.

use crate::palette::{Scheme, BLACK, COBALT, LEMON, LIME, SILVER, SKY, SLATE, WHITE};
use crate::Rgb;

/// Colours of window frames, menus, buttons, the desktop and the System Bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Theme {
    /// The scheme's short name (classic, paper, …).
    pub key: &'static str,
    /// Window faces, menus, buttons, and the text on them.
    pub face: Rgb,
    pub text: Rgb,
    pub disabled: Rgb,
    /// Outlines, separators, the line under a title bar.
    pub line: Rgb,
    /// Bevels: top-left and bottom-right.
    pub light: Rgb,
    pub dark: Rgb,
    /// Inside text fields, lists, check boxes and work areas; `text` on it.
    /// (Classic and Paper: white; the phosphor schemes and High Contrast:
    /// black, so the text stays readable — `light` there is as bright as
    /// the text.)
    pub well: Rgb,
    /// Title bars: active and inactive, background and text.
    pub active: Rgb,
    pub active_text: Rgb,
    pub inactive: Rgb,
    pub inactive_text: Rgb,
    /// Selected menu items and icons.
    pub sel: Rgb,
    pub sel_text: Rgb,
    /// The two colours of the desktop dither.
    pub desktop: (Rgb, Rgb),
    /// The System Bar.
    pub bar: Rgb,
    pub bar_text: Rgb,
    /// Labels under desktop icons (when not selected).
    pub label: Rgb,
    pub label_text: Rgb,
    /// Monochrome schemes draw pictures (icons) in these four steps, dark
    /// to bright; empty for Classic, which keeps their colours.
    pub ramp: Option<[Rgb; 4]>,
}

impl Theme {
    pub fn for_scheme(s: &Scheme) -> Theme {
        match s.key() {
            // Black and white, a 50 % grey desktop (System 6).
            "paper" => Theme {
                key: "paper",
                face: WHITE,
                text: BLACK,
                disabled: SLATE,
                line: BLACK,
                light: WHITE,
                dark: BLACK,
                well: WHITE,
                active: BLACK,
                active_text: WHITE,
                inactive: WHITE,
                inactive_text: SLATE,
                sel: BLACK,
                sel_text: WHITE,
                desktop: (WHITE, BLACK),
                bar: WHITE,
                bar_text: BLACK,
                label: BLACK,
                label_text: WHITE,
                ramp: Some([BLACK, SLATE, WHITE, WHITE]),
            },
            // Monochrome phosphor: the scheme's ramp on black.
            "amber" | "green" => {
                let (dark, mid, bright, black) = match s.key() {
                    "amber" => (0x5A3A00, 0xC88A00, 0xFFB000, 0x0A0702),
                    _ => (0x1F5A1D, 0x4FB04A, LIME, 0x050A05),
                };
                Theme {
                    key: s.key(),
                    face: black,
                    text: bright,
                    disabled: dark,
                    line: mid,
                    light: bright,
                    dark,
                    well: black,
                    active: bright,
                    active_text: black,
                    inactive: black,
                    inactive_text: mid,
                    sel: mid,
                    sel_text: black,
                    desktop: (dark, black),
                    bar: black,
                    bar_text: bright,
                    label: black,
                    label_text: bright,
                    ramp: Some([black, dark, mid, bright]),
                }
            }
            // Black, White and Lemon only.
            "high" => Theme {
                key: "high",
                face: 0x000000,
                text: WHITE,
                disabled: SLATE,
                line: WHITE,
                light: WHITE,
                dark: WHITE,
                well: 0x000000,
                active: LEMON,
                active_text: 0x000000,
                inactive: 0x000000,
                inactive_text: WHITE,
                sel: LEMON,
                sel_text: 0x000000,
                desktop: (0x000000, 0x000000),
                bar: 0x000000,
                bar_text: WHITE,
                label: 0x000000,
                label_text: WHITE,
                ramp: Some([0x000000, 0x000000, WHITE, WHITE]),
            },
            // Phosphor Classic: Silver faces, Cobalt titles, Sky selection.
            _ => Theme {
                key: "classic",
                face: SILVER,
                text: BLACK,
                disabled: SLATE,
                line: BLACK,
                light: WHITE,
                dark: SLATE,
                well: WHITE,
                active: COBALT,
                active_text: WHITE,
                inactive: SILVER,
                inactive_text: SLATE,
                sel: SKY,
                sel_text: WHITE,
                desktop: (COBALT, BLACK),
                bar: WHITE,
                bar_text: BLACK,
                label: BLACK,
                label_text: WHITE,
                ramp: None,
            },
        }
    }

    /// A picture's colour in this theme: unchanged in Classic, else the
    /// ramp step for its brightness.
    pub fn picture(&self, c: Rgb) -> Rgb {
        let Some(r) = self.ramp else { return c };
        let (red, g, b) = ((c >> 16) & 0xFF, (c >> 8) & 0xFF, c & 0xFF);
        let l = (red * 299 + g * 587 + b * 114) / 1000;
        r[((l * 4) / 256).min(3) as usize]
    }

    /// The system's theme (from [`Scheme::system`]).
    pub fn system() -> Theme {
        Theme::for_scheme(Scheme::system())
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme::for_scheme(&Scheme::CLASSIC)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scheme_has_a_readable_theme() {
        for s in Scheme::ALL {
            let t = Theme::for_scheme(s);
            assert_eq!(t.key, s.key());
            assert_ne!(t.face, t.text, "{}", s.name);
            assert_ne!(t.active, t.active_text, "{}", s.name);
            assert_ne!(t.sel, t.sel_text, "{}", s.name);
            assert_ne!(t.bar, t.bar_text, "{}", s.name);
            assert_eq!(Scheme::by_key(s.key()).map(|x| x.name), Some(s.name));
        }
        assert_eq!(Theme::default().active, COBALT);
        assert_eq!(Theme::default().picture(SKY), SKY);
        let amber = Theme::for_scheme(&Scheme::AMBER);
        assert_eq!(amber.picture(WHITE), 0xFFB000);
        assert_eq!(amber.picture(BLACK), 0x0A0702);
    }
}

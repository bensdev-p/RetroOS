//! The Phosphor16 palette and colour schemes (see docs/STYLE.md §2).

/// A colour packed as 0x00RRGGBB (the XRGB8888 layout KMS and wl_shm use).
pub type Rgb = u32;

pub const BLACK: Rgb = 0x0A0A0F;
pub const COBALT: Rgb = 0x1F3BB3;
pub const FERN: Rgb = 0x1E9A3C;
pub const TEAL: Rgb = 0x1A9FA0;
pub const BRICK: Rgb = 0xB2302B;
pub const PLUM: Rgb = 0x8E3A9E;
pub const OCHRE: Rgb = 0xB07A1E;
pub const SILVER: Rgb = 0xBDBDB8;
pub const SLATE: Rgb = 0x5C5C66;
pub const SKY: Rgb = 0x5C7CFA;
pub const LIME: Rgb = 0x6FE06A;
pub const AQUA: Rgb = 0x5FE3E3;
pub const CORAL: Rgb = 0xFF6A5C;
pub const ORCHID: Rgb = 0xE27BF0;
pub const LEMON: Rgb = 0xF7E15A;
pub const WHITE: Rgb = 0xF5F5F0;

/// Phosphor16 in the classic IRGB index order used by text attributes.
pub const PHOSPHOR16: [Rgb; 16] = [
    BLACK, COBALT, FERN, TEAL, BRICK, PLUM, OCHRE, SILVER, SLATE, SKY, LIME, AQUA, CORAL, ORCHID,
    LEMON, WHITE,
];

pub const NAMES: [&str; 16] = [
    "Black", "Cobalt", "Fern", "Teal", "Brick", "Plum", "Ochre", "Silver", "Slate", "Sky", "Lime",
    "Aqua", "Coral", "Orchid", "Lemon", "White",
];

/// A text-mode colour scheme: a 16-entry palette plus default attributes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scheme {
    pub name: &'static str,
    pub palette: [Rgb; 16],
    /// Default foreground / background palette indices.
    pub fg: u8,
    pub bg: u8,
    /// Colour of the overscan border around a letterboxed text screen.
    pub border: u8,
    /// Highlight (mnemonic / hotkey) index.
    pub hot: u8,
    /// Selection background index.
    pub sel: u8,
}

const fn ramp(dark: Rgb, mid: Rgb, bright: Rgb, black: Rgb) -> [Rgb; 16] {
    // Monochrome schemes map every colour onto a 4-step ramp by intensity.
    [
        black, mid, mid, mid, mid, mid, mid, mid, dark, bright, bright, bright, bright, bright,
        bright, bright,
    ]
}

impl Scheme {
    pub const CLASSIC: Scheme = Scheme {
        name: "Phosphor Classic",
        palette: PHOSPHOR16,
        fg: 7,
        bg: 1,
        border: 1,
        hot: 14,
        sel: 3,
    };
    pub const PAPER: Scheme = Scheme {
        name: "Paper",
        palette: [
            WHITE, BLACK, BLACK, BLACK, BLACK, BLACK, BLACK, BLACK, SLATE, BLACK, BLACK, BLACK,
            BLACK, BLACK, BLACK, BLACK,
        ],
        fg: 1,
        bg: 0,
        border: 0,
        hot: 8,
        sel: 1,
    };
    pub const AMBER: Scheme = Scheme {
        name: "Amber",
        palette: ramp(0x5A3A00, 0xC88A00, 0xFFB000, 0x0A0702),
        fg: 7,
        bg: 0,
        border: 0,
        hot: 15,
        sel: 8,
    };
    pub const GREEN: Scheme = Scheme {
        name: "Green Screen",
        palette: ramp(0x1F5A1D, 0x4FB04A, LIME, 0x050A05),
        fg: 7,
        bg: 0,
        border: 0,
        hot: 15,
        sel: 8,
    };
    pub const HIGH_CONTRAST: Scheme = Scheme {
        name: "High Contrast",
        palette: [
            0x000000, 0x000000, WHITE, WHITE, WHITE, WHITE, WHITE, WHITE, WHITE, LEMON, LEMON,
            LEMON, LEMON, LEMON, LEMON, 0xFFFFFF,
        ],
        fg: 15,
        bg: 0,
        border: 0,
        hot: 14,
        sel: 9,
    };

    pub const ALL: [&'static Scheme; 5] = [
        &Scheme::CLASSIC,
        &Scheme::PAPER,
        &Scheme::AMBER,
        &Scheme::GREEN,
        &Scheme::HIGH_CONTRAST,
    ];

    pub fn by_name(name: &str) -> Option<&'static Scheme> {
        let key = name.trim().to_ascii_lowercase();
        Scheme::ALL.into_iter().find(|s| {
            let n = s.name.to_ascii_lowercase();
            n == key || n.split_whitespace().next() == Some(key.as_str())
        })
    }

    pub fn color(&self, idx: u8) -> Rgb {
        self.palette[(idx & 15) as usize]
    }
}

/// Nearest Phosphor16 index for an arbitrary colour (used to quantise
/// truecolour/256-colour terminal output in "Authentic" mode).
pub fn nearest(rgb: Rgb) -> u8 {
    let (r, g, b) = split(rgb);
    let mut best = (u32::MAX, 0u8);
    for (i, &c) in PHOSPHOR16.iter().enumerate() {
        let (cr, cg, cb) = split(c);
        // Weighted Euclidean distance ("redmean" approximation).
        let rm = (r + cr) / 2;
        let dr = r.abs_diff(cr);
        let dg = g.abs_diff(cg);
        let db = b.abs_diff(cb);
        let d = (((512 + rm) * dr * dr) >> 8) + 4 * dg * dg + (((767 - rm) * db * db) >> 8);
        if d < best.0 {
            best = (d, i as u8);
        }
    }
    best.1
}

pub const fn split(rgb: Rgb) -> (u32, u32, u32) {
    ((rgb >> 16) & 0xFF, (rgb >> 8) & 0xFF, rgb & 0xFF)
}

pub const fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_is_identity_on_palette() {
        for (i, &c) in PHOSPHOR16.iter().enumerate() {
            assert_eq!(nearest(c) as usize, i);
        }
    }

    #[test]
    fn scheme_lookup() {
        assert_eq!(Scheme::by_name("amber").unwrap().name, "Amber");
        assert_eq!(Scheme::by_name("Phosphor Classic").unwrap().fg, 7);
        assert_eq!(Scheme::by_name("green").unwrap().name, "Green Screen");
        assert!(Scheme::by_name("plaid").is_none());
    }
}

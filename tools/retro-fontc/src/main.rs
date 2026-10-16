//! retro-fontc — build-time font tool.
//!
//!   retro-fontc sheet <out.png> [scale]   glyph sheet of every own glyph
//!   retro-fontc psf <out.psf>             PSF2 console font (CP437 order)
//!   retro-fontc sample <out.png> <text>   render a text sample (Classic scheme)

use retro_gfx::font::{phosphor_mono, CP437};
use retro_gfx::palette::{self, Scheme};
use retro_gfx::Canvas;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let font = phosphor_mono();
    let res = match args.get(1).map(String::as_str) {
        Some("sheet") if args.len() >= 3 => {
            let scale = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(2u32);
            sheet(&args[2], scale)
        }
        Some("psf") if args.len() == 3 => std::fs::write(&args[2], font.to_psf2(&CP437)),
        Some("sample") if args.len() >= 4 => sample(&args[2], &args[3..].join(" ")),
        _ => {
            eprintln!("usage: retro-fontc sheet <out.png> [scale] | psf <out.psf> | sample <out.png> <text>");
            std::process::exit(2);
        }
    };
    if let Err(e) = res {
        eprintln!("retro-fontc: {e}");
        std::process::exit(1);
    }
}

fn sheet(out: &str, scale: u32) -> std::io::Result<()> {
    let font = phosphor_mono();
    let chars = font.chars();
    let cols = 32u32;
    let rows = (chars.len() as u32).div_ceil(cols);
    let (cw, ch) = (font.cell_w as u32 + 2, font.cell_h as u32 + 2);
    let mut c = Canvas::new(cols * cw * scale, rows * ch * scale, palette::SLATE);
    let mut s = c.surface();
    for (i, &chr) in chars.iter().enumerate() {
        let (x, y) = ((i as u32 % cols) * cw, (i as u32 / cols) * ch);
        s.fill_rect(
            ((x + 1) * scale) as i32,
            ((y + 1) * scale) as i32,
            8 * scale,
            16 * scale,
            palette::COBALT,
        );
        s.glyph(
            font.glyph_or_replacement(chr),
            ((x + 1) * scale) as i32,
            ((y + 1) * scale) as i32,
            scale,
            palette::WHITE,
            None,
        );
    }
    c.save_png(out)?;
    eprintln!("{} glyphs -> {out}", chars.len());
    Ok(())
}

fn sample(out: &str, text: &str) -> std::io::Result<()> {
    let font = phosphor_mono();
    let sch = &Scheme::CLASSIC;
    let lines: Vec<&str> = text.split("\\n").collect();
    let w = lines.iter().map(|l| l.chars().count()).max().unwrap_or(1) as u32 + 2;
    let h = lines.len() as u32 + 2;
    let scale = 2;
    let mut c = Canvas::new(w * 8 * scale, h * 16 * scale, sch.color(sch.bg));
    let mut s = c.surface();
    for (i, l) in lines.iter().enumerate() {
        s.text(
            font,
            (8 * scale) as i32,
            ((i as u32 + 1) * 16 * scale) as i32,
            scale,
            sch.color(sch.fg),
            None,
            l,
        );
    }
    c.save_png(out)
}

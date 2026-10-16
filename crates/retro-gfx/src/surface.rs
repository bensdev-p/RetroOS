//! XRGB8888 drawing surfaces.

use crate::font::{Font, Glyph};
use crate::palette::Rgb;

/// A borrowed XRGB8888 pixel buffer (a KMS dumb buffer, a wl_shm pool, or a
/// [`Canvas`]). `stride` is in pixels.
pub struct Surface<'a> {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub px: &'a mut [u32],
}

/// An owned surface, used for off-screen rendering and golden-image tests.
#[derive(Clone)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub px: Vec<u32>,
}

impl Canvas {
    pub fn new(width: u32, height: u32, fill: Rgb) -> Canvas {
        Canvas {
            width,
            height,
            px: vec![fill; (width * height) as usize],
        }
    }

    pub fn surface(&mut self) -> Surface<'_> {
        Surface {
            width: self.width,
            height: self.height,
            stride: self.width,
            px: &mut self.px,
        }
    }

    pub fn get(&self, x: u32, y: u32) -> Rgb {
        self.px[(y * self.width + x) as usize]
    }

    /// Packed 8-bit RGB triples, row-major.
    pub fn to_rgb8(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.px.len() * 3);
        for &p in &self.px {
            out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
        }
        out
    }

    #[cfg(feature = "png")]
    pub fn write_png<W: std::io::Write>(&self, w: W) -> std::io::Result<()> {
        let mut enc = png::Encoder::new(w, self.width, self.height);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().map_err(std::io::Error::other)?;
        wr.write_image_data(&self.to_rgb8())
            .map_err(std::io::Error::other)
    }

    #[cfg(feature = "png")]
    pub fn save_png(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        let f = std::io::BufWriter::new(std::fs::File::create(path)?);
        self.write_png(f)
    }

    #[cfg(feature = "png")]
    pub fn load_png(path: impl AsRef<std::path::Path>) -> std::io::Result<Canvas> {
        let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path)?));
        let mut rd = dec.read_info().map_err(std::io::Error::other)?;
        let mut buf = vec![0; rd.output_buffer_size()];
        let info = rd.next_frame(&mut buf).map_err(std::io::Error::other)?;
        let ch = match info.color_type {
            png::ColorType::Rgb => 3,
            png::ColorType::Rgba => 4,
            other => return Err(std::io::Error::other(format!("unsupported PNG {other:?}"))),
        };
        let px = buf[..info.buffer_size()]
            .chunks(ch)
            .map(|c| ((c[0] as u32) << 16) | ((c[1] as u32) << 8) | c[2] as u32)
            .collect();
        Ok(Canvas {
            width: info.width,
            height: info.height,
            px,
        })
    }
}

impl Surface<'_> {
    #[inline]
    pub fn put(&mut self, x: i32, y: i32, c: Rgb) {
        if x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height {
            self.px[(y as u32 * self.stride + x as u32) as usize] = c;
        }
    }

    pub fn clear(&mut self, c: Rgb) {
        self.fill_rect(0, 0, self.width, self.height, c);
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: u32, h: u32, c: Rgb) {
        let x0 = x.max(0) as u32;
        let y0 = y.max(0) as u32;
        let x1 = ((x as i64 + w as i64).max(0) as u32).min(self.width);
        let y1 = ((y as i64 + h as i64).max(0) as u32).min(self.height);
        if x0 >= x1 {
            return;
        }
        for yy in y0..y1 {
            let row = (yy * self.stride) as usize;
            self.px[row + x0 as usize..row + x1 as usize].fill(c);
        }
    }

    pub fn hline(&mut self, x: i32, y: i32, w: u32, c: Rgb) {
        self.fill_rect(x, y, w, 1, c);
    }

    pub fn vline(&mut self, x: i32, y: i32, h: u32, c: Rgb) {
        self.fill_rect(x, y, 1, h, c);
    }

    /// 1-px rectangle outline, scaled by `t` (line thickness).
    pub fn frame(&mut self, x: i32, y: i32, w: u32, h: u32, t: u32, c: Rgb) {
        self.fill_rect(x, y, w, t, c);
        self.fill_rect(x, y + h as i32 - t as i32, w, t, c);
        self.fill_rect(x, y, t, h, c);
        self.fill_rect(x + w as i32 - t as i32, y, t, h, c);
    }

    /// Draw one glyph at an integer `scale`. `bg = None` leaves paper pixels untouched.
    pub fn glyph(&mut self, g: &Glyph, x: i32, y: i32, scale: u32, fg: Rgb, bg: Option<Rgb>) {
        let s = scale as i32;
        for (ry, &bits) in g.rows.iter().enumerate().take(g.height as usize) {
            for rx in 0..g.width as i32 {
                let on = bits & (0x8000 >> rx) != 0;
                let c = if on { Some(fg) } else { bg };
                if let Some(c) = c {
                    self.fill_rect(x + rx * s, y + ry as i32 * s, scale, scale, c);
                }
            }
        }
    }

    /// Draw a string left-to-right in cells; returns the x after the last cell.
    /// Wide (16-px) fallback glyphs take two cells.
    pub fn text(
        &mut self,
        font: &Font,
        x: i32,
        y: i32,
        scale: u32,
        fg: Rgb,
        bg: Option<Rgb>,
        s: &str,
    ) -> i32 {
        let mut cx = x;
        for ch in s.chars() {
            let g = font.glyph_or_replacement(ch);
            self.glyph(g, cx, y, scale, fg, bg);
            cx += (g.width as u32 * scale) as i32;
        }
        cx
    }

    /// Copy a rectangle from another buffer (same pixel format).
    pub fn blit(&mut self, src: &Canvas, dx: i32, dy: i32) {
        for sy in 0..src.height as i32 {
            let y = dy + sy;
            if y < 0 || y as u32 >= self.height {
                continue;
            }
            for sx in 0..src.width as i32 {
                self.put(
                    dx + sx,
                    y,
                    src.px[(sy as u32 * src.width + sx as u32) as usize],
                );
            }
        }
    }
}

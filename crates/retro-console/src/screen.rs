//! The terminal screen: alacritty_terminal state, rendered cell by cell in
//! Phosphor Mono with the Phosphor16 palette. Only changed cells are redrawn.

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Processor, StdSyncHandler};
use retro_gfx::font::{build_phosphor_mono, Font};
use retro_gfx::grid::fit_scale;
use retro_gfx::palette::{self, Scheme};
use retro_gfx::Canvas;
use std::cell::RefCell;
use std::rc::Rc;

/// Messages from the terminal state machine to the app.
#[derive(Default)]
pub struct Outbox {
    pub pty_write: Vec<u8>,
    pub title: Option<String>,
    pub bell: bool,
}

#[derive(Clone)]
pub struct Listener(pub Rc<RefCell<Outbox>>);

impl EventListener for Listener {
    fn send_event(&self, ev: Event) {
        let mut o = self.0.borrow_mut();
        match ev {
            Event::PtyWrite(s) => o.pty_write.extend_from_slice(s.as_bytes()),
            Event::Title(t) => o.title = Some(t),
            Event::ResetTitle => o.title = Some(String::new()),
            Event::Bell => o.bell = true,
            _ => {}
        }
    }
}

struct Size {
    cols: usize,
    lines: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.lines
    }
    fn screen_lines(&self) -> usize {
        self.lines
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

/// ANSI colour number → Phosphor16 (IRGB) index.
const ANSI_TO_DOS: [u8; 16] = [0, 4, 2, 6, 1, 5, 3, 7, 8, 12, 10, 14, 9, 13, 11, 15];

/// Splits RetroOS private OSC 7701 commands out of the byte stream.
#[derive(Default)]
struct OscFilter {
    state: u8, // 0 text, 1 after ESC, 2 inside OSC, 3 inside OSC after ESC
    payload: Vec<u8>,
}

impl OscFilter {
    fn filter(&mut self, input: &[u8], out: &mut Vec<u8>, cmds: &mut Vec<String>) {
        for &b in input {
            match self.state {
                0 => {
                    if b == 0x1b {
                        self.state = 1;
                    } else {
                        out.push(b);
                    }
                }
                1 => {
                    if b == b']' {
                        self.state = 2;
                        self.payload.clear();
                    } else {
                        out.push(0x1b);
                        if b == 0x1b {
                            continue; // stay in state 1
                        }
                        out.push(b);
                        self.state = 0;
                    }
                }
                2 | 3 => {
                    let end = b == 0x07 || (self.state == 3 && b == b'\\');
                    if end {
                        if let Some(cmd) = self.payload.strip_prefix(b"7701;") {
                            cmds.push(String::from_utf8_lossy(cmd).into_owned());
                        } else {
                            out.extend_from_slice(b"\x1b]");
                            out.extend_from_slice(&self.payload);
                            out.extend_from_slice(if b == 0x07 { b"\x07" } else { b"\x1b\\" });
                        }
                        self.state = 0;
                    } else if self.state == 2 && b == 0x1b {
                        self.state = 3;
                    } else {
                        if self.state == 3 {
                            self.payload.push(0x1b);
                            self.state = 2;
                        }
                        self.payload.push(b);
                        if self.payload.len() > 4096 {
                            out.extend_from_slice(b"\x1b]");
                            out.extend_from_slice(&self.payload);
                            self.payload.clear();
                            self.state = 0;
                        }
                    }
                }
                _ => self.state = 0,
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Drawn {
    ch: char,
    fg: u32,
    bg: u32,
    deco: u8,
}

pub struct Screen {
    pub term: Term<Listener>,
    parser: Processor<StdSyncHandler>,
    osc: OscFilter,
    font: Font,
    fallback_loaded: bool,
    pub scheme: Scheme,
    /// COLOR XY override of default (bg, fg).
    attr: Option<(u8, u8)>,
    pub cols: usize,
    pub rows: usize,
    pub font_scale: u32,
    pub canvas: Canvas,
    origin: (i32, i32),
    drawn: Vec<Option<Drawn>>,
    pub cursor_on: bool,
    pub mouse_cell: Option<(usize, usize)>,
    pub outbox: Rc<RefCell<Outbox>>,
    full_redraw: bool,
    /// MODE CON COLS= LINES=: a fixed grid, letterboxed (None = fill).
    fixed: Option<(usize, usize)>,
    /// Draw at 1× into a small canvas that the compositor enlarges by `zoom`
    /// (wp_viewporter, nearest-neighbour): a quarter of the memory at 1080p.
    compact: bool,
    /// How much the canvas is enlarged on screen (1 when not compact).
    pub zoom: u32,
    /// The physical size the screen fills.
    pub phys: (u32, u32),
    /// In a Desktop window: keep the text size when the window is resized
    /// (instead of re-picking the largest scale that fits 80×25).
    pub keep_scale: bool,
}

impl Screen {
    #[cfg(test)]
    pub fn new(phys_w: u32, phys_h: u32) -> Screen {
        Screen::with_mode(phys_w, phys_h, false)
    }

    pub fn with_mode(phys_w: u32, phys_h: u32, compact: bool) -> Screen {
        let outbox = Rc::new(RefCell::new(Outbox::default()));
        let (cols, rows, fs, origin) = geometry(phys_w, phys_h, None);
        let term = Term::new(
            Config::default(),
            &Size { cols, lines: rows },
            Listener(outbox.clone()),
        );
        let scheme = Scheme::CLASSIC;
        Screen {
            term,
            parser: Processor::new(),
            osc: OscFilter::default(),
            font: build_phosphor_mono().expect("font"),
            fallback_loaded: false,
            canvas: Canvas::new(phys_w.max(1), phys_h.max(1), scheme.color(scheme.border)),
            scheme,
            attr: None,
            cols,
            rows,
            font_scale: fs,
            origin,
            drawn: vec![None; cols * rows],
            cursor_on: true,
            mouse_cell: None,
            outbox,
            full_redraw: true,
            fixed: None,
            compact,
            zoom: 1,
            phys: (phys_w, phys_h),
            keep_scale: false,
        }
        .laid_out()
    }

    fn laid_out(mut self) -> Screen {
        let (w, h) = self.phys;
        self.relayout(w, h);
        self
    }

    pub fn resize(&mut self, phys_w: u32, phys_h: u32) -> bool {
        if (phys_w, phys_h) == self.phys {
            return false;
        }
        self.relayout(phys_w, phys_h);
        true
    }

    fn relayout(&mut self, phys_w: u32, phys_h: u32) {
        let keep = (self.keep_scale && self.fixed.is_none()).then_some(self.zoom * self.font_scale);
        let (cols, rows, fs, origin) = match keep {
            Some(fs) if phys_w / (8 * fs) >= 20 && phys_h / (16 * fs) >= 5 => {
                let (c, r) = ((phys_w / (8 * fs)) as usize, (phys_h / (16 * fs)) as usize);
                let ox = ((phys_w - c as u32 * 8 * fs) / 2) as i32;
                let oy = ((phys_h - r as u32 * 16 * fs) / 2) as i32;
                (c, r, fs, (ox, oy))
            }
            _ => geometry(phys_w, phys_h, self.fixed),
        };
        self.phys = (phys_w, phys_h);
        self.cols = cols;
        self.rows = rows;
        self.term.resize(Size { cols, lines: rows });
        let (zoom, w, h) = if self.compact {
            (fs, phys_w.div_ceil(fs), phys_h.div_ceil(fs))
        } else {
            (1, phys_w, phys_h)
        };
        self.zoom = zoom;
        self.font_scale = fs / zoom;
        self.origin = (
            (origin.0 + zoom as i32 / 2) / zoom as i32,
            (origin.1 + zoom as i32 / 2) / zoom as i32,
        );
        self.canvas = Canvas::new(w.max(1), h.max(1), 0);
        self.drawn = vec![None; cols * rows];
        self.full_redraw = true;
    }

    /// Feed shell output. Returns true if RetroOS commands changed colours.
    pub fn feed(&mut self, bytes: &[u8]) {
        let mut out = Vec::with_capacity(bytes.len());
        let mut cmds = Vec::new();
        self.osc.filter(bytes, &mut out, &mut cmds);
        self.parser.advance(&mut self.term, &out);
        for c in cmds {
            self.retro_command(&c);
        }
    }

    fn retro_command(&mut self, cmd: &str) {
        let (k, v) = cmd.split_once('=').unwrap_or((cmd, ""));
        match k {
            "scheme" => {
                self.scheme = match v {
                    "paper" => Scheme::PAPER,
                    "amber" => Scheme::AMBER,
                    "green" => Scheme::GREEN,
                    "high" => Scheme::HIGH_CONTRAST,
                    _ => Scheme::CLASSIC,
                };
                self.attr = None;
            }
            "grid" => {
                self.fixed = v.split_once('x').and_then(|(c, r)| {
                    let (c, r) = (c.parse::<usize>().ok()?, r.parse::<usize>().ok()?);
                    (c >= 20 && r >= 5).then_some((c, r))
                });
                let (w, h) = self.phys;
                self.relayout(w, h);
            }
            "attr" if v == "default" => self.attr = None,
            "attr" => {
                if let Ok(x) = u8::from_str_radix(v, 16) {
                    self.attr = Some((x >> 4, x & 15));
                }
            }
            _ => {}
        }
        self.full_redraw = true;
    }

    pub fn scroll(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
    }

    pub fn mode(&self) -> TermMode {
        *self.term.mode()
    }

    fn default_fg(&self) -> u8 {
        self.attr.map(|a| a.1).unwrap_or(self.scheme.fg)
    }

    fn default_bg(&self) -> u8 {
        self.attr.map(|a| a.0).unwrap_or(self.scheme.bg)
    }

    fn index(&self, c: Color, fg: bool, bold: bool) -> u8 {
        let brighten = |i: u8| if fg && bold && i < 8 { i + 8 } else { i };
        match c {
            Color::Named(n) => match n {
                NamedColor::Foreground | NamedColor::Cursor | NamedColor::DimForeground => {
                    let d = self.default_fg();
                    if bold && d < 8 {
                        d + 8
                    } else {
                        d
                    }
                }
                NamedColor::BrightForeground => self.default_fg() | 8,
                NamedColor::Background => self.default_bg(),
                n => {
                    let i = n as usize;
                    if i < 16 {
                        ANSI_TO_DOS[brighten(i as u8) as usize]
                    } else if (NamedColor::DimBlack as usize..=NamedColor::DimWhite as usize)
                        .contains(&i)
                    {
                        ANSI_TO_DOS[i - NamedColor::DimBlack as usize]
                    } else {
                        self.default_fg()
                    }
                }
            },
            Color::Indexed(i) if i < 16 => ANSI_TO_DOS[brighten(i) as usize],
            Color::Indexed(i) => palette::nearest(xterm256(i)),
            Color::Spec(rgb) => palette::nearest(palette::rgb(rgb.r, rgb.g, rgb.b)),
        }
    }

    /// Redraw changed cells into the canvas. Returns true if anything changed.
    pub fn render(&mut self) -> bool {
        let fs = self.font_scale;
        let (cw, ch) = (8 * fs, 16 * fs);
        if self.full_redraw {
            let border = self.scheme.color(self.scheme.border);
            let bg = self.scheme.color(self.default_bg());
            let mut s = self.canvas.surface();
            s.clear(border);
            // The text area itself is the default background (letterboxed by the border).
            s.fill_rect(
                self.origin.0,
                self.origin.1,
                self.cols as u32 * cw,
                self.rows as u32 * ch,
                bg,
            );
            self.drawn.iter_mut().for_each(|d| *d = None);
            self.full_redraw = false;
        }
        let content = self.term.renderable_content();
        let cursor = content.cursor;
        let display_offset = content.display_offset as i32;
        let mut cells: Vec<(usize, usize, char, Color, Color, Flags)> =
            Vec::with_capacity(self.cols * self.rows);
        for ic in content.display_iter {
            let line = ic.point.line.0 + display_offset;
            if line < 0 || line as usize >= self.rows {
                continue;
            }
            let col = ic.point.column.0;
            if col >= self.cols {
                continue;
            }
            cells.push((
                line as usize,
                col,
                ic.cell.c,
                ic.cell.fg,
                ic.cell.bg,
                ic.cell.flags,
            ));
        }
        let cursor_pos = (cursor.point.line.0 + display_offset, cursor.point.column.0);
        let cursor_shape = cursor.shape;
        let mut changed = false;
        for (line, col, c, fg, bg, flags) in cells {
            let bold = flags.contains(Flags::BOLD);
            let mut fgi = self.index(fg, true, bold);
            let mut bgi = self.index(bg, false, false);
            if flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fgi, &mut bgi);
            }
            let mut fgc = self.scheme.color(fgi);
            let mut bgc = self.scheme.color(bgi);
            let mut deco = 0u8;
            if flags.intersects(Flags::UNDERLINE | Flags::DOUBLE_UNDERLINE | Flags::UNDERCURL) {
                deco |= 1;
            }
            if flags.contains(Flags::STRIKEOUT) {
                deco |= 2;
            }
            let here = cursor_pos == (line as i32, col);
            if here && self.cursor_on {
                match cursor_shape {
                    CursorShape::Block | CursorShape::HollowBlock => {
                        std::mem::swap(&mut fgc, &mut bgc)
                    }
                    CursorShape::Underline | CursorShape::Beam => deco |= 4,
                    CursorShape::Hidden => {}
                }
            }
            if self.mouse_cell == Some((line, col)) {
                fgc ^= 0x00FF_FFFF;
                bgc ^= 0x00FF_FFFF;
            }
            let ch = if flags.contains(Flags::HIDDEN) || flags.contains(Flags::WIDE_CHAR_SPACER) {
                ' '
            } else {
                c
            };
            let d = Drawn {
                ch,
                fg: fgc,
                bg: bgc,
                deco,
            };
            let idx = line * self.cols + col;
            if self.drawn[idx] == Some(d) {
                continue;
            }
            self.drawn[idx] = Some(d);
            changed = true;
            self.draw_cell(line, col, d, flags.contains(Flags::WIDE_CHAR));
        }
        changed
    }

    fn draw_cell(&mut self, line: usize, col: usize, d: Drawn, wide: bool) {
        if !self.fallback_loaded && !self.font.has_own(d.ch) && d.ch != ' ' {
            self.fallback_loaded = true;
            self.font.load_system_fallback();
        }
        let fs = self.font_scale;
        let (cw, ch) = (8 * fs, 16 * fs);
        let x = self.origin.0 + (col as u32 * cw) as i32;
        let y = self.origin.1 + (line as u32 * ch) as i32;
        let g = *self.font.glyph_or_replacement(d.ch);
        let width = if wide || g.width > 8 { cw * 2 } else { cw };
        let mut s = self.canvas.surface();
        s.fill_rect(x, y, width, ch, d.bg);
        if d.ch != ' ' {
            s.glyph(&g, x, y, fs, d.fg, None);
        }
        if d.deco & 1 != 0 {
            s.fill_rect(x, y + 13 * fs as i32, width, fs, d.fg);
        }
        if d.deco & 2 != 0 {
            s.fill_rect(x, y + 7 * fs as i32, width, fs, d.fg);
        }
        if d.deco & 4 != 0 {
            // DOS underline cursor: the bottom two pixel rows of the cell.
            s.fill_rect(x, y + 14 * fs as i32, cw, 2 * fs, d.fg);
        }
    }

    /// Cell under a physical-pixel position.
    pub fn cell_at(&self, px: f64, py: f64) -> Option<(usize, usize)> {
        let (cw, ch) = (8.0 * self.font_scale as f64, 16.0 * self.font_scale as f64);
        let z = self.zoom as f64;
        let x = px / z - self.origin.0 as f64;
        let y = py / z - self.origin.1 as f64;
        if x < 0.0 || y < 0.0 {
            return None;
        }
        let (c, l) = ((x / cw) as usize, (y / ch) as usize);
        (c < self.cols && l < self.rows).then_some((l, c))
    }
}

/// (cols, rows, font scale, origin) for a physical size: the largest integer
/// scale that still gives at least 80×25 cells, centred with a border. A
/// fixed grid gets the largest scale it fits at (clamped to the screen).
fn geometry(w: u32, h: u32, fixed: Option<(usize, usize)>) -> (usize, usize, u32, (i32, i32)) {
    let (fs, cols, rows) = match fixed {
        Some((c, r)) => {
            let fs = fit_scale(w.max(8), h.max(16), 8, 16, c as u32, r as u32);
            let c = c.min((w / (8 * fs)).max(1) as usize);
            let r = r.min((h / (16 * fs)).max(1) as usize);
            (fs, c, r)
        }
        None => {
            let fs = fit_scale(w.max(8), h.max(16), 8, 16, 80, 25);
            (
                fs,
                (w / (8 * fs)).max(1) as usize,
                (h / (16 * fs)).max(1) as usize,
            )
        }
    };
    let ox = ((w - cols as u32 * 8 * fs) / 2) as i32;
    let oy = ((h - rows as u32 * 16 * fs) / 2) as i32;
    (cols, rows, fs, (ox, oy))
}

/// The standard xterm 256-colour palette.
fn xterm256(i: u8) -> u32 {
    if i < 16 {
        return palette::PHOSPHOR16[ANSI_TO_DOS[i as usize] as usize];
    }
    if i >= 232 {
        let v = 8 + (i as u32 - 232) * 10;
        return (v << 16) | (v << 8) | v;
    }
    let i = i as u32 - 16;
    let step = |n: u32| if n == 0 { 0 } else { 55 + n * 40 };
    (step(i / 36) << 16) | (step((i / 6) % 6) << 8) | step(i % 6)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc_filter_splits_private_commands() {
        let mut f = OscFilter::default();
        let (mut out, mut cmds) = (Vec::new(), Vec::new());
        f.filter(b"ab\x1b]7701;scheme=am", &mut out, &mut cmds);
        f.filter(b"ber\x07cd\x1b]0;title\x07\x1b[1m", &mut out, &mut cmds);
        assert_eq!(cmds, vec!["scheme=amber"]);
        assert_eq!(out, b"abcd\x1b]0;title\x07\x1b[1m");
    }

    #[test]
    fn geometry_1080p() {
        let (c, r, fs, o) = geometry(1920, 1080, None);
        assert_eq!((c, r, fs), (120, 33, 2));
        assert_eq!(o, (0, 12));
        // MODE CON COLS=80 LINES=25: 80×25 at 2×, letterboxed.
        let (c, r, fs, o) = geometry(1920, 1080, Some((80, 25)));
        assert_eq!((c, r, fs), (80, 25, 2));
        assert_eq!(o, (320, 140));
        // More than fits at 1×: clamped.
        assert_eq!(geometry(640, 480, Some((132, 60))).0, 80);
    }

    #[test]
    fn grid_command() {
        let mut s = Screen::new(1920, 1080);
        s.feed(b"\x1b]7701;grid=80x25\x07");
        assert_eq!((s.cols, s.rows), (80, 25));
        s.feed(b"\x1b]7701;grid=auto\x07");
        assert_eq!((s.cols, s.rows), (120, 33));
    }

    #[test]
    fn compact_mode_draws_at_one_x() {
        let mut s = Screen::with_mode(1920, 1080, true);
        assert_eq!((s.cols, s.rows, s.zoom, s.font_scale), (120, 33, 2, 1));
        assert_eq!((s.canvas.width, s.canvas.height), (960, 540));
        assert_eq!(s.origin, (0, 6));
        s.feed(b"C:\\>");
        assert!(s.render());
        // A click at physical (20, 45) is column 1, line 1: 8x16 cells enlarged 2x, 12 px border.
        assert_eq!(s.cell_at(20.0, 45.0), Some((1, 1)));
        s.feed(b"\x1b]7701;grid=80x25\x07");
        assert_eq!((s.cols, s.rows, s.canvas.width), (80, 25, 960));
    }

    #[test]
    fn last_column_is_drawn() {
        let mut s = Screen::new(1920, 1080);
        assert_eq!(s.cols, 120);
        let line = format!("{}/x", "a".repeat(119));
        s.feed(line.as_bytes());
        s.render();
        let (cw, ch) = (8 * s.font_scale, 16 * s.font_scale);
        let bg = s.scheme.color(s.default_bg());
        // The '/' in column 120 (index 119) has ink.
        let x0 = s.origin.0 as u32 + 119 * cw;
        let y0 = s.origin.1 as u32;
        let ink = (x0..x0 + cw)
            .flat_map(|x| (y0..y0 + ch).map(move |y| (x, y)))
            .any(|(x, y)| s.canvas.get(x, y) != bg);
        assert!(ink, "column 120 is blank");
    }

    #[test]
    fn renders_text() {
        let mut s = Screen::new(1280, 800);
        s.feed(b"C:\\>DIR\r\n\x1b[1;33mHello\x1b[0m");
        assert!(s.render());
        // Second render with nothing new draws nothing.
        assert!(!s.render());
        // 'H' (bright Lemon on Cobalt) is somewhere in row 1.
        let fs = s.font_scale as usize;
        let y0 = s.origin.1 as usize + 16 * fs;
        let found = (0..16 * fs).any(|dy| {
            (0..40 * fs).any(|x| s.canvas.get(x as u32, (y0 + dy) as u32) == palette::LEMON)
        });
        assert!(found);
    }
}

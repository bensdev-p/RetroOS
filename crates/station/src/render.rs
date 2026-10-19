//! What Station draws: in text mode, the focused task full-screen, its
//! dialogs on top, and the mouse pointer; in the Desktop, what
//! `Station::desktop_prepare` listed (desktop.rs).

use crate::desktop::{Desktop, DrawItem};
use crate::state::Personality;
use crate::tasks::Tasks;
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::memory::{
    MemoryRenderBuffer, MemoryRenderBufferRenderElement,
};
use smithay::backend::renderer::element::solid::SolidColorRenderElement;
use smithay::backend::renderer::element::surface::{
    render_elements_from_surface_tree, WaylandSurfaceRenderElement,
};
use smithay::backend::renderer::element::{
    AsRenderElements, Element, Id, Kind, RenderElement, UnderlyingStorage,
};
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::backend::renderer::utils::{CommitCounter, DamageSet, OpaqueRegions};
use smithay::backend::renderer::{
    Bind, ExportMem, ImportAll, ImportMem, Offscreen, Renderer, TextureMapping,
};
use smithay::desktop::{Space, Window};
use smithay::input::pointer::{CursorImageStatus, CursorImageSurfaceData};
use smithay::output::Output;
use smithay::reexports::pixman;
use smithay::utils::{Buffer, Logical, Physical, Point, Rectangle, Scale, Transform};
use smithay::wayland::compositor::with_states;

smithay::backend::renderer::element::render_elements! {
    pub StationElement<R> where R: ImportAll + ImportMem;
    Surface=WaylandSurfaceRenderElement<R>,
    Memory=MemoryRenderBufferRenderElement<R>,
    Tile=Tile<R>,
    Solid=SolidColorRenderElement,
}

/// A memory buffer drawn under another element Id, so one buffer can be
/// drawn several times (the desktop pattern bands) and the damage tracker
/// still tells the copies apart.
pub struct Tile<R: Renderer> {
    id: Id,
    inner: MemoryRenderBufferRenderElement<R>,
}

impl<R: Renderer + ImportMem> Element for Tile<R>
where
    R::TextureId: 'static,
{
    fn id(&self) -> &Id {
        &self.id
    }
    fn current_commit(&self) -> CommitCounter {
        self.inner.current_commit()
    }
    fn src(&self) -> Rectangle<f64, Buffer> {
        self.inner.src()
    }
    fn transform(&self) -> Transform {
        self.inner.transform()
    }
    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.inner.geometry(scale)
    }
    fn damage_since(
        &self,
        scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        self.inner.damage_since(scale, commit)
    }
    fn opaque_regions(&self, scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        self.inner.opaque_regions(scale)
    }
    fn alpha(&self) -> f32 {
        self.inner.alpha()
    }
    fn kind(&self) -> Kind {
        self.inner.kind()
    }
}

impl<R: Renderer + ImportMem> RenderElement<R> for Tile<R>
where
    R::TextureId: 'static,
{
    fn draw(
        &self,
        frame: &mut R::Frame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque_regions: &[Rectangle<i32, Physical>],
    ) -> Result<(), R::Error> {
        self.inner.draw(frame, src, dst, damage, opaque_regions)
    }
    fn underlying_storage(&self, renderer: &mut R) -> Option<UnderlyingStorage<'_>> {
        self.inner.underlying_storage(renderer)
    }
}

/// Mouse pointers Station draws itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorShape {
    Arrow,
    SizeWE,
    SizeNS,
    SizeNWSE,
    SizeNESW,
}

impl CursorShape {
    /// The sizing pointer for frame edges (chrome::LEFT…).
    pub fn for_edges(e: u8) -> CursorShape {
        use crate::chrome::{BOTTOM, LEFT, RIGHT, TOP};
        let h = e & (LEFT | RIGHT) != 0;
        let v = e & (TOP | BOTTOM) != 0;
        match (h, v) {
            (true, true) if (e & LEFT != 0) == (e & TOP != 0) => CursorShape::SizeNWSE,
            (true, true) => CursorShape::SizeNESW,
            (true, false) => CursorShape::SizeWE,
            (false, true) => CursorShape::SizeNS,
            _ => CursorShape::Arrow,
        }
    }

    /// Art rows ('X' outline, 'o' fill) and the hotspot.
    fn art(self) -> (Vec<String>, (i32, i32)) {
        if self == CursorShape::Arrow {
            return (ARROW.iter().map(|s| s.to_string()).collect(), (0, 0));
        }
        // Double arrows: a filled shape, outlined where it meets the outside.
        let (w, h): (i32, i32) = match self {
            CursorShape::SizeWE => (17, 9),
            CursorShape::SizeNS => (9, 17),
            _ => (13, 13),
        };
        let inside = |x: i32, y: i32| -> bool {
            match self {
                CursorShape::SizeWE => {
                    let d = (y - 4).abs();
                    d <= x.min(16 - x) && (d <= 1 || x <= 4 || x >= 12)
                }
                CursorShape::SizeNS => {
                    let d = (x - 4).abs();
                    d <= y.min(16 - y) && (d <= 1 || y <= 4 || y >= 12)
                }
                CursorShape::SizeNWSE => x + y <= 5 || x + y >= 19 || (x - y).abs() <= 1,
                _ => {
                    let x = 12 - x;
                    x + y <= 5 || x + y >= 19 || (x - y).abs() <= 1
                }
            }
        };
        let ok = |x: i32, y: i32| x >= 0 && y >= 0 && x < w && y < h && inside(x, y);
        let mut rows = Vec::new();
        for y in -1..=h {
            let mut r = String::new();
            for x in -1..=w {
                r.push(if ok(x, y) {
                    'o'
                } else if ok(x - 1, y) || ok(x + 1, y) || ok(x, y - 1) || ok(x, y + 1) {
                    'X'
                } else {
                    '.'
                });
            }
            rows.push(r);
        }
        (rows, (w / 2 + 1, h / 2 + 1))
    }
}

/// The RetroOS arrow (docs/STYLE.md §5): black outline, white fill.
const ARROW: [&str; 19] = [
    "X...........",
    "XX..........",
    "XoX.........",
    "XooX........",
    "XoooX.......",
    "XooooX......",
    "XoooooX.....",
    "XooooooX....",
    "XoooooooX...",
    "XooooooooX..",
    "XoooooooooX.",
    "XooooooXXXXX",
    "XoooXooX....",
    "XooXXooX....",
    "XoX..XooX...",
    "XX...XooX...",
    "X.....XooX..",
    "......XooX..",
    ".......XX...",
];

/// Pointer images pre-scaled by whole pixels (so they stay sharp).
#[derive(Default)]
pub struct CursorCache {
    key: Option<(CursorShape, i32, i32)>,
    buf: Option<MemoryRenderBuffer>,
    hotspot: (i32, i32),
}

impl CursorCache {
    /// `scale` = pixels per art pixel; `buffer_scale` = the output scale.
    /// Returns the image and its hotspot in physical pixels.
    fn get(
        &mut self,
        shape: CursorShape,
        scale: i32,
        buffer_scale: i32,
    ) -> (&MemoryRenderBuffer, (i32, i32)) {
        if self.key != Some((shape, scale, buffer_scale)) || self.buf.is_none() {
            let (art, hot) = shape.art();
            let (w, h) = (art[0].len() as i32 * scale, art.len() as i32 * scale);
            let mut px = vec![0u8; (w * h * 4) as usize];
            for (y, row) in art.iter().enumerate() {
                for (x, c) in row.chars().enumerate() {
                    let argb: u32 = match c {
                        'X' => 0xFF0A0A0F,
                        'o' => 0xFFF5F5F0,
                        _ => 0,
                    };
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let i = (((y as i32 * scale + dy) * w + x as i32 * scale + dx) * 4)
                                as usize;
                            px[i..i + 4].copy_from_slice(&argb.to_le_bytes());
                        }
                    }
                }
            }
            self.buf = Some(MemoryRenderBuffer::from_slice(
                &px,
                Fourcc::Argb8888,
                (w, h),
                buffer_scale,
                Transform::Normal,
                None,
            ));
            self.key = Some((shape, scale, buffer_scale));
            self.hotspot = (hot.0 * scale, hot.1 * scale);
        }
        (self.buf.as_ref().unwrap(), self.hotspot)
    }
}

pub struct Scene<'a> {
    pub space: &'a Space<Window>,
    pub tasks: &'a Tasks,
    pub cursor_status: &'a CursorImageStatus,
    pub pointer: Point<f64, Logical>,
    pub scale: i32,
    /// Pixel scale of the arrow (it stays 2× at 1080p even at output scale 1).
    pub art_scale: i32,
    /// Hide the pointer entirely (no pointer device yet).
    pub show_pointer: bool,
    /// A full-screen overlay drawn by Station (the Task List).
    pub overlay: Option<&'a MemoryRenderBuffer>,
    pub personality: Personality,
    pub desktop: &'a Desktop,
}

/// Render elements, front to back.
pub fn build<R>(renderer: &mut R, scene: &Scene, cursor: &mut CursorCache) -> Vec<StationElement<R>>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Clone + Send + 'static,
{
    let scale = Scale::from(scene.scale as f64);
    let mut out: Vec<StationElement<R>> = Vec::new();

    // Pointer on top: the program's, or Station's own (the arrow, or a
    // sizing arrow over Desktop frames).
    if scene.show_pointer {
        let pos: Point<f64, Physical> = scene.pointer.to_physical(scale);
        let own = match (scene.personality, scene.desktop.cursor, scene.cursor_status) {
            (Personality::Desktop, Some(shape), _) => Some(shape),
            (_, _, CursorImageStatus::Named(_)) => Some(CursorShape::Arrow),
            _ => None,
        };
        match (own, scene.cursor_status) {
            (Some(shape), _) => {
                let (buf, hot) = cursor.get(shape, scene.art_scale.max(scene.scale), scene.scale);
                let pos = pos - Point::from((hot.0 as f64, hot.1 as f64));
                if let Ok(e) = MemoryRenderBufferRenderElement::from_buffer(
                    renderer,
                    pos,
                    buf,
                    None,
                    None,
                    None,
                    Kind::Cursor,
                ) {
                    out.push(StationElement::Memory(e));
                }
            }
            (None, CursorImageStatus::Surface(surface)) => {
                let hotspot = with_states(surface, |s| {
                    s.data_map
                        .get::<CursorImageSurfaceData>()
                        .and_then(|d| d.lock().ok().map(|d| d.hotspot))
                        .unwrap_or_default()
                });
                let loc = (pos - hotspot.to_f64().to_physical(scale)).to_i32_round();
                out.extend(render_elements_from_surface_tree(
                    renderer,
                    surface,
                    loc,
                    scale,
                    1.0,
                    Kind::Cursor,
                ));
            }
            _ => {}
        }
    }

    if let Some(buf) = scene.overlay {
        if let Ok(e) = MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            (0.0, 0.0),
            buf,
            None,
            None,
            None,
            Kind::Unspecified,
        ) {
            out.push(StationElement::Memory(e));
            return out;
        }
    }

    if scene.personality == Personality::Desktop {
        for item in &scene.desktop.draw {
            match item {
                DrawItem::Window(id, at) => {
                    let Some(t) = scene.tasks.get(*id) else {
                        continue;
                    };
                    let w = &t.window;
                    let loc = (*at - w.geometry().loc).to_physical_precise_round(scale);
                    out.extend(AsRenderElements::<R>::render_elements::<StationElement<R>>(
                        w, renderer, loc, scale, 1.0,
                    ));
                }
                DrawItem::Layer(l, at) => {
                    let loc = at.to_physical_precise_round(scale);
                    out.extend(AsRenderElements::<R>::render_elements::<StationElement<R>>(
                        l, renderer, loc, scale, 1.0,
                    ));
                }
                DrawItem::Solid(id, r, c) => {
                    let rgba = [
                        ((c >> 16) & 0xFF) as f32 / 255.0,
                        ((c >> 8) & 0xFF) as f32 / 255.0,
                        (c & 0xFF) as f32 / 255.0,
                        1.0,
                    ];
                    out.push(StationElement::Solid(SolidColorRenderElement::new(
                        id.clone(),
                        r.to_physical(scene.scale),
                        CommitCounter::default(),
                        rgba,
                        Kind::Unspecified,
                    )));
                }
                DrawItem::Buffer(buf, at, id) => {
                    let pos = at.to_f64().to_physical(scale);
                    let Ok(e) = MemoryRenderBufferRenderElement::from_buffer(
                        renderer,
                        pos,
                        buf,
                        None,
                        None,
                        None,
                        Kind::Unspecified,
                    ) else {
                        continue;
                    };
                    out.push(match id {
                        Some(id) => StationElement::Tile(Tile {
                            id: id.clone(),
                            inner: e,
                        }),
                        None => StationElement::Memory(e),
                    });
                }
            }
        }
        return out;
    }

    if let Some(fid) = scene.tasks.focused() {
        // Dialogs of the focused task, then the task itself.
        let mut windows: Vec<&Window> = scene
            .tasks
            .dialogs_of(fid)
            .iter()
            .map(|t| &t.window)
            .collect();
        if let Some(t) = scene.tasks.get(fid) {
            if t.dialog {
                // A focused dialog: draw it over its parent.
                windows.push(&t.window);
                if let Some(parent) = t.window.toplevel().and_then(|top| top.parent()) {
                    if let Some(pid) = scene.tasks.find_surface(&parent) {
                        windows.push(&scene.tasks.get(pid).unwrap().window);
                    }
                }
            } else {
                windows.push(&t.window);
            }
        }
        for w in windows {
            let loc = scene.space.element_location(w).unwrap_or_default() - w.geometry().loc;
            let loc = loc.to_physical_precise_round(scale);
            out.extend(AsRenderElements::<R>::render_elements::<StationElement<R>>(
                w, renderer, loc, scale, 1.0,
            ));
        }
    }
    out
}

/// Black, with a hint of Cobalt so an empty screen still looks intentional.
pub const CLEAR: [f32; 4] = [0.039, 0.039, 0.059, 1.0];

/// Render the scene into a fresh offscreen image (screenshots).
pub fn offscreen(
    renderer: &mut PixmanRenderer,
    output: &Output,
    scene: &Scene,
    cursor: &mut CursorCache,
) -> Result<retro_gfx::Canvas, String> {
    let mode = output.current_mode().ok_or("output has no mode")?;
    let (w, h) = (mode.size.w, mode.size.h);
    let mut image = pixman::Image::new(pixman::FormatCode::X8R8G8B8, w as usize, h as usize, true)
        .map_err(|_| "image alloc failed".to_string())?;
    let elements = build(renderer, scene, cursor);
    {
        let mut fb = renderer.bind(&mut image).map_err(|e| format!("{e:?}"))?;
        let mut damage = OutputDamageTracker::from_output(output);
        damage
            .render_output(renderer, &mut fb, 0, &elements, CLEAR)
            .map_err(|e| format!("{e:?}"))?;
    }
    Ok(image_to_canvas(&image))
}

/// The same with the GPU renderer (read back through a texture).
pub fn offscreen_gles(
    renderer: &mut GlesRenderer,
    output: &Output,
    scene: &Scene,
    cursor: &mut CursorCache,
) -> Result<retro_gfx::Canvas, String> {
    let mode = output.current_mode().ok_or("output has no mode")?;
    let (w, h) = (mode.size.w, mode.size.h);
    let mut tex: GlesTexture = renderer
        .create_buffer(Fourcc::Abgr8888, (w, h).into())
        .map_err(|e| format!("{e:?}"))?;
    let elements = build(renderer, scene, cursor);
    let mapping = {
        let mut fb = renderer.bind(&mut tex).map_err(|e| format!("{e:?}"))?;
        let mut damage = OutputDamageTracker::from_output(output);
        damage
            .render_output(renderer, &mut fb, 0, &elements, CLEAR)
            .map_err(|e| format!("{e:?}"))?;
        renderer
            .copy_framebuffer(&fb, Rectangle::from_size((w, h).into()), Fourcc::Abgr8888)
            .map_err(|e| format!("{e:?}"))?
    };
    let flipped = mapping.flipped();
    let data = renderer
        .map_texture(&mapping)
        .map_err(|e| format!("{e:?}"))?;
    let mut c = retro_gfx::Canvas::new(w as u32, h as u32, 0);
    for y in 0..h as usize {
        let sy = if flipped { h as usize - 1 - y } else { y };
        for x in 0..w as usize {
            let i = (sy * w as usize + x) * 4;
            let (r, g, b) = (data[i] as u32, data[i + 1] as u32, data[i + 2] as u32);
            c.px[y * w as usize + x] = (r << 16) | (g << 8) | b;
        }
    }
    Ok(c)
}

pub fn image_to_canvas(image: &pixman::Image<'_, '_>) -> retro_gfx::Canvas {
    let (w, h) = (image.width() as u32, image.height() as u32);
    let stride = image.stride() / 4;
    // SAFETY: pixman owns `stride * height` u32 pixels for the image's lifetime.
    let data: &[u32] =
        unsafe { std::slice::from_raw_parts(image.data() as *const u32, stride * h as usize) };
    let mut c = retro_gfx::Canvas::new(w, h, 0);
    for y in 0..h as usize {
        for x in 0..w as usize {
            c.px[y * w as usize + x] = data[y * stride + x] & 0x00FF_FFFF;
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::CursorShape;
    use crate::chrome::{BOTTOM, LEFT, RIGHT, TOP};

    #[test]
    fn sizing_pointers() {
        assert_eq!(CursorShape::for_edges(LEFT), CursorShape::SizeWE);
        assert_eq!(CursorShape::for_edges(BOTTOM), CursorShape::SizeNS);
        assert_eq!(CursorShape::for_edges(LEFT | TOP), CursorShape::SizeNWSE);
        assert_eq!(
            CursorShape::for_edges(RIGHT | BOTTOM),
            CursorShape::SizeNWSE
        );
        assert_eq!(CursorShape::for_edges(RIGHT | TOP), CursorShape::SizeNESW);
        for s in [
            CursorShape::SizeWE,
            CursorShape::SizeNS,
            CursorShape::SizeNWSE,
            CursorShape::SizeNESW,
        ] {
            let (rows, (hx, hy)) = s.art();
            // The hotspot is on the shape, rows are even, the edge is outlined.
            assert_eq!(rows[hy as usize].as_bytes()[hx as usize], b'o', "{s:?}");
            assert!(rows.iter().all(|r| r.len() == rows[0].len()));
            assert!(!rows.iter().any(|r| r.starts_with('o') || r.ends_with('o')));
        }
        let (we, _) = CursorShape::SizeWE.art();
        assert_eq!(we[5], format!("X{}X", "o".repeat(17)));
    }
}

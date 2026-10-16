//! What Station draws: in text mode, the focused task full-screen, its
//! dialogs on top, and the mouse pointer.

use crate::tasks::Tasks;
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::element::memory::{
    MemoryRenderBuffer, MemoryRenderBufferRenderElement,
};
use smithay::backend::renderer::element::surface::{
    render_elements_from_surface_tree, WaylandSurfaceRenderElement,
};
use smithay::backend::renderer::element::{AsRenderElements, Kind};
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::backend::renderer::{Bind, ImportAll, ImportMem, Renderer};
use smithay::desktop::{Space, Window};
use smithay::input::pointer::{CursorImageStatus, CursorImageSurfaceData};
use smithay::output::Output;
use smithay::reexports::pixman;
use smithay::utils::{Logical, Physical, Point, Scale, Transform};
use smithay::wayland::compositor::with_states;

smithay::backend::renderer::element::render_elements! {
    pub StationElement<R> where R: ImportAll + ImportMem;
    Surface=WaylandSurfaceRenderElement<R>,
    Memory=MemoryRenderBufferRenderElement<R>,
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

/// The arrow pre-scaled to the output scale (so it stays pixel-sharp).
#[derive(Default)]
pub struct CursorCache {
    scale: i32,
    buf: Option<MemoryRenderBuffer>,
}

impl CursorCache {
    fn get(&mut self, scale: i32) -> &MemoryRenderBuffer {
        if self.scale != scale || self.buf.is_none() {
            let (w, h) = (ARROW[0].len() as i32 * scale, ARROW.len() as i32 * scale);
            let mut px = vec![0u8; (w * h * 4) as usize];
            for (y, row) in ARROW.iter().enumerate() {
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
                scale,
                Transform::Normal,
                None,
            ));
            self.scale = scale;
        }
        self.buf.as_ref().unwrap()
    }
}

pub struct Scene<'a> {
    pub space: &'a Space<Window>,
    pub tasks: &'a Tasks,
    pub cursor_status: &'a CursorImageStatus,
    pub pointer: Point<f64, Logical>,
    pub scale: i32,
    /// Hide the pointer entirely (no pointer device yet).
    pub show_pointer: bool,
}

/// Render elements, front to back.
pub fn build<R>(renderer: &mut R, scene: &Scene, cursor: &mut CursorCache) -> Vec<StationElement<R>>
where
    R: Renderer + ImportAll + ImportMem,
    R::TextureId: Clone + Send + 'static,
{
    let scale = Scale::from(scene.scale as f64);
    let mut out: Vec<StationElement<R>> = Vec::new();

    // Pointer on top.
    if scene.show_pointer {
        let pos: Point<f64, Physical> = scene.pointer.to_physical(scale);
        match scene.cursor_status {
            CursorImageStatus::Hidden => {}
            CursorImageStatus::Surface(surface) => {
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
            CursorImageStatus::Named(_) => {
                let buf = cursor.get(scene.scale);
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
        }
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

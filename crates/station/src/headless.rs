//! Headless backend: renders with Pixman into memory at a fixed size. Used
//! for automated tests (screenshots via the broker) and development without
//! a display.

use crate::render::{self, CursorCache, Scene};
use crate::state::{client_scale, integer_scale, Station};
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::backend::renderer::Bind;
use smithay::backend::renderer::{Renderer as _, TextureFilter};
use smithay::output::{Mode, Output, PhysicalProperties, Scale, Subpixel};
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::pixman;
use std::time::Duration;

pub struct HeadlessData {
    pub renderer: PixmanRenderer,
    /// Degrees clockwise the picture is turned on the virtual panel
    /// (`RETROOS_ROTATE`, D-042); screenshots are turned back upright.
    rotation: u32,
    image: pixman::Image<'static, 'static>,
    damage: OutputDamageTracker,
    pub cursor: CursorCache,
}

pub fn init(st: &mut Station, w: i32, h: i32) {
    let output = Output::new(
        "HEADLESS-1".into(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "Phosphor".into(),
            model: "Virtual Display".into(),
        },
    );
    let mode = Mode {
        size: (w, h).into(),
        refresh: 60_000,
    };
    // `--headless 1200x1920` with RETROOS_ROTATE=auto|90|270 stands in for a
    // portrait panel shown landscape (only the env var: tests must not pick
    // up the host's settings).
    let rotation = std::env::var("RETROOS_ROTATE")
        .ok()
        .map(|v| retro_gfx::screen::rotation_for(Some(&v), w as u32, h as u32))
        .unwrap_or(0);
    let (lw, lh) = retro_gfx::screen::logical_size(w as u32, h as u32, rotation);
    st.scale = client_scale(lw as i32, lh as i32);
    st.art_scale = integer_scale(lw as i32, lh as i32);
    output.change_current_state(
        Some(mode),
        Some(render::transform_for(rotation)),
        Some(Scale::Integer(st.scale)),
        Some((0, 0).into()),
    );
    output.set_preferred(mode);
    let _global = output.create_global::<Station>(&st.dh);
    st.space.map_output(&output, (0, 0));
    let damage = OutputDamageTracker::from_output(&output);
    st.output = Some(output);
    let image = pixman::Image::new(pixman::FormatCode::X8R8G8B8, w as usize, h as usize, true)
        .expect("image");
    st.backend = crate::Backend::Headless(Box::new(HeadlessData {
        renderer: {
            let mut r = PixmanRenderer::new().expect("pixman");
            let _ = r.upscale_filter(TextureFilter::Nearest);
            r
        },
        rotation,
        image,
        damage,
        cursor: CursorCache::default(),
    }));
    // 30 fps when something changed.
    st.loop_handle
        .insert_source(
            Timer::from_duration(Duration::from_millis(33)),
            |_, _, st| {
                if st.dirty {
                    render(st);
                }
                TimeoutAction::ToDuration(Duration::from_millis(33))
            },
        )
        .expect("render timer");
}

pub fn render(st: &mut Station) {
    st.prepare_frame();
    let crate::Backend::Headless(hd) = &mut st.backend else {
        return;
    };
    let scene = Scene {
        space: &st.space,
        tasks: &st.tasks,
        cursor_status: &st.cursor_status,
        pointer: st.pointer_location,
        scale: st.scale,
        art_scale: st.art_scale,
        show_pointer: false,
        overlay: st.task_list.as_ref().and_then(|t| t.buffer()),
        personality: st.personality,
        desktop: &st.desktop,
        x11_overrides: &st.x11_overrides,
    };
    let elements = render::build(&mut hd.renderer, &scene, &mut hd.cursor);
    if let Ok(mut fb) = hd.renderer.bind(&mut hd.image) {
        if let Err(e) =
            hd.damage
                .render_output(&mut hd.renderer, &mut fb, 0, &elements, render::CLEAR)
        {
            tracing::warn!("headless render failed: {e:?}");
        }
    }
    st.dirty = false;
    crate::send_frames(st);
}

pub fn screenshot(st: &mut Station) -> retro_gfx::Canvas {
    render(st);
    let crate::Backend::Headless(hd) = &st.backend else {
        unreachable!()
    };
    let panel = render::image_to_canvas(&hd.image);
    if hd.rotation == 0 {
        return panel;
    }
    let (lw, lh) = retro_gfx::screen::logical_size(panel.width, panel.height, hd.rotation);
    let mut c = retro_gfx::Canvas::new(lw, lh, 0);
    for y in 0..lh {
        for x in 0..lw {
            let (px, py) = retro_gfx::screen::to_panel(x, y, lw, lh, hd.rotation);
            c.px[(y * lw + x) as usize] = panel.px[(py * panel.width + px) as usize];
        }
    }
    c
}

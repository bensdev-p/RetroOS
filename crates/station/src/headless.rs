//! Headless backend: renders with Pixman into memory at a fixed size. Used
//! for automated tests (screenshots via the broker) and development without
//! a display.

use crate::render::{self, CursorCache, Scene};
use crate::state::{integer_scale, Station};
use smithay::backend::renderer::damage::OutputDamageTracker;
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::backend::renderer::Bind;
use smithay::output::{Mode, Output, PhysicalProperties, Scale, Subpixel};
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::pixman;
use smithay::utils::Transform;
use std::time::Duration;

pub struct HeadlessData {
    pub renderer: PixmanRenderer,
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
    st.scale = integer_scale(w, h);
    output.change_current_state(
        Some(mode),
        Some(Transform::Normal),
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
        renderer: PixmanRenderer::new().expect("pixman"),
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
    st.task_list_buffer();
    let crate::Backend::Headless(hd) = &mut st.backend else {
        return;
    };
    let scene = Scene {
        space: &st.space,
        tasks: &st.tasks,
        cursor_status: &st.cursor_status,
        pointer: st.pointer_location,
        scale: st.scale,
        show_pointer: false,
        overlay: st.task_list.as_ref().and_then(|t| t.buffer()),
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
    render::image_to_canvas(&hd.image)
}

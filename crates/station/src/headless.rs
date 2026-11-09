//! Headless backend: renders with Pixman into memory at fixed sizes. Used
//! for automated tests (screenshots via the broker) and development without
//! a display. `--headless 1200x1920+1920x1080` gives two screens (D-051):
//! the first is the main one (turned by `RETROOS_ROTATE`, as a Touch
//! Display 2 would be), the others extend the desktop to its right;
//! screenshots show them side by side.

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

/// One virtual screen.
struct Head {
    output: Output,
    /// Degrees clockwise the picture is turned on the virtual panel
    /// (`RETROOS_ROTATE`, D-042, main screen only); screenshots are turned
    /// back upright.
    rotation: u32,
    image: pixman::Image<'static, 'static>,
    damage: OutputDamageTracker,
}

pub struct HeadlessData {
    pub renderer: PixmanRenderer,
    heads: Vec<Head>,
    pub cursor: CursorCache,
}

fn make_output(n: usize, w: i32, h: i32, rotation: u32, scale: i32) -> Output {
    let output = Output::new(
        format!("HEADLESS-{}", n + 1),
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
    output.change_current_state(
        Some(mode),
        Some(render::transform_for(rotation)),
        Some(Scale::Integer(scale)),
        Some((0, 0).into()),
    );
    output.set_preferred(mode);
    output
}

pub fn init(st: &mut Station, sizes: &[(i32, i32)]) {
    let mut heads = Vec::new();
    for (n, &(w, h)) in sizes.iter().enumerate() {
        // `--headless 1200x1920` with RETROOS_ROTATE=auto|90|270 stands in for
        // a portrait panel shown landscape (only the env var: tests must not
        // pick up the host's settings).
        let rotation = if n == 0 {
            std::env::var("RETROOS_ROTATE")
                .ok()
                .map(|v| retro_gfx::screen::rotation_for(Some(&v), w as u32, h as u32))
                .unwrap_or(0)
        } else {
            0
        };
        if n == 0 {
            let (lw, lh) = retro_gfx::screen::logical_size(w as u32, h as u32, rotation);
            st.scale = client_scale(lw as i32, lh as i32);
            st.art_scale = integer_scale(lw as i32, lh as i32);
        }
        let output = make_output(n, w, h, rotation, st.scale);
        let _global = output.create_global::<Station>(&st.dh);
        if n == 0 {
            st.space.map_output(&output, (0, 0));
            st.output = Some(output.clone());
        } else {
            st.add_extra_output(output.clone());
        }
        let damage = OutputDamageTracker::from_output(&output);
        let image = pixman::Image::new(pixman::FormatCode::X8R8G8B8, w as usize, h as usize, true)
            .expect("image");
        heads.push(Head {
            output,
            rotation,
            image,
            damage,
        });
    }
    st.backend = crate::Backend::Headless(Box::new(HeadlessData {
        renderer: {
            let mut r = PixmanRenderer::new().expect("pixman");
            let _ = r.upscale_filter(TextureFilter::Nearest);
            r
        },
        heads,
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
    for head in hd.heads.iter_mut() {
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
            origin: head.output.current_location(),
        };
        let elements = render::build(&mut hd.renderer, &scene, &mut hd.cursor);
        // Age 1: the same image every frame, so only what changed is drawn,
        // as on the screens (damage bugs show in host tests too).
        if let Ok(mut fb) = hd.renderer.bind(&mut head.image) {
            if let Err(e) =
                head.damage
                    .render_output(&mut hd.renderer, &mut fb, 1, &elements, render::CLEAR)
            {
                tracing::warn!("headless render failed: {e:?}");
            }
        }
    }
    st.dirty = false;
    crate::send_frames(st);
}

/// One screen's picture, the right way up.
fn upright(head: &Head) -> retro_gfx::Canvas {
    let panel = render::image_to_canvas(&head.image);
    if head.rotation == 0 {
        return panel;
    }
    let (lw, lh) = retro_gfx::screen::logical_size(panel.width, panel.height, head.rotation);
    let mut c = retro_gfx::Canvas::new(lw, lh, 0);
    for y in 0..lh {
        for x in 0..lw {
            let (px, py) = retro_gfx::screen::to_panel(x, y, lw, lh, head.rotation);
            c.px[(y * lw + x) as usize] = panel.px[(py * panel.width + px) as usize];
        }
    }
    c
}

/// Every screen, placed as in the desktop (the space between screens of
/// different heights stays black).
pub fn screenshot(st: &mut Station) -> retro_gfx::Canvas {
    render(st);
    let lay = st.layout_rect();
    let scale = st.scale.max(1);
    let crate::Backend::Headless(hd) = &st.backend else {
        unreachable!()
    };
    if hd.heads.len() == 1 {
        return upright(&hd.heads[0]);
    }
    let mut c = retro_gfx::Canvas::new((lay.size.w * scale) as u32, (lay.size.h * scale) as u32, 0);
    for head in &hd.heads {
        let pic = upright(head);
        let at = head.output.current_location() - lay.loc;
        c.surface().blit(&pic, at.x * scale, at.y * scale);
    }
    c
}

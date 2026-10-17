//! The real-hardware backend: libseat session, one KMS output driven through
//! Smithay's DrmCompositor, and libinput for keyboards and mice.
//!
//! Rendering is GLES on the GPU (GBM/EGL; v3d on the Pi) with linux-dmabuf
//! for GPU clients, or Pixman on the CPU into dumb buffers when there is no
//! GPU or only a software one (QEMU without virgl). D-023.

use crate::render::{self, CursorCache, Scene};
use crate::state::{client_scale, integer_scale, Station};
use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::backend::allocator::dumb::DumbAllocator;
use smithay::backend::allocator::gbm::{GbmAllocator, GbmBufferFlags, GbmDevice};
use smithay::backend::allocator::Fourcc;
use smithay::backend::drm::compositor::{DrmCompositor, FrameFlags};
use smithay::backend::drm::exporter::gbm::GbmFramebufferExporter;
use smithay::backend::drm::{DrmDevice, DrmDeviceFd, DrmEvent, DrmNode, DrmSurface};
use smithay::backend::egl::{EGLContext, EGLDevice, EGLDisplay};
use smithay::backend::input::InputEvent;
use smithay::backend::libinput::{LibinputInputBackend, LibinputSessionInterface};
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::backend::renderer::{ImportDma, Renderer as _, TextureFilter};
use smithay::backend::session::libseat::LibSeatSession;
use smithay::backend::session::{Event as SessionEvent, Session};
use smithay::backend::udev::UdevBackend;
use smithay::output::{Mode, Output, OutputModeSource, PhysicalProperties, Scale, Subpixel};
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::drm::control::{connector, Device as ControlDevice, ModeTypeFlags};
use smithay::reexports::input::{DeviceCapability, Libinput};
use smithay::reexports::rustix::fs::OFlags;
use smithay::utils::{DeviceFd, Transform};
use smithay::wayland::dmabuf::DmabufFeedbackBuilder;
use std::path::Path;
use std::time::Duration;

type DumbCompositor = DrmCompositor<DumbAllocator, DrmDeviceFd, (), DrmDeviceFd>;
type GbmCompositor =
    DrmCompositor<GbmAllocator<DrmDeviceFd>, GbmFramebufferExporter<DrmDeviceFd>, (), DrmDeviceFd>;

/// How frames are drawn: on the GPU (GLES via GBM/EGL), or on the CPU into
/// dumb buffers when there is no usable GPU (docs/DESIGN.md §6.3).
pub enum Gpu {
    Pixman {
        renderer: PixmanRenderer,
        compositor: DumbCompositor,
    },
    Gles {
        renderer: Box<GlesRenderer>,
        compositor: GbmCompositor,
        /// Where clients should allocate buffers (linux-dmabuf feedback).
        render_node: Option<DrmNode>,
    },
}

/// Run `$body` with `$r` = the renderer and `$c` = the DRM compositor.
macro_rules! with_gpu {
    ($gpu:expr, |$r:ident, $c:ident| $body:expr) => {
        match $gpu {
            Gpu::Pixman {
                renderer: $r,
                compositor: $c,
            } => $body,
            Gpu::Gles {
                renderer: $r,
                compositor: $c,
                ..
            } => {
                let $r = &mut **$r;
                $body
            }
        }
    };
}

pub struct UdevData {
    pub session: LibSeatSession,
    libinput: Libinput,
    drm: DrmDevice,
    pub gpu: Gpu,
    pub cursor: CursorCache,
    frame_pending: bool,
    pub pointer_seen: bool,
}

impl UdevData {
    /// Import a client's dmabuf into the GPU renderer.
    pub fn import_dmabuf(&mut self, dmabuf: &Dmabuf) -> bool {
        match &mut self.gpu {
            Gpu::Gles { renderer, .. } => renderer.import_dmabuf(dmabuf, None).is_ok(),
            Gpu::Pixman { .. } => false,
        }
    }
}

struct Opened {
    drm: DrmDevice,
    notifier: smithay::backend::drm::DrmDeviceNotifier,
    gpu: Gpu,
    output: Output,
    scale: i32,
}

/// A GLES renderer on this DRM device, unless there is no GPU or only a
/// software one (then Pixman into dumb buffers is cheaper and simpler).
fn open_gles(
    fd: &DrmDeviceFd,
) -> Result<(GbmDevice<DrmDeviceFd>, GlesRenderer, Option<DrmNode>), String> {
    if std::env::var("RETROOS_RENDERER").is_ok_and(|v| v == "pixman") {
        return Err("RETROOS_RENDERER=pixman".into());
    }
    let gbm = GbmDevice::new(fd.clone()).map_err(|e| format!("gbm: {e}"))?;
    // SAFETY: the GBM device outlives the display (both live in Gpu::Gles).
    let display = unsafe { EGLDisplay::new(gbm.clone()) }.map_err(|e| format!("egl: {e}"))?;
    let device = EGLDevice::device_for_display(&display).ok();
    if device.as_ref().is_some_and(|d| d.is_software()) {
        return Err("only a software renderer is available".into());
    }
    let node = device.and_then(|d| d.try_get_render_node().ok().flatten());
    let context = EGLContext::new(&display).map_err(|e| format!("egl context: {e}"))?;
    // SAFETY: the context is current only on this thread.
    let mut renderer = unsafe { GlesRenderer::new(context) }.map_err(|e| format!("gles: {e}"))?;
    let _ = renderer.upscale_filter(TextureFilter::Nearest);
    let name = renderer
        .with_context(|gl| unsafe {
            let p = gl.GetString(smithay::backend::renderer::gles::ffi::RENDERER);
            if p.is_null() {
                String::new()
            } else {
                std::ffi::CStr::from_ptr(p as *const std::ffi::c_char)
                    .to_string_lossy()
                    .into_owned()
            }
        })
        .unwrap_or_default();
    if ["llvmpipe", "softpipe", "swrast"]
        .iter()
        .any(|s| name.contains(s))
    {
        return Err(format!("only a software renderer is available ({name})"));
    }
    tracing::info!(renderer = %name, node = ?node, "GPU rendering");
    Ok((gbm, renderer, node))
}

fn dumb_compositor(
    output: &Output,
    surface: DrmSurface,
    fd: &DrmDeviceFd,
    renderer: &PixmanRenderer,
) -> Result<DumbCompositor, String> {
    DumbCompositor::new(
        OutputModeSource::Auto(output.clone()),
        surface,
        None,
        DumbAllocator::new(fd.clone()),
        fd.clone(),
        [Fourcc::Xrgb8888, Fourcc::Argb8888],
        renderer.dmabuf_formats(),
        (64, 64).into(),
        None::<GbmDevice<DrmDeviceFd>>,
    )
    .map_err(|e| format!("compositor: {e:?}"))
}

fn open_device(
    session: &mut LibSeatSession,
    path: &Path,
    dh: &smithay::reexports::wayland_server::DisplayHandle,
) -> Result<Opened, String> {
    let fd = session
        .open(
            path,
            OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
        )
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    let fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let (mut drm, notifier) = DrmDevice::new(fd.clone(), true).map_err(|e| format!("drm: {e}"))?;
    let res = drm
        .resource_handles()
        .map_err(|e| format!("resources: {e}"))?;
    let conn = res
        .connectors()
        .iter()
        .filter_map(|c| drm.get_connector(*c, false).ok())
        .find(|c| c.state() == connector::State::Connected && !c.modes().is_empty())
        .ok_or("no connected display")?;
    let mode = conn
        .modes()
        .iter()
        .find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
        .copied()
        .unwrap_or(conn.modes()[0]);
    let crtc = conn
        .encoders()
        .iter()
        .filter_map(|e| drm.get_encoder(*e).ok())
        .find_map(|enc| res.filter_crtcs(enc.possible_crtcs()).first().copied())
        .ok_or("no CRTC for display")?;

    let name = format!("{}-{}", conn.interface().as_str(), conn.interface_id());
    let (mm_w, mm_h) = conn.size().unwrap_or((0, 0));
    let output = Output::new(
        name.clone(),
        PhysicalProperties {
            size: (mm_w as i32, mm_h as i32).into(),
            subpixel: Subpixel::Unknown,
            make: "Phosphor".into(),
            model: name,
        },
    );
    let (w, h) = mode.size();
    let scale = client_scale(w as i32, h as i32);
    let wl_mode = Mode::from(mode);
    output.change_current_state(
        Some(wl_mode),
        Some(Transform::Normal),
        Some(Scale::Integer(scale)),
        Some((0, 0).into()),
    );
    output.set_preferred(wl_mode);
    let _global = output.create_global::<Station>(dh);

    let new_surface = |drm: &mut DrmDevice| {
        drm.create_surface(crtc, mode, &[conn.handle()])
            .map_err(|e| format!("surface: {e}"))
    };
    let gpu = match open_gles(&fd) {
        Ok((gbm, renderer, render_node)) => {
            let formats = renderer.dmabuf_formats();
            match GbmCompositor::new(
                OutputModeSource::Auto(output.clone()),
                new_surface(&mut drm)?,
                None,
                GbmAllocator::new(
                    gbm.clone(),
                    GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
                ),
                GbmFramebufferExporter::new(gbm.clone(), render_node),
                [Fourcc::Xrgb8888, Fourcc::Argb8888],
                formats,
                (64, 64).into(),
                Some(gbm),
            ) {
                Ok(compositor) => Some(Gpu::Gles {
                    renderer: Box::new(renderer),
                    compositor,
                    render_node,
                }),
                Err(e) => {
                    tracing::warn!("GPU output failed ({e:?}); using CPU rendering");
                    None
                }
            }
        }
        Err(e) => {
            tracing::info!("CPU rendering: {e}");
            None
        }
    };
    let gpu = match gpu {
        Some(g) => g,
        None => {
            let mut renderer = PixmanRenderer::new().map_err(|e| format!("pixman: {e:?}"))?;
            // Integer enlargement must stay pixel-sharp (the console's 1× buffer).
            let _ = renderer.upscale_filter(TextureFilter::Nearest);
            let compositor = dumb_compositor(&output, new_surface(&mut drm)?, &fd, &renderer)?;
            Gpu::Pixman {
                renderer,
                compositor,
            }
        }
    };
    Ok(Opened {
        drm,
        notifier,
        gpu,
        output,
        scale,
    })
}

pub fn init(st: &mut Station) -> Result<(), String> {
    let (mut session, session_notifier) =
        LibSeatSession::new().map_err(|e| format!("libseat: {e}"))?;
    let seat = session.seat();
    let udev = UdevBackend::new(&seat).map_err(|e| format!("udev: {e}"))?;

    let mut devices: Vec<_> = udev.device_list().map(|(_, p)| p.to_path_buf()).collect();
    devices.sort();
    let mut opened = None;
    let mut errors = Vec::new();
    for p in &devices {
        match open_device(&mut session, p, &st.dh) {
            Ok(o) => {
                tracing::info!(device = %p.display(), output = %o.output.name(), scale = o.scale, "display");
                opened = Some(o);
                break;
            }
            Err(e) => errors.push(e),
        }
    }
    let Opened {
        drm,
        notifier,
        gpu,
        output,
        scale,
    } = opened.ok_or_else(|| format!("no usable display: {}", errors.join("; ")))?;
    // GPU clients: linux-dmabuf, with feedback naming the render device.
    if let Gpu::Gles {
        renderer,
        render_node,
        ..
    } = &gpu
    {
        let formats = renderer.dmabuf_formats();
        let global = match render_node.and_then(|n| {
            DmabufFeedbackBuilder::new(n.dev_id(), formats.clone())
                .build()
                .ok()
        }) {
            Some(fb) => st
                .dmabuf_state
                .create_global_with_default_feedback::<Station>(&st.dh, &fb),
            None => st.dmabuf_state.create_global::<Station>(&st.dh, formats),
        };
        st.dmabuf_global = Some(global);
    }
    st.scale = scale;
    if let Some(m) = output.current_mode() {
        st.art_scale = integer_scale(m.size.w, m.size.h);
    }
    st.space.map_output(&output, (0, 0));
    st.output = Some(output);

    let mut libinput =
        Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(session.clone().into());
    libinput
        .udev_assign_seat(&seat)
        .map_err(|_| "libinput seat assign failed")?;
    let input_backend = LibinputInputBackend::new(libinput.clone());

    st.backend = crate::Backend::Udev(Box::new(UdevData {
        session,
        libinput,
        drm,
        gpu,
        cursor: CursorCache::default(),
        frame_pending: false,
        pointer_seen: false,
    }));

    let h = st.loop_handle.clone();
    h.insert_source(input_backend, |mut event, _, st: &mut Station| {
        if let InputEvent::DeviceAdded { device } = &mut event {
            if device.has_capability(DeviceCapability::Pointer) {
                if let crate::Backend::Udev(ud) = &mut st.backend {
                    ud.pointer_seen = true;
                }
            }
        }
        st.process_input_event(event);
    })
    .map_err(|_| "input source")?;

    h.insert_source(notifier, |event, _, st: &mut Station| match event {
        DrmEvent::VBlank(_) => frame_finish(st),
        DrmEvent::Error(e) => tracing::error!("drm error: {e}"),
    })
    .map_err(|_| "drm source")?;

    h.insert_source(session_notifier, |event, _, st: &mut Station| {
        let crate::Backend::Udev(ud) = &mut st.backend else {
            return;
        };
        match event {
            SessionEvent::PauseSession => {
                tracing::info!("session paused");
                ud.libinput.suspend();
                ud.drm.pause();
            }
            SessionEvent::ActivateSession => {
                tracing::info!("session resumed");
                let _ = ud.libinput.resume();
                let _ = ud.drm.activate(false);
                let _ = with_gpu!(&mut ud.gpu, |_r, c| c
                    .reset_state()
                    .map_err(|e| format!("{e:?}")));
                ud.frame_pending = false;
                st.dirty = true;
                render(st);
            }
        }
    })
    .map_err(|_| "session source")?;

    // Render tick: catches changes that arrive while no vblank is pending.
    h.insert_source(
        Timer::from_duration(Duration::from_millis(16)),
        |_, _, st| {
            if st.dirty {
                render(st);
            }
            TimeoutAction::ToDuration(Duration::from_millis(16))
        },
    )
    .map_err(|_| "render timer")?;
    st.dirty = true;
    render(st);
    Ok(())
}

pub fn render(st: &mut Station) {
    st.task_list_buffer();
    let crate::Backend::Udev(ud) = &mut st.backend else {
        return;
    };
    if !ud.session.is_active() || ud.frame_pending {
        return;
    }
    let scene = Scene {
        space: &st.space,
        tasks: &st.tasks,
        cursor_status: &st.cursor_status,
        pointer: st.pointer_location,
        scale: st.scale,
        art_scale: st.art_scale,
        show_pointer: ud.pointer_seen && st.pointer_moved,
        overlay: st.task_list.as_ref().and_then(|t| t.buffer()),
    };
    let cursor = &mut ud.cursor;
    let result = with_gpu!(&mut ud.gpu, |r, c| {
        let elements = render::build(r, &scene, cursor);
        c.render_frame(r, &elements, render::CLEAR, FrameFlags::DEFAULT)
            .map(|res| res.is_empty)
            .map_err(|e| format!("{e:?}"))
            .and_then(|empty| {
                if empty {
                    Ok(false)
                } else {
                    c.queue_frame(())
                        .map(|_| true)
                        .map_err(|e| format!("queue_frame: {e:?}"))
                }
            })
    });
    match result {
        Ok(queued) => {
            if queued {
                ud.frame_pending = true;
            }
            st.dirty = false;
        }
        Err(e) => tracing::warn!("render: {e}"),
    }
    crate::send_frames(st);
}

fn frame_finish(st: &mut Station) {
    if let crate::Backend::Udev(ud) = &mut st.backend {
        if let Err(e) = with_gpu!(&mut ud.gpu, |_r, c| c
            .frame_submitted()
            .map(|_| ())
            .map_err(|e| format!("{e:?}")))
        {
            tracing::warn!("frame_submitted: {e:?}");
        }
        ud.frame_pending = false;
    }
    if st.dirty {
        render(st);
    }
}

pub fn change_vt(st: &mut Station, vt: i32) {
    if let crate::Backend::Udev(ud) = &mut st.backend {
        if let Err(e) = ud.session.change_vt(vt) {
            tracing::warn!("change_vt({vt}): {e}");
        }
    }
}

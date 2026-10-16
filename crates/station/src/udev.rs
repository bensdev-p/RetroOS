//! The real-hardware backend: libseat session, one KMS output driven through
//! Smithay's DrmCompositor with CPU (Pixman) rendering into dumb buffers, and
//! libinput for keyboards and mice.
//!
//! Text mode needs no GPU context at all (docs/DECISIONS.md D-007); the GLES
//! path for GPU clients arrives with Milestone 2.

use crate::render::{self, CursorCache, Scene};
use crate::state::{integer_scale, Station};
use smithay::backend::allocator::dumb::DumbAllocator;
use smithay::backend::allocator::gbm::GbmDevice;
use smithay::backend::allocator::Fourcc;
use smithay::backend::drm::compositor::{DrmCompositor, FrameFlags};
use smithay::backend::drm::{DrmDevice, DrmDeviceFd, DrmEvent};
use smithay::backend::input::InputEvent;
use smithay::backend::libinput::{LibinputInputBackend, LibinputSessionInterface};
use smithay::backend::renderer::pixman::PixmanRenderer;
use smithay::backend::renderer::ImportDma;
use smithay::backend::session::libseat::LibSeatSession;
use smithay::backend::session::{Event as SessionEvent, Session};
use smithay::backend::udev::UdevBackend;
use smithay::output::{Mode, Output, OutputModeSource, PhysicalProperties, Scale, Subpixel};
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::drm::control::{connector, Device as ControlDevice, ModeTypeFlags};
use smithay::reexports::input::{DeviceCapability, Libinput};
use smithay::reexports::rustix::fs::OFlags;
use smithay::utils::{DeviceFd, Transform};
use std::path::Path;
use std::time::Duration;

type Compositor = DrmCompositor<DumbAllocator, DrmDeviceFd, (), DrmDeviceFd>;

pub struct UdevData {
    pub session: LibSeatSession,
    libinput: Libinput,
    drm: DrmDevice,
    compositor: Compositor,
    pub renderer: PixmanRenderer,
    pub cursor: CursorCache,
    frame_pending: bool,
    pub pointer_seen: bool,
}

struct Opened {
    drm: DrmDevice,
    notifier: smithay::backend::drm::DrmDeviceNotifier,
    compositor: Compositor,
    output: Output,
    scale: i32,
}

fn open_device(
    session: &mut LibSeatSession,
    path: &Path,
    dh: &smithay::reexports::wayland_server::DisplayHandle,
    renderer: &PixmanRenderer,
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
    let surface = drm
        .create_surface(crtc, mode, &[conn.handle()])
        .map_err(|e| format!("surface: {e}"))?;

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
    let scale = integer_scale(w as i32, h as i32);
    let wl_mode = Mode::from(mode);
    output.change_current_state(
        Some(wl_mode),
        Some(Transform::Normal),
        Some(Scale::Integer(scale)),
        Some((0, 0).into()),
    );
    output.set_preferred(wl_mode);
    let _global = output.create_global::<Station>(dh);

    let compositor = Compositor::new(
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
    .map_err(|e| format!("compositor: {e:?}"))?;
    Ok(Opened {
        drm,
        notifier,
        compositor,
        output,
        scale,
    })
}

pub fn init(st: &mut Station) -> Result<(), String> {
    let (mut session, session_notifier) =
        LibSeatSession::new().map_err(|e| format!("libseat: {e}"))?;
    let seat = session.seat();
    let udev = UdevBackend::new(&seat).map_err(|e| format!("udev: {e}"))?;
    let renderer = PixmanRenderer::new().map_err(|e| format!("pixman: {e:?}"))?;

    let mut devices: Vec<_> = udev.device_list().map(|(_, p)| p.to_path_buf()).collect();
    devices.sort();
    let mut opened = None;
    let mut errors = Vec::new();
    for p in &devices {
        match open_device(&mut session, p, &st.dh, &renderer) {
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
        compositor,
        output,
        scale,
    } = opened.ok_or_else(|| format!("no usable display: {}", errors.join("; ")))?;
    st.scale = scale;
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
        compositor,
        renderer,
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
                let _ = ud.compositor.reset_state();
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
        show_pointer: ud.pointer_seen,
    };
    let elements = render::build(&mut ud.renderer, &scene, &mut ud.cursor);
    match ud.compositor.render_frame(
        &mut ud.renderer,
        &elements,
        render::CLEAR,
        FrameFlags::DEFAULT,
    ) {
        Ok(res) => {
            if !res.is_empty {
                match ud.compositor.queue_frame(()) {
                    Ok(()) => ud.frame_pending = true,
                    Err(e) => tracing::warn!("queue_frame: {e:?}"),
                }
            }
            st.dirty = false;
        }
        Err(e) => tracing::warn!("render_frame: {e:?}"),
    }
    crate::send_frames(st);
}

fn frame_finish(st: &mut Station) {
    if let crate::Backend::Udev(ud) = &mut st.backend {
        if let Err(e) = ud.compositor.frame_submitted() {
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

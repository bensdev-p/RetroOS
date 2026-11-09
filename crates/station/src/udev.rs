//! The real-hardware backend: libseat session, every connected screen on
//! every DRM device driven through Smithay's DrmCompositor (D-051; the main
//! screen first), and libinput for keyboards and mice.
//!
//! Rendering is Pixman on the CPU into dumb buffers while only the console and
//! other software-drawn programs are on screen, and GLES on the GPU (GBM/EGL;
//! v3d on the Pi) with linux-dmabuf while graphical programs run: the switch
//! happens as the broker launches one and ~60 s after the last one
//! exits, so the prompt does not carry Mesa and GPU buffers (D-023, D-028).
//! `renderer=gpu|cpu|auto` in /etc/retroos/startup.conf (or RETROOS_RENDERER)
//! pins it.

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
use smithay::backend::udev::{UdevBackend, UdevEvent};
use smithay::output::{Mode, Output, OutputModeSource, PhysicalProperties, Scale, Subpixel};
use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay::reexports::drm::control::{
    connector, crtc, Device as ControlDevice, Mode as DrmMode, ModeTypeFlags,
};
use smithay::reexports::input::{DeviceCapability, Libinput};
use smithay::reexports::rustix::fs::OFlags;
use smithay::reexports::wayland_server::DisplayHandle;
use smithay::utils::DeviceFd;
use smithay::wayland::dmabuf::DmabufFeedbackBuilder;
use std::path::Path;
use std::time::{Duration, Instant};

/// When the GPU renderer is used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RendererPolicy {
    /// CPU at the prompt, GPU while graphical programs run.
    Auto,
    Gpu,
    Cpu,
}

fn renderer_policy() -> RendererPolicy {
    let conf = std::fs::read_to_string("/etc/retroos/startup.conf").unwrap_or_default();
    let v = std::env::var("RETROOS_RENDERER").ok().or_else(|| {
        conf.lines().find_map(|l| {
            l.split('#')
                .next()?
                .trim()
                .strip_prefix("renderer=")
                .map(|v| v.trim().to_string())
        })
    });
    match v.as_deref() {
        Some("gpu" | "gles") => RendererPolicy::Gpu,
        Some("cpu" | "pixman") => RendererPolicy::Cpu,
        _ => RendererPolicy::Auto,
    }
}

/// Time without graphical programs before the GPU renderer is dropped (long
/// enough that closing and reopening a program does not pay the switch twice).
const GPU_IDLE: Duration = Duration::from_secs(60);

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

/// One screen: a connector on a DRM device, its CRTC and mode, the wl_output
/// for it, and the renderer and compositor that draw it (D-051).
pub struct Head {
    /// Index into [`UdevData::devices`].
    dev: usize,
    crtc: crtc::Handle,
    mode: DrmMode,
    connector: connector::Handle,
    pub output: Output,
    /// None only for the moment a renderer switch takes.
    pub gpu: Option<Gpu>,
    frame_pending: bool,
    /// GLES failed on this device (no GPU, or none for this display
    /// controller): stay on the CPU.
    gpu_unavailable: bool,
}

/// An open DRM device (the Pi 5 has two with screens: `vc4` for HDMI and
/// `rp1-dsi` for the Touch Display 2).
pub struct Device {
    id: libc::dev_t,
    path: std::path::PathBuf,
    fd: DrmDeviceFd,
    drm: DrmDevice,
}

pub struct UdevData {
    pub session: LibSeatSession,
    libinput: Libinput,
    devices: Vec<Device>,
    /// The main screen first (D-051).
    pub heads: Vec<Head>,
    policy: RendererPolicy,
    gpu_on: bool,
    gpu_idle_since: Option<Instant>,
    pub cursor: CursorCache,
    pub pointer_seen: bool,
    /// Mice, touchpads and the like, for Control Panel → Mouse settings.
    input_devices: Vec<smithay::reexports::input::Device>,
}

/// Pointer speed and button swap (Control Panel → Mouse) on one device.
fn apply_prefs(d: &mut smithay::reexports::input::Device, p: &retro_gfx::prefs::Prefs) {
    if d.config_accel_is_available() {
        // -2..2 → libinput's -1..1 (not the very ends: they feel broken).
        let _ = d.config_accel_set_speed(p.pointer_speed as f64 * 0.4);
    }
    if d.config_left_handed_is_available() {
        let _ = d.config_left_handed_set(p.left_handed);
    }
}

/// Settings changed: apply them to every pointing device.
pub fn apply_prefs_all(st: &mut Station) {
    let prefs = st.prefs.clone();
    if let crate::Backend::Udev(ud) = &mut st.backend {
        for d in ud.input_devices.iter_mut() {
            apply_prefs(d, &prefs);
        }
    }
}

impl UdevData {
    /// Import a client's dmabuf into every GPU renderer.
    pub fn import_dmabuf(&mut self, dmabuf: &Dmabuf) -> bool {
        let mut ok = false;
        for h in self.heads.iter_mut() {
            if let Some(Gpu::Gles { renderer, .. }) = &mut h.gpu {
                ok |= renderer.import_dmabuf(dmabuf, None).is_ok();
            }
        }
        ok
    }

    fn surface(&mut self, i: usize) -> Result<DrmSurface, String> {
        let h = &self.heads[i];
        self.devices[h.dev]
            .drm
            .create_surface(h.crtc, h.mode, &[h.connector])
            .map_err(|e| format!("surface: {e}"))
    }

    fn build_pixman(&mut self, i: usize) -> Result<Gpu, String> {
        let mut renderer = PixmanRenderer::new().map_err(|e| format!("pixman: {e:?}"))?;
        // Integer enlargement must stay pixel-sharp (the console's 1× buffer).
        let _ = renderer.upscale_filter(TextureFilter::Nearest);
        let surface = self.surface(i)?;
        let fd = self.devices[self.heads[i].dev].fd.clone();
        let compositor = dumb_compositor(&self.heads[i].output, surface, &fd, &renderer)?;
        Ok(Gpu::Pixman {
            renderer,
            compositor,
        })
    }

    fn build_gles(&mut self, i: usize) -> Result<Gpu, String> {
        let fd = self.devices[self.heads[i].dev].fd.clone();
        let (gbm, renderer, render_node) = open_gles(&fd)?;
        let formats = renderer.dmabuf_formats();
        let surface = self.surface(i)?;
        let compositor = GbmCompositor::new(
            OutputModeSource::Auto(self.heads[i].output.clone()),
            surface,
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
        )
        .map_err(|e| format!("GPU output: {e:?}"))?;
        Ok(Gpu::Gles {
            renderer: Box::new(renderer),
            compositor,
            render_node,
        })
    }

    /// (Re)build head `i`'s renderer, on the GPU if `gpu` and it can.
    fn build(&mut self, i: usize, gpu: bool) {
        // The old compositor holds the CRTC: drop it before building the new one.
        self.heads[i].gpu = None;
        self.heads[i].frame_pending = false;
        let built = if gpu && !self.heads[i].gpu_unavailable {
            self.build_gles(i).or_else(|e| {
                tracing::info!(output = %self.heads[i].output.name(), "CPU rendering: {e}");
                self.heads[i].gpu_unavailable = true;
                self.build_pixman(i)
            })
        } else {
            self.build_pixman(i)
        };
        match built {
            Ok(g) => {
                let how = match g {
                    Gpu::Gles { .. } => "GPU",
                    _ => "CPU",
                };
                tracing::info!("screen {}: {how} rendering", self.heads[i].output.name());
                self.heads[i].gpu = Some(g)
            }
            Err(e) => tracing::error!(output = %self.heads[i].output.name(), "no renderer: {e}"),
        }
    }
}

/// A GLES renderer on this DRM device, unless there is no GPU or only a
/// software one (then Pixman into dumb buffers is cheaper and simpler).
fn open_gles(
    fd: &DrmDeviceFd,
) -> Result<(GbmDevice<DrmDeviceFd>, GlesRenderer, Option<DrmNode>), String> {
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

/// A connected screen found on a device, not yet driven.
struct Found {
    dev: usize,
    connector: connector::Handle,
    crtc: crtc::Handle,
    mode: DrmMode,
    name: String,
    /// DSI, eDP or LVDS: a screen built into the machine.
    builtin: bool,
    mm: (u32, u32),
}

/// Open a DRM device (no screens chosen yet).
fn open_device(
    session: &mut LibSeatSession,
    id: libc::dev_t,
    path: &Path,
) -> Result<(Device, smithay::backend::drm::DrmDeviceNotifier), String> {
    let fd = session
        .open(
            path,
            OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
        )
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    let fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let (drm, notifier) = DrmDevice::new(fd.clone(), true).map_err(|e| format!("drm: {e}"))?;
    Ok((
        Device {
            id,
            path: path.to_path_buf(),
            fd,
            drm,
        },
        notifier,
    ))
}

/// The connected screens of device `dev` that are not driven yet, each with
/// a free CRTC; `Err` lists every connector (a DSI panel missing here means
/// its overlay was not loaded).
fn find_screens(ud: &UdevData, dev: usize) -> Result<Vec<Found>, String> {
    let d = &ud.devices[dev];
    let res = d
        .drm
        .resource_handles()
        .map_err(|e| format!("resources: {e}"))?;
    let conns: Vec<_> = res
        .connectors()
        .iter()
        .filter_map(|c| d.drm.get_connector(*c, false).ok())
        .collect();
    let mut used: Vec<crtc::Handle> = ud
        .heads
        .iter()
        .filter(|h| h.dev == dev)
        .map(|h| h.crtc)
        .collect();
    let mut found = Vec::new();
    for c in &conns {
        if c.state() != connector::State::Connected || c.modes().is_empty() {
            continue;
        }
        if ud
            .heads
            .iter()
            .any(|h| h.dev == dev && h.connector == c.handle())
        {
            continue;
        }
        let mode = c
            .modes()
            .iter()
            .find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
            .copied()
            .unwrap_or(c.modes()[0]);
        let Some(crtc) = c
            .encoders()
            .iter()
            .filter_map(|e| d.drm.get_encoder(*e).ok())
            .flat_map(|enc| res.filter_crtcs(enc.possible_crtcs()))
            .find(|crtc| !used.contains(crtc))
        else {
            tracing::warn!(
                "no free CRTC for {}-{}",
                c.interface().as_str(),
                c.interface_id()
            );
            continue;
        };
        used.push(crtc);
        found.push(Found {
            dev,
            connector: c.handle(),
            crtc,
            mode,
            name: format!("{}-{}", c.interface().as_str(), c.interface_id()),
            builtin: matches!(
                c.interface(),
                connector::Interface::DSI
                    | connector::Interface::EmbeddedDisplayPort
                    | connector::Interface::LVDS
            ),
            mm: c.size().unwrap_or((0, 0)),
        });
    }
    if found.is_empty() && ud.heads.iter().all(|h| h.dev != dev) {
        let list: Vec<String> = conns
            .iter()
            .map(|c| {
                format!(
                    "{}-{} {:?}",
                    c.interface().as_str(),
                    c.interface_id(),
                    c.state()
                )
            })
            .collect();
        return Err(format!(
            "{}: no connected display [{}]",
            d.path.display(),
            list.join(", ")
        ));
    }
    Ok(found)
}

/// The wl_output for a screen. The main one follows `rotate=` (D-042,
/// D-044); the others turn only if they are portrait panels.
fn make_output(f: &Found, main: bool, scale: Option<i32>, dh: &DisplayHandle) -> (Output, i32) {
    let output = Output::new(
        f.name.clone(),
        PhysicalProperties {
            size: (f.mm.0 as i32, f.mm.1 as i32).into(),
            subpixel: Subpixel::Unknown,
            make: "Phosphor".into(),
            model: f.name.clone(),
        },
    );
    let (w, h) = f.mode.size();
    let deg = if main {
        retro_gfx::screen::rotation(w as u32, h as u32)
    } else {
        retro_gfx::screen::rotation_for(None, w as u32, h as u32)
    };
    let transform = render::transform_for(deg);
    let turned: smithay::utils::Size<i32, smithay::utils::Physical> =
        transform.transform_size((w as i32, h as i32).into());
    // Every screen uses the main screen's scale (D-051).
    let scale = scale.unwrap_or_else(|| client_scale(turned.w, turned.h));
    let wl_mode = Mode::from(f.mode);
    output.change_current_state(
        Some(wl_mode),
        Some(transform),
        Some(Scale::Integer(scale)),
        Some((0, 0).into()),
    );
    output.set_preferred(wl_mode);
    let _global = output.create_global::<Station>(dh);
    (output, scale)
}

/// Drive screen `f` (a new head) and give it a renderer.
fn add_head(st: &mut Station, f: Found, main: bool) {
    let scale = (!main).then_some(st.scale);
    let (output, scale) = make_output(&f, main, scale, &st.dh);
    if main {
        st.scale = scale;
        st.space.map_output(&output, (0, 0));
        st.output = Some(output.clone());
        if let Some(s) = st.output_pixels() {
            st.art_scale = integer_scale(s.w, s.h);
            tracing::info!(width = s.w, height = s.h, transform = ?output.current_transform(), "picture");
        }
    } else {
        st.add_extra_output(output.clone());
    }
    tracing::info!(output = %f.name, main, "screen");
    let crate::Backend::Udev(ud) = &mut st.backend else {
        return;
    };
    ud.heads.push(Head {
        dev: f.dev,
        crtc: f.crtc,
        mode: f.mode,
        connector: f.connector,
        output,
        gpu: None,
        frame_pending: false,
        gpu_unavailable: false,
    });
    let i = ud.heads.len() - 1;
    let on = ud.gpu_on;
    ud.build(i, on);
    st.dirty = true;
}

/// A screen was unplugged: stop driving it; windows on it come back to the
/// main screen.
fn remove_head(st: &mut Station, i: usize) {
    let crate::Backend::Udev(ud) = &mut st.backend else {
        return;
    };
    let h = ud.heads.remove(i);
    tracing::info!(output = %h.output.name(), "screen unplugged");
    drop(h.gpu);
    st.remove_extra_output(&h.output);
}

/// Look at device `dev`'s connectors again (a monitor plugged in or out).
fn rescan(st: &mut Station, dev: usize) {
    let crate::Backend::Udev(ud) = &mut st.backend else {
        return;
    };
    // Gone: heads whose connector is no longer connected (never the main one).
    let gone: Vec<usize> = (1..ud.heads.len())
        .rev()
        .filter(|i| {
            let h = &ud.heads[*i];
            h.dev == dev
                && ud.devices[dev]
                    .drm
                    .get_connector(h.connector, false)
                    .map_or(true, |c| c.state() != connector::State::Connected)
        })
        .collect();
    for i in gone {
        remove_head(st, i);
    }
    let crate::Backend::Udev(ud) = &st.backend else {
        return;
    };
    let new = find_screens(ud, dev).unwrap_or_default();
    for f in new {
        add_head(st, f, false);
    }
    sync_dmabuf(st);
}

/// Switch between the CPU and GPU renderer (no-op if already there, if the
/// policy pins one, or if the GPU failed before). Creates or removes the
/// linux-dmabuf global to match, so clients only see dmabuf with a GPU.
pub fn set_gpu(st: &mut Station, on: bool) {
    let crate::Backend::Udev(ud) = &mut st.backend else {
        return;
    };
    let on = match ud.policy {
        RendererPolicy::Gpu => true,
        RendererPolicy::Cpu => false,
        RendererPolicy::Auto => on,
    };
    if ud.gpu_on == on && ud.heads.iter().all(|h| h.gpu.is_some()) {
        return;
    }
    ud.gpu_on = on;
    for i in 0..ud.heads.len() {
        let want_gles = on && !ud.heads[i].gpu_unavailable;
        let is_gles = matches!(ud.heads[i].gpu, Some(Gpu::Gles { .. }));
        if ud.heads[i].gpu.is_none() || want_gles != is_gles {
            ud.build(i, on);
        }
    }
    ud.gpu_idle_since = None;
    sync_dmabuf(st);
}

/// Offer linux-dmabuf to programs exactly when every screen draws on the
/// GPU (after a renderer switch or a screen coming or going).
fn sync_dmabuf(st: &mut Station) {
    let crate::Backend::Udev(ud) = &st.backend else {
        return;
    };
    // GPU clients: linux-dmabuf, with feedback naming the main screen's
    // render device — only when every screen draws on the GPU: a screen
    // drawn on the CPU cannot show GPU buffers, so with one of those all
    // programs keep to shared memory (D-051).
    let all_gles = ud
        .heads
        .iter()
        .all(|h| matches!(h.gpu, Some(Gpu::Gles { .. })));
    let gles = ud.heads.iter().find_map(|h| match &h.gpu {
        Some(Gpu::Gles {
            renderer,
            render_node,
            ..
        }) if all_gles => Some((renderer.dmabuf_formats(), *render_node)),
        _ => None,
    });
    match gles {
        Some((formats, render_node)) => {
            tracing::info!("renderer: GPU");
            if st.dmabuf_global.is_none() {
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
        }
        None => {
            tracing::info!("renderer: CPU");
            if let Some(g) = st.dmabuf_global.take() {
                st.dmabuf_state.destroy_global::<Station>(&st.dh, g);
            }
        }
    }
    st.dirty = true;
}

/// Drop the GPU renderer once no graphical program has run for a while.
fn gpu_idle_check(st: &mut Station) {
    let busy = st
        .tasks
        .iter()
        .any(|t| t.kind == crate::tasks::TaskKind::App)
        || st.children.iter().any(|(_, l)| l.is_some());
    let crate::Backend::Udev(ud) = &mut st.backend else {
        return;
    };
    if !ud.gpu_on || ud.policy != RendererPolicy::Auto {
        return;
    }
    if busy {
        ud.gpu_idle_since = None;
        return;
    }
    let since = *ud.gpu_idle_since.get_or_insert_with(Instant::now);
    if since.elapsed() >= GPU_IDLE {
        set_gpu(st, false);
    }
}

pub fn init(st: &mut Station) -> Result<(), String> {
    let (session, session_notifier) = LibSeatSession::new().map_err(|e| format!("libseat: {e}"))?;
    let seat = session.seat();
    let udev = UdevBackend::new(&seat).map_err(|e| format!("udev: {e}"))?;

    let mut list: Vec<(libc::dev_t, std::path::PathBuf)> = udev
        .device_list()
        .map(|(id, p)| (id, p.to_path_buf()))
        .collect();
    list.sort_by(|a, b| a.1.cmp(&b.1));
    let mut libinput =
        Libinput::new_with_udev::<LibinputSessionInterface<LibSeatSession>>(session.clone().into());
    libinput
        .udev_assign_seat(&seat)
        .map_err(|_| "libinput seat assign failed")?;
    let input_backend = LibinputInputBackend::new(libinput.clone());

    let mut ud = UdevData {
        session,
        libinput,
        devices: Vec::new(),
        heads: Vec::new(),
        policy: renderer_policy(),
        gpu_on: false,
        gpu_idle_since: None,
        cursor: CursorCache::default(),
        pointer_seen: false,
        input_devices: Vec::new(),
    };
    let h = st.loop_handle.clone();
    let mut errors = Vec::new();
    let mut found = Vec::new();
    for (id, p) in &list {
        let (dev, notifier) = match open_device(&mut ud.session, *id, p) {
            Ok(x) => x,
            Err(e) => {
                errors.push(e);
                continue;
            }
        };
        ud.devices.push(dev);
        let di = ud.devices.len() - 1;
        match find_screens(&ud, di) {
            Ok(f) => found.extend(f),
            Err(e) => errors.push(e),
        }
        // Each device's vblanks go to its own screens.
        h.insert_source(notifier, move |event, _, st: &mut Station| match event {
            DrmEvent::VBlank(crtc) => frame_finish(st, di, crtc),
            DrmEvent::Error(e) => tracing::error!("drm error: {e}"),
        })
        .map_err(|_| "drm source")?;
    }
    if found.is_empty() {
        return Err(format!("no usable display: {}", errors.join("; ")));
    }
    // The main screen: the built-in one if any, else the first (D-051).
    let main = found.iter().position(|f| f.builtin).unwrap_or(0);
    let first = found.remove(main);
    st.backend = crate::Backend::Udev(Box::new(ud));
    add_head(st, first, true);
    for f in found {
        add_head(st, f, false);
    }

    h.insert_source(input_backend, |mut event, _, st: &mut Station| {
        match &mut event {
            InputEvent::DeviceAdded { device }
                if device.has_capability(DeviceCapability::Pointer) =>
            {
                let prefs = st.prefs.clone();
                if let crate::Backend::Udev(ud) = &mut st.backend {
                    ud.pointer_seen = true;
                    apply_prefs(device, &prefs);
                    ud.input_devices.push(device.clone());
                }
            }
            InputEvent::DeviceRemoved { device } => {
                if let crate::Backend::Udev(ud) = &mut st.backend {
                    ud.input_devices.retain(|d| d != device);
                }
            }
            _ => {}
        }
        st.process_input_event(event);
    })
    .map_err(|_| "input source")?;

    // Monitors plugged in or out while running.
    h.insert_source(udev, |event, _, st: &mut Station| {
        if let UdevEvent::Changed { device_id } = event {
            let crate::Backend::Udev(ud) = &st.backend else {
                return;
            };
            if let Some(dev) = ud.devices.iter().position(|d| d.id == device_id) {
                rescan(st, dev);
            }
        }
    })
    .map_err(|_| "udev source")?;

    h.insert_source(session_notifier, |event, _, st: &mut Station| {
        let crate::Backend::Udev(ud) = &mut st.backend else {
            return;
        };
        match event {
            SessionEvent::PauseSession => {
                tracing::info!("session paused");
                ud.libinput.suspend();
                for d in ud.devices.iter_mut() {
                    d.drm.pause();
                }
            }
            SessionEvent::ActivateSession => {
                tracing::info!("session resumed");
                let _ = ud.libinput.resume();
                for d in ud.devices.iter_mut() {
                    let _ = d.drm.activate(false);
                }
                for head in ud.heads.iter_mut() {
                    if let Some(gpu) = head.gpu.as_mut() {
                        let _ =
                            with_gpu!(gpu, |_r, c| c.reset_state().map_err(|e| format!("{e:?}")));
                    }
                    head.frame_pending = false;
                }
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
    // Start on the CPU (or the GPU if pinned); see set_gpu.
    set_gpu(st, false);
    if let crate::Backend::Udev(ud) = &st.backend {
        if ud.heads.first().is_none_or(|h| h.gpu.is_none()) {
            return Err("no renderer for the display".into());
        }
    }
    h.insert_source(Timer::from_duration(Duration::from_secs(2)), |_, _, st| {
        gpu_idle_check(st);
        TimeoutAction::ToDuration(Duration::from_secs(2))
    })
    .map_err(|_| "gpu idle timer")?;
    st.dirty = true;
    render(st);
    Ok(())
}

pub fn render(st: &mut Station) {
    st.prepare_frame();
    let crate::Backend::Udev(ud) = &mut st.backend else {
        return;
    };
    if !ud.session.is_active() {
        return;
    }
    let mut drew = false;
    let mut waiting = false;
    for head in ud.heads.iter_mut() {
        if head.frame_pending {
            waiting = true;
            continue;
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
            personality: st.personality,
            desktop: &st.desktop,
            x11_overrides: &st.x11_overrides,
            origin: head.output.current_location(),
        };
        let cursor = &mut ud.cursor;
        let Some(gpu) = head.gpu.as_mut() else {
            continue;
        };
        let result = with_gpu!(gpu, |r, c| {
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
                head.frame_pending = queued;
                drew = true;
            }
            Err(e) => tracing::warn!(output = %head.output.name(), "render: {e}"),
        }
    }
    // A screen still showing its last frame draws this change at its vblank.
    if drew && !waiting {
        st.dirty = false;
    }
    crate::send_frames(st);
}

fn frame_finish(st: &mut Station, dev: usize, crtc: crtc::Handle) {
    if let crate::Backend::Udev(ud) = &mut st.backend {
        if let Some(head) = ud.heads.iter_mut().find(|h| h.dev == dev && h.crtc == crtc) {
            if let Some(gpu) = head.gpu.as_mut() {
                if let Err(e) = with_gpu!(gpu, |_r, c| c
                    .frame_submitted()
                    .map(|_| ())
                    .map_err(|e| format!("{e:?}")))
                {
                    tracing::warn!("frame_submitted: {e:?}");
                }
            }
            head.frame_pending = false;
        }
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

//! retro-ui: the RetroOS GUI toolkit (docs/DESIGN.md §9.3).
//!
//! Programs draw into 1× canvases in art pixels with retro-gfx (Lantern 12,
//! the shared widgets); retro-ui sends them as wl_shm buffers and asks
//! Station to enlarge them by whole pixels (wp_viewporter, nearest
//! neighbour), so our UI is pixel-exact at every scale and costs a quarter
//! (or less) of the memory of drawing at full resolution.
//!
//! Surfaces: bars along a screen edge (wlr-layer-shell; the System Bar),
//! popups (menus) and ordinary windows (Station draws their frames).
//!
//! A program implements [`Program`] and calls [`run`].

pub mod controls;
pub mod station;

use retro_gfx::widgets::CLEAR_PX;
use retro_gfx::{Canvas, Scheme, Theme};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState, FrameCallbackData};
use smithay_client_toolkit::data_device_manager::data_device::{DataDevice, DataDeviceHandler};
use smithay_client_toolkit::data_device_manager::data_offer::{DataOfferHandler, DragOffer};
use smithay_client_toolkit::data_device_manager::data_source::{
    CopyPasteSource, DataSourceHandler,
};
use smithay_client_toolkit::data_device_manager::{DataDeviceManagerState, WritePipe};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopHandle};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::globals::registry_queue_init;
use smithay_client_toolkit::reexports::client::protocol::{
    wl_data_device, wl_data_device_manager::DndAction, wl_data_source, wl_keyboard, wl_output,
    wl_pointer, wl_seat, wl_shm, wl_surface,
};
use smithay_client_toolkit::reexports::client::{Connection, Proxy, QueueHandle};
use smithay_client_toolkit::reexports::protocols::wp::viewporter::client::{
    wp_viewport::WpViewport, wp_viewporter::WpViewporter,
};
use smithay_client_toolkit::reexports::protocols::xdg::shell::client::xdg_positioner;
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::keyboard::{
    KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers,
};
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, PointerHandler};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
    LayerSurfaceConfigure,
};
use smithay_client_toolkit::shell::xdg::popup::{Popup, PopupConfigure, PopupHandler};
use smithay_client_toolkit::shell::xdg::window::{
    Window, WindowConfigure, WindowDecorations, WindowHandler,
};
use smithay_client_toolkit::shell::xdg::{XdgPositioner, XdgShell, XdgSurface};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{delegate_registry, registry_handlers};
use std::collections::HashMap;
use std::time::Duration;

pub use smithay_client_toolkit::shell::wlr_layer::Layer as BarLayer;

/// A surface of the program.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SurfaceId(u32);

/// A rectangle in art pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The surface got (or changed) its size, in art pixels.
    Configure {
        w: i32,
        h: i32,
    },
    PointerEnter {
        x: i32,
        y: i32,
    },
    PointerLeave,
    PointerMove {
        x: i32,
        y: i32,
    },
    /// Mouse button (evdev code, 0x110 = left) pressed or released.
    Button {
        button: u32,
        pressed: bool,
        x: i32,
        y: i32,
    },
    Key {
        sym: u32,
        text: Option<String>,
        ctrl: bool,
        alt: bool,
        shift: bool,
    },
    /// Mouse wheel (or touchpad) scroll: positive is down, in wheel steps.
    Scroll {
        steps: f64,
    },
    /// The window's close button or Alt+F4.
    Close,
    /// Station dismissed the popup (a click elsewhere).
    PopupDone,
}

/// Keysyms of the keys programs react to ([`Event::Key`] `sym`).
pub mod keys {
    pub const UP: u32 = 0xff52;
    pub const DOWN: u32 = 0xff54;
    pub const LEFT: u32 = 0xff51;
    pub const RIGHT: u32 = 0xff53;
    pub const HOME: u32 = 0xff50;
    pub const END: u32 = 0xff57;
    pub const PAGE_UP: u32 = 0xff55;
    pub const PAGE_DOWN: u32 = 0xff56;
    pub const RETURN: u32 = 0xff0d;
    pub const KP_ENTER: u32 = 0xff8d;
    pub const ESCAPE: u32 = 0xff1b;
    pub const SPACE: u32 = 0x20;
    pub const TAB: u32 = 0xff09;
    pub const ISO_LEFT_TAB: u32 = 0xfe20;
    pub const BACKSPACE: u32 = 0xff08;
    pub const DELETE: u32 = 0xffff;
    pub const INSERT: u32 = 0xff63;
    pub const F1: u32 = 0xffbe;
    pub const F3: u32 = 0xffc0;
    pub const F4: u32 = 0xffc1;
    pub const F6: u32 = 0xffc3;
    pub const F10: u32 = 0xffc7;
}

/// What a program does: draw its surfaces and react to events.
pub trait Program {
    /// Called once, when the toolkit is ready: create the first surfaces.
    fn start(&mut self, ui: &mut Ui);
    /// Draw a surface into `c` (its size in art pixels; starts transparent).
    fn draw(&mut self, ui: &Ui, id: SurfaceId, c: &mut Canvas);
    fn event(&mut self, ui: &mut Ui, id: SurfaceId, ev: Event);
    /// A timer set with [`Ui::timer`] fired.
    fn timer(&mut self, _ui: &mut Ui, _token: u32) {}
    /// The system's colour scheme changed ([`Ui::theme`] has the new one;
    /// every surface is redrawn anyway).
    fn theme_changed(&mut self, _ui: &mut Ui) {}
}

enum Role {
    Bar(LayerSurface),
    Popup(Popup),
    Window(Window),
}

struct Surf {
    role: Role,
    viewport: Option<WpViewport>,
    /// Size in art pixels, and on screen (logical pixels).
    art: (i32, i32),
    logical: (i32, i32),
    configured: bool,
    dirty: bool,
    frame_pending: bool,
    buffer: Option<Buffer>,
}

impl Surf {
    fn wl_surface(&self) -> &wl_surface::WlSurface {
        match &self.role {
            Role::Bar(l) => l.wl_surface(),
            Role::Popup(p) => p.wl_surface(),
            Role::Window(w) => w.wl_surface(),
        }
    }
}

/// The toolkit side: surfaces, scales, the event loop.
pub struct Ui {
    qh: QueueHandle<State>,
    loop_handle: LoopHandle<'static, State>,
    compositor: CompositorState,
    xdg: XdgShell,
    layer_shell: Option<LayerShell>,
    shm: Shm,
    pool: SlotPool,
    viewporter: Option<WpViewporter>,
    registry: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    seat: Option<wl_seat::WlSeat>,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    surfaces: HashMap<SurfaceId, Surf>,
    next_id: u32,
    /// Serial of the last button press (popup grabs need it).
    last_serial: u32,
    /// The surface with the keyboard.
    kb_focus: Option<SurfaceId>,
    mods: Modifiers,
    /// Physical pixels per art pixel, and the output scale.
    pub art_scale: i32,
    pub output_scale: i32,
    /// Output size in art pixels.
    pub screen: (i32, i32),
    /// Colours of the system's scheme (followed live from Station).
    pub theme: Theme,
    quit: bool,
    conn: Connection,
    data_manager: Option<DataDeviceManagerState>,
    data_device: Option<DataDevice>,
    /// The clipboard while we own it, and what is on it.
    copy_source: Option<CopyPasteSource>,
    copied: String,
}

/// Text types we offer and accept on the clipboard, preferred first.
const TEXT_MIMES: [&str; 5] = [
    "text/plain;charset=utf-8",
    "UTF8_STRING",
    "text/plain",
    "TEXT",
    "STRING",
];

impl Ui {
    /// Logical pixels per art pixel.
    fn f(&self) -> f64 {
        self.art_scale as f64 / self.output_scale.max(1) as f64
    }

    fn logical(&self, v: i32) -> i32 {
        (v as f64 * self.f()).round() as i32
    }

    fn add(&mut self, role: Role, art: (i32, i32)) -> SurfaceId {
        self.next_id += 1;
        let id = SurfaceId(self.next_id);
        let mut s = Surf {
            role,
            viewport: None,
            art,
            logical: (self.logical(art.0), self.logical(art.1)),
            configured: false,
            dirty: true,
            frame_pending: false,
            buffer: None,
        };
        s.viewport = self
            .viewporter
            .as_ref()
            .map(|v| v.get_viewport(s.wl_surface(), &self.qh, NoEvents));
        self.surfaces.insert(id, s);
        id
    }

    /// A bar along the top of the screen, `height` art pixels, that windows
    /// keep clear of (the System Bar).
    pub fn bar_top(&mut self, height: i32, namespace: &str) -> Option<SurfaceId> {
        let shell = self.layer_shell.as_ref()?;
        let surface = self.compositor.create_surface(&self.qh);
        let layer =
            shell.create_layer_surface(&self.qh, surface, Layer::Top, Some(namespace), None);
        let lh = self.logical(height);
        layer.set_anchor(Anchor::TOP | Anchor::LEFT | Anchor::RIGHT);
        layer.set_size(0, lh as u32);
        layer.set_exclusive_zone(lh);
        layer.set_keyboard_interactivity(KeyboardInteractivity::OnDemand);
        layer.commit();
        Some(self.add(Role::Bar(layer), (self.screen.0, height)))
    }

    /// A popup (menu) `size` art pixels, below `anchor` (art pixels in
    /// `parent`), or beside it with `beside` (submenus). With `grab`, a
    /// click elsewhere closes it and it gets the keyboard.
    pub fn popup(
        &mut self,
        parent: SurfaceId,
        anchor: Rect,
        size: (i32, i32),
        beside: bool,
        grab: bool,
    ) -> Option<SurfaceId> {
        let positioner = XdgPositioner::new(&self.xdg).ok()?;
        positioner.set_size(self.logical(size.0).max(1), self.logical(size.1).max(1));
        positioner.set_anchor_rect(
            self.logical(anchor.x),
            self.logical(anchor.y),
            self.logical(anchor.w).max(1),
            self.logical(anchor.h).max(1),
        );
        use xdg_positioner::{Anchor as A, ConstraintAdjustment as C, Gravity as G};
        if beside {
            positioner.set_anchor(A::TopRight);
        } else {
            positioner.set_anchor(A::BottomLeft);
        }
        positioner.set_gravity(G::BottomRight);
        positioner.set_constraint_adjustment(C::SlideX | C::SlideY | C::FlipX | C::FlipY);
        let p = self.surfaces.get(&parent)?;
        let parent_xdg = match &p.role {
            Role::Popup(pp) => Some(pp.xdg_surface().clone()),
            Role::Window(w) => Some(w.xdg_surface().clone()),
            Role::Bar(_) => None,
        };
        let surface = self.compositor.create_surface(&self.qh);
        let popup = Popup::from_surface(
            parent_xdg.as_ref(),
            &positioner,
            &self.qh,
            surface,
            &self.xdg,
        )
        .ok()?;
        if let Role::Bar(l) = &p.role {
            l.get_popup(popup.xdg_popup());
        }
        if grab {
            if let Some(seat) = &self.seat {
                popup.xdg_popup().grab(seat, self.last_serial);
            }
        }
        popup.wl_surface().commit();
        Some(self.add(Role::Popup(popup), size))
    }

    /// An ordinary window (Station draws its frame); `fixed`: not resizable.
    pub fn window(
        &mut self,
        title: &str,
        app_id: &str,
        size: (i32, i32),
        fixed: bool,
    ) -> SurfaceId {
        let surface = self.compositor.create_surface(&self.qh);
        let w = self
            .xdg
            .create_window(surface, WindowDecorations::RequestServer, &self.qh);
        w.set_title(title);
        w.set_app_id(app_id);
        if fixed {
            let l = (self.logical(size.0) as u32, self.logical(size.1) as u32);
            w.set_min_size(Some(l));
            w.set_max_size(Some(l));
        }
        w.commit();
        self.add(Role::Window(w), size)
    }

    /// A dialog of window `parent` (Station centres it over its parent);
    /// not resizable.
    pub fn dialog(&mut self, parent: SurfaceId, title: &str, size: (i32, i32)) -> SurfaceId {
        let id = self.window(title, "retroos.dialog", size, true);
        let parent_win = match self.surfaces.get(&parent).map(|s| &s.role) {
            Some(Role::Window(w)) => Some(w.clone()),
            _ => None,
        };
        if let (Some(Role::Window(w)), Some(p)) =
            (self.surfaces.get(&id).map(|s| &s.role), parent_win)
        {
            w.set_parent(Some(&p));
            w.commit();
        }
        id
    }

    /// Is Shift held (for Shift+click)?
    pub fn shift(&self) -> bool {
        self.mods.shift
    }

    /// Change a window's title.
    pub fn set_title(&mut self, id: SurfaceId, title: &str) {
        if let Some(Surf {
            role: Role::Window(w),
            ..
        }) = self.surfaces.get(&id)
        {
            w.set_title(title);
        }
    }

    /// Ask Station to minimise a window (Program Manager's Minimize on Use).
    pub fn minimize(&mut self, id: SurfaceId) {
        if let Some(Surf {
            role: Role::Window(w),
            ..
        }) = self.surfaces.get(&id)
        {
            w.set_minimized();
        }
    }

    /// Put text on the clipboard (Cut, Copy).
    pub fn set_clipboard(&mut self, text: &str) {
        self.copied = text.to_string();
        let (Some(m), Some(d)) = (&self.data_manager, &self.data_device) else {
            return;
        };
        let src = m.create_copy_paste_source(&self.qh, TEXT_MIMES);
        src.set_selection(d, self.last_serial);
        self.copy_source = Some(src);
    }

    /// The text on the clipboard (Paste): ours, or another program's
    /// (read with a two-second limit, so a stuck program cannot hang us).
    pub fn clipboard(&mut self) -> Option<String> {
        if self.copy_source.is_some() || self.data_device.is_none() {
            return (!self.copied.is_empty()).then(|| self.copied.clone());
        }
        let offer = self.data_device.as_ref()?.data().selection_offer()?;
        let mime = offer.with_mime_types(|have| {
            TEXT_MIMES
                .iter()
                .find(|m| have.iter().any(|h| h == *m))
                .map(|m| m.to_string())
        })?;
        let pipe = offer.receive(mime).ok()?;
        let _ = self.conn.flush();
        read_with_timeout(pipe, Duration::from_secs(2))
    }

    pub fn close(&mut self, id: SurfaceId) {
        self.surfaces.remove(&id);
    }

    pub fn exists(&self, id: SurfaceId) -> bool {
        self.surfaces.contains_key(&id)
    }

    /// Draw the surface again at the next frame.
    pub fn redraw(&mut self, id: SurfaceId) {
        if let Some(s) = self.surfaces.get_mut(&id) {
            s.dirty = true;
        }
    }

    pub fn size(&self, id: SurfaceId) -> (i32, i32) {
        self.surfaces.get(&id).map(|s| s.art).unwrap_or_default()
    }

    /// Call [`Program::timer`] with `token` after `after`.
    pub fn timer(&mut self, token: u32, after: Duration) {
        let _ = self.loop_handle.insert_source(
            Timer::from_duration(after),
            move |_, _, st: &mut State| {
                let State { ui, program } = st;
                program.timer(ui, token);
                TimeoutAction::Drop
            },
        );
    }

    pub fn quit(&mut self) {
        self.quit = true;
    }

    fn id_of(&self, s: &wl_surface::WlSurface) -> Option<SurfaceId> {
        self.surfaces
            .iter()
            .find(|(_, v)| v.wl_surface() == s)
            .map(|(k, _)| *k)
    }

    /// The art-pixel position of a surface-local logical position.
    fn art_pos(&self, p: (f64, f64)) -> (i32, i32) {
        let f = self.f();
        ((p.0 / f).floor() as i32, (p.1 / f).floor() as i32)
    }
}

struct State {
    ui: Ui,
    program: Box<dyn Program>,
}

impl State {
    fn key_event(&mut self, ev: KeyEvent) {
        let Some(id) = self.ui.kb_focus.filter(|id| self.ui.exists(*id)) else {
            return;
        };
        let m = self.ui.mods;
        self.event(
            id,
            Event::Key {
                sym: ev.keysym.raw(),
                text: ev.utf8,
                ctrl: m.ctrl,
                alt: m.alt,
                shift: m.shift,
            },
        );
    }

    fn event(&mut self, id: SurfaceId, ev: Event) {
        let State { ui, program } = self;
        program.event(ui, id, ev);
    }

    /// Draw every surface that changed and is ready for a frame.
    fn render(&mut self) {
        let ids: Vec<SurfaceId> = self
            .ui
            .surfaces
            .iter()
            .filter(|(_, s)| s.configured && s.dirty && !s.frame_pending)
            .map(|(k, _)| *k)
            .collect();
        for id in ids {
            let (w, h) = self.ui.surfaces[&id].art;
            if w <= 0 || h <= 0 {
                continue;
            }
            let mut canvas = Canvas::new(w as u32, h as u32, CLEAR_PX);
            {
                let State { ui, program } = self;
                program.draw(ui, id, &mut canvas);
            }
            let ui = &mut self.ui;
            let Ok((buffer, data)) = ui.pool.create_buffer(w, h, w * 4, wl_shm::Format::Argb8888)
            else {
                continue;
            };
            for (dst, &v) in data.chunks_exact_mut(4).zip(canvas.px.iter()) {
                let argb = if v == CLEAR_PX { 0 } else { v | 0xFF00_0000 };
                dst.copy_from_slice(&argb.to_le_bytes());
            }
            let qh = ui.qh.clone();
            let s = ui.surfaces.get_mut(&id).unwrap();
            let surface = s.wl_surface().clone();
            match &s.viewport {
                Some(vp) => {
                    surface.set_buffer_scale(1);
                    vp.set_source(0.0, 0.0, w as f64, h as f64);
                    vp.set_destination(s.logical.0.max(1), s.logical.1.max(1));
                }
                None => surface.set_buffer_scale(1),
            }
            if buffer.attach_to(&surface).is_err() {
                continue;
            }
            surface.damage_buffer(0, 0, w, h);
            surface.frame(&qh, FrameCallbackData(surface.clone()));
            surface.commit();
            s.buffer = Some(buffer);
            s.dirty = false;
            s.frame_pending = true;
        }
    }
}

/// Pixel scale for our art on an output: the largest that keeps a 960×540
/// grid (the same rule as Station's frames).
fn read_with_timeout(
    pipe: impl std::os::fd::AsRawFd + std::io::Read,
    limit: Duration,
) -> Option<String> {
    let fd = pipe.as_raw_fd();
    // SAFETY: fcntl and poll on a file descriptor we own for this call.
    unsafe {
        let fl = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, fl | libc::O_NONBLOCK);
    }
    let end = std::time::Instant::now() + limit;
    let mut pipe = pipe;
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match pipe.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                let left = end.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() {
                    return None;
                }
                let mut p = libc::pollfd {
                    fd,
                    events: libc::POLLIN,
                    revents: 0,
                };
                // SAFETY: one valid pollfd.
                unsafe { libc::poll(&mut p, 1, left.as_millis() as i32) };
            }
            Err(_) => return None,
        }
    }
    Some(String::from_utf8_lossy(&out).replace("\r\n", "\n"))
}

/// The picture's size on an output whose mode is `dims`: a turned output (a
/// portrait panel shown landscape, D-042) reports its panel's own mode.
pub fn turned(dims: (i32, i32), t: wl_output::Transform) -> (i32, i32) {
    use wl_output::Transform as T;
    match t {
        T::_90 | T::_270 | T::Flipped90 | T::Flipped270 => (dims.1, dims.0),
        _ => dims,
    }
}

pub fn art_scale(w: i32, h: i32) -> i32 {
    (w / 960).min(h / 540).clamp(1, 8)
}

/// Connect to Station and run `program` until it quits.
pub fn run(program: Box<dyn Program>) -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|e| format!("no Wayland display ({e})"))?;
    let (globals, mut queue) = registry_queue_init(&conn).map_err(|e| format!("registry: {e}"))?;
    let qh = queue.handle();
    let mut event_loop: EventLoop<State> = EventLoop::try_new().map_err(|e| e.to_string())?;
    let compositor =
        CompositorState::bind(&globals, &qh).map_err(|e| format!("wl_compositor: {e}"))?;
    let xdg = XdgShell::bind(&globals, &qh).map_err(|e| format!("xdg_wm_base: {e}"))?;
    let shm = Shm::bind(&globals, &qh).map_err(|e| format!("wl_shm: {e}"))?;
    let pool = SlotPool::new(1 << 18, &shm).map_err(|e| format!("shm pool: {e}"))?;
    let ui = Ui {
        qh: qh.clone(),
        loop_handle: event_loop.handle(),
        layer_shell: LayerShell::bind(&globals, &qh).ok(),
        viewporter: globals
            .bind::<WpViewporter, _, _>(&qh, 1..=1, NoEvents)
            .ok(),
        registry: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        compositor,
        xdg,
        shm,
        pool,
        seat: None,
        pointer: None,
        keyboard: None,
        surfaces: HashMap::new(),
        next_id: 0,
        last_serial: 0,
        kb_focus: None,
        mods: Modifiers::default(),
        art_scale: 1,
        output_scale: 1,
        screen: (960, 540),
        theme: Theme::system(),
        quit: false,
        conn: conn.clone(),
        data_manager: DataDeviceManagerState::bind(&globals, &qh).ok(),
        data_device: None,
        copy_source: None,
        copied: String::new(),
    };
    let mut st = State { ui, program };
    // Learn the output's size and scale before anything is drawn.
    queue.roundtrip(&mut st).map_err(|e| e.to_string())?;
    queue.roundtrip(&mut st).map_err(|e| e.to_string())?;
    if let Some(info) = st
        .ui
        .output_state
        .outputs()
        .next()
        .and_then(|o| st.ui.output_state.info(&o))
    {
        let mode = info.modes.iter().find(|m| m.current).or(info.modes.first());
        if let Some(m) = mode {
            let (w, h) = turned(m.dimensions, info.transform);
            st.ui.art_scale = art_scale(w, h);
            st.ui.output_scale = info.scale_factor.max(1);
            st.ui.screen = (w / st.ui.art_scale, h / st.ui.art_scale);
        }
    }
    WaylandSource::new(conn.clone(), queue)
        .insert(event_loop.handle())
        .map_err(|e| format!("wayland source: {e}"))?;
    station::watch(&event_loop.handle(), |st: &mut State, f: &[&str]| {
        if let ["SCHEME", key] = f {
            let theme = Theme::for_scheme(Scheme::by_key(key).unwrap_or(&Scheme::CLASSIC));
            if theme != st.ui.theme {
                st.ui.theme = theme;
                for s in st.ui.surfaces.values_mut() {
                    s.dirty = true;
                }
                let State { ui, program } = st;
                program.theme_changed(ui);
            }
        }
    });
    {
        let State { ui, program } = &mut st;
        program.start(ui);
    }
    while !st.ui.quit {
        st.render();
        let _ = conn.flush();
        event_loop
            .dispatch(Duration::from_millis(500), &mut st)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ------------------------------------------------------------- handlers

impl CompositorHandler for State {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, s: &wl_surface::WlSurface, _: u32) {
        if let Some(id) = self.ui.id_of(s) {
            if let Some(v) = self.ui.surfaces.get_mut(&id) {
                v.frame_pending = false;
            }
        }
    }
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for State {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.ui.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for State {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        if let Some(id) = self.ui.id_of(layer.wl_surface()) {
            self.event(id, Event::Close);
        }
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        cfg: LayerSurfaceConfigure,
        _: u32,
    ) {
        let Some(id) = self.ui.id_of(layer.wl_surface()) else {
            return;
        };
        let f = self.ui.f();
        let s = self.ui.surfaces.get_mut(&id).unwrap();
        if cfg.new_size.0 > 0 {
            s.logical.0 = cfg.new_size.0 as i32;
        }
        if cfg.new_size.1 > 0 {
            s.logical.1 = cfg.new_size.1 as i32;
        }
        s.art = (
            (s.logical.0 as f64 / f).round() as i32,
            (s.logical.1 as f64 / f).round() as i32,
        );
        s.configured = true;
        s.dirty = true;
        let (w, h) = s.art;
        self.event(id, Event::Configure { w, h });
    }
}

impl PopupHandler for State {
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        popup: &Popup,
        cfg: PopupConfigure,
    ) {
        let Some(id) = self.ui.id_of(popup.wl_surface()) else {
            return;
        };
        let f = self.ui.f();
        let s = self.ui.surfaces.get_mut(&id).unwrap();
        if cfg.width > 0 && cfg.height > 0 {
            s.logical = (cfg.width, cfg.height);
            s.art = (
                (cfg.width as f64 / f).round() as i32,
                (cfg.height as f64 / f).round() as i32,
            );
        }
        s.configured = true;
        s.dirty = true;
    }

    fn done(&mut self, _: &Connection, _: &QueueHandle<Self>, popup: &Popup) {
        if let Some(id) = self.ui.id_of(popup.wl_surface()) {
            self.ui.surfaces.remove(&id);
            self.event(id, Event::PopupDone);
        }
    }
}

impl WindowHandler for State {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, w: &Window) {
        if let Some(id) = self.ui.id_of(w.wl_surface()) {
            self.event(id, Event::Close);
        }
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        w: &Window,
        cfg: WindowConfigure,
        _: u32,
    ) {
        let Some(id) = self.ui.id_of(w.wl_surface()) else {
            return;
        };
        let f = self.ui.f();
        let s = self.ui.surfaces.get_mut(&id).unwrap();
        if let (Some(cw), Some(ch)) = cfg.new_size {
            s.logical = (cw.get() as i32, ch.get() as i32);
            s.art = (
                (s.logical.0 as f64 / f).round() as i32,
                (s.logical.1 as f64 / f).round() as i32,
            );
        }
        let first = !s.configured;
        s.configured = true;
        s.dirty = true;
        // A configure needs a commit in reply, even when nothing changed.
        s.frame_pending = false;
        let (aw, ah) = s.art;
        if first || cfg.new_size.0.is_some() {
            self.event(id, Event::Configure { w: aw, h: ah });
        }
    }
}

impl SeatHandler for State {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.ui.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        self.ui.seat.get_or_insert(seat);
    }
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        cap: Capability,
    ) {
        self.ui.seat.get_or_insert(seat.clone());
        if self.ui.data_device.is_none() {
            if let Some(m) = &self.ui.data_manager {
                self.ui.data_device = Some(m.get_data_device(qh, &seat));
            }
        }
        if cap == Capability::Keyboard && self.ui.keyboard.is_none() {
            // Held keys repeat (the compositor's rate and delay).
            let lh = self.ui.loop_handle.clone();
            self.ui.keyboard = self
                .ui
                .seat_state
                .get_keyboard_with_repeat(
                    qh,
                    &seat,
                    None,
                    lh,
                    Box::new(|st: &mut State, _, ev| st.key_event(ev)),
                )
                .ok();
        }
        if cap == Capability::Pointer && self.ui.pointer.is_none() {
            self.ui.pointer = self.ui.seat_state.get_pointer(qh, &seat).ok();
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        cap: Capability,
    ) {
        if cap == Capability::Keyboard {
            if let Some(k) = self.ui.keyboard.take() {
                k.release();
            }
        }
        if cap == Capability::Pointer {
            if let Some(p) = self.ui.pointer.take() {
                p.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for State {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for e in events {
            let Some(id) = self.ui.id_of(&e.surface) else {
                continue;
            };
            let (x, y) = self.ui.art_pos(e.position);
            let ev = match e.kind {
                PointerEventKind::Enter { .. } => Event::PointerEnter { x, y },
                PointerEventKind::Leave { .. } => Event::PointerLeave,
                PointerEventKind::Motion { .. } => Event::PointerMove { x, y },
                PointerEventKind::Press { button, serial, .. } => {
                    self.ui.last_serial = serial;
                    Event::Button {
                        button,
                        pressed: true,
                        x,
                        y,
                    }
                }
                PointerEventKind::Release { button, .. } => Event::Button {
                    button,
                    pressed: false,
                    x,
                    y,
                },
                PointerEventKind::Axis { vertical, .. } => {
                    // Wheel steps (120 each), or pixels from a touchpad.
                    let steps = if vertical.value120 != 0 {
                        vertical.value120 as f64 / 120.0
                    } else if vertical.discrete != 0 {
                        vertical.discrete as f64
                    } else {
                        vertical.absolute / 10.0
                    };
                    if steps == 0.0 {
                        continue;
                    }
                    Event::Scroll { steps }
                }
            };
            self.event(id, ev);
        }
    }
}

impl KeyboardHandler for State {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        s: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        self.ui.kb_focus = self.ui.id_of(s);
    }
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        s: &wl_surface::WlSurface,
        _: u32,
    ) {
        if self.ui.kb_focus == self.ui.id_of(s) {
            self.ui.kb_focus = None;
        }
    }
    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        serial: u32,
        ev: KeyEvent,
    ) {
        // Setting the clipboard needs the serial of a recent input event.
        self.ui.last_serial = serial;
        self.key_event(ev);
    }
    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }
    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }
    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        m: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.ui.mods = m;
    }
}

impl ShmHandler for State {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.ui.shm
    }
}

impl ProvidesRegistryState for State {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.ui.registry
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(State);

// Clipboard. Drag and drop between programs is not used yet.
impl DataDeviceHandler for State {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
        _: f64,
        _: f64,
        _: &wl_surface::WlSurface,
    ) {
    }
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_data_device::WlDataDevice) {}
    fn motion(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
        _: f64,
        _: f64,
    ) {
    }
    fn selection(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
    ) {
    }
    fn drop_performed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_device::WlDataDevice,
    ) {
    }
}

impl DataOfferHandler for State {
    fn source_actions(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut DragOffer,
        _: DndAction,
    ) {
    }
    fn selected_action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &mut DragOffer,
        _: DndAction,
    ) {
    }
}

impl DataSourceHandler for State {
    fn accept_mime(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_source::WlDataSource,
        _: Option<String>,
    ) {
    }
    fn send_request(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_source::WlDataSource,
        _: String,
        mut fd: WritePipe,
    ) {
        use std::io::Write;
        let _ = fd.write_all(self.ui.copied.as_bytes());
    }
    fn cancelled(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        source: &wl_data_source::WlDataSource,
    ) {
        // Another program took the clipboard.
        if self
            .ui
            .copy_source
            .as_ref()
            .is_some_and(|c| c.inner() == source)
        {
            self.ui.copy_source = None;
        }
    }
    fn dnd_dropped(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_source::WlDataSource,
    ) {
    }
    fn dnd_finished(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_source::WlDataSource,
    ) {
    }
    fn action(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_data_source::WlDataSource,
        _: DndAction,
    ) {
    }
}

/// User data for objects whose events we ignore (viewporter).
struct NoEvents;

impl<I: Proxy> smithay_client_toolkit::dispatch2::Dispatch2<I, State> for NoEvents {
    fn event(&self, _: &mut State, _: &I, _: I::Event, _: &Connection, _: &QueueHandle<State>) {}
}
smithay_client_toolkit::delegate_dispatch2!(State);

#[cfg(test)]
mod tests {
    use super::{art_scale, turned};
    use smithay_client_toolkit::reexports::client::protocol::wl_output::Transform;

    #[test]
    fn a_turned_panel_is_measured_the_way_it_is_shown() {
        // A 10.1" Touch Display 2 (1200×1920 portrait panel) shown landscape:
        // the bar and windows must use the same 2× as Station's frames.
        let d = turned((1200, 1920), Transform::_270);
        assert_eq!(d, (1920, 1200));
        assert_eq!(art_scale(d.0, d.1), 2);
        assert_eq!(turned((1920, 1080), Transform::Normal), (1920, 1080));
        assert_eq!(turned((720, 1280), Transform::_90), (1280, 720));
    }
}

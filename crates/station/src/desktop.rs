//! The Desktop personality (docs/DESIGN.md §6.2): a stacking window manager
//! with Windows 3.1 frames drawn by Station.
//!
//!   click             focus and raise; title bar drag moves, border drag
//!                     resizes (an outline follows the mouse, the window
//!                     moves when the button is released)
//!   control-menu box  click: window menu; double-click: close
//!   ▼ ▲               minimise to an icon on the desktop / maximise, restore
//!   Alt+Tab           cycle windows (hold Alt; Shift goes back, Esc cancels)
//!   Alt+Esc           send the window to the back
//!   Alt+Space         window menu      Alt+F4  close
//!   Alt+Enter         DOS Prompt full screen and back
//!   Ctrl+Alt+F1       Exit to DOS (the RetroDOS personality)
//!
//! Windows keep their Desktop state (`Place`) while Station is in RetroDOS,
//! so switching back and forth puts them where they were.

use crate::chrome::{self, IconArt, MenuItem, Metrics, Part, TitleStyle, BOTTOM, LEFT, RIGHT, TOP};
use crate::render::CursorShape;
use crate::state::{Personality, Station};
use crate::tasks::{Task, TaskKind};
use retro_gfx::Rgb;
use smithay::backend::renderer::element::memory::MemoryRenderBuffer;
use smithay::backend::renderer::element::Id;
use smithay::desktop::WindowSurfaceType;
use smithay::input::keyboard::keysyms;
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode;
use smithay::utils::{Logical, Point, Rectangle, Size, SERIAL_COUNTER};
use smithay::wayland::compositor::with_states;
use smithay::wayland::shell::xdg::SurfaceCachedState;
use std::collections::HashMap;

/// Two clicks closer together than this are a double-click.
const DOUBLE_CLICK_MS: u32 = 500;
/// The mouse must move this far (logical px) before a title-bar drag moves.
const DRAG_START: f64 = 3.0;

/// One thing to draw, front to back (built by `desktop_prepare`).
pub enum DrawItem {
    /// A task's window (surface and popups) at its client-area location.
    Window(u64, Point<i32, Logical>),
    /// A layer surface (System Bar, desktop icons) and its popups.
    Layer(smithay::desktop::LayerSurface, Point<i32, Logical>),
    Solid(Id, Rectangle<i32, Logical>, Rgb),
    /// A buffer; with an Id when the same buffer is drawn more than once.
    Buffer(MemoryRenderBuffer, Point<i32, Logical>, Option<Id>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragKind {
    Move,
    /// The edges being moved (0: keyboard sizing, before an arrow picks one).
    Resize(u8),
}

pub struct Drag {
    pub id: u64,
    pub kind: DragKind,
    /// Where the mouse started (None: Move or Size from the window menu).
    start: Option<Point<f64, Logical>>,
    from: Rectangle<i32, Logical>,
    /// The outline (a frame rectangle).
    pub to: Rectangle<i32, Logical>,
    pub shown: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuCmd {
    Restore,
    Move,
    Size,
    Minimize,
    Maximize,
    Close,
    SwitchTo,
}

pub struct WinMenu {
    pub id: u64,
    items: Vec<(MenuItem, MenuCmd)>,
    sel: Option<usize>,
    at: Point<i32, Logical>,
    buffer: Option<MemoryRenderBuffer>,
    /// Opened by a button press not yet released: releasing over an item picks it.
    held: bool,
}

pub struct Switcher {
    ids: Vec<u64>,
    index: usize,
    back: Option<u64>,
    buffer: Option<(MemoryRenderBuffer, Size<i32, Logical>)>,
}

/// A drawn message box: the box, where it is, and its OK button.
type MessageBox = (
    MemoryRenderBuffer,
    Point<i32, Logical>,
    Rectangle<i32, Logical>,
);

pub struct Message {
    title: String,
    lines: Vec<String>,
    back: Option<u64>,
    buffer: Option<MessageBox>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Window(u64, Part),
    Icon(u64),
    Desktop,
}

/// What a drawn icon shows: label, selected, position, metrics.
type IconKey = (String, bool, Point<i32, Logical>, Metrics);

struct Decor {
    ids: Vec<Id>,
    title: Option<((i32, TitleStyle, Metrics), MemoryRenderBuffer)>,
}

#[derive(Default)]
pub struct Desktop {
    pub drag: Option<Drag>,
    pub menu: Option<WinMenu>,
    pub switcher: Option<Switcher>,
    pub message: Option<Message>,
    /// A title-bar button held down, and whether the mouse is still on it.
    pressed: Option<(u64, Part, bool)>,
    /// A press Station handled; its release is Station's too.
    grab: bool,
    /// Cursor over Station's own parts (None: the program's cursor).
    pub cursor: Option<CursorShape>,
    last_click: Option<(Hit, u32)>,
    cascade: i32,
    decor: HashMap<u64, Decor>,
    icons: HashMap<u64, (IconKey, MemoryRenderBuffer)>,
    band: Option<((i32, i32, Metrics), MemoryRenderBuffer)>,
    band_ids: Vec<Id>,
    outline_ids: Vec<Id>,
    pub draw: Vec<DrawItem>,
    /// The System Bar process, and how often it was started.
    bar_pid: Option<u32>,
    bar_starts: u32,
}

impl Desktop {
    /// Station is holding the mouse (programs get no pointer events).
    pub fn captures_pointer(&self) -> bool {
        self.drag.is_some() || self.grab || self.menu.is_some() || self.message.is_some()
    }

    /// Station is holding the keyboard.
    pub fn captures_keys(&self) -> bool {
        self.menu.is_some()
            || self.message.is_some()
            || self.switcher.is_some()
            || self.drag.as_ref().is_some_and(|d| d.start.is_none())
    }
}

/// The xdg size limits a client asked for (0 = none).
fn size_limits(t: &Task) -> (Size<i32, Logical>, Size<i32, Logical>) {
    t.window
        .toplevel()
        .map(|top| {
            with_states(top.wl_surface(), |s| {
                let mut c = s.cached_state.get::<SurfaceCachedState>();
                let cur = c.current();
                (cur.min_size, cur.max_size)
            })
        })
        .unwrap_or_default()
}

fn fixed_size(t: &Task) -> bool {
    let (min, max) = size_limits(t);
    min.w > 0 && min == max
}

/// Clamp a client size to the client's limits and the frame's minimum.
fn clamp_size(t: &Task, s: Size<i32, Logical>, floor: Size<i32, Logical>) -> Size<i32, Logical> {
    let (min, max) = size_limits(t);
    let lim = |v: i32, lo: i32, hi: i32, fl: i32| {
        let v = v.max(lo.max(fl));
        if hi > 0 {
            v.min(hi)
        } else {
            v
        }
    };
    (
        lim(s.w, min.w, max.w, floor.w),
        lim(s.h, min.h, max.h, floor.h),
    )
        .into()
}

impl Station {
    /// Layers drawn above windows, and below them.
    pub const UPPER: [smithay::wayland::shell::wlr_layer::Layer; 2] = [
        smithay::wayland::shell::wlr_layer::Layer::Overlay,
        smithay::wayland::shell::wlr_layer::Layer::Top,
    ];
    pub const LOWER: [smithay::wayland::shell::wlr_layer::Layer; 2] = [
        smithay::wayland::shell::wlr_layer::Layer::Bottom,
        smithay::wayland::shell::wlr_layer::Layer::Background,
    ];

    pub fn metrics(&self) -> Metrics {
        Metrics::new(self.scale, self.art_scale)
    }

    /// The screen minus reserved strips (the System Bar's exclusive zone).
    pub fn work_area(&self) -> Rectangle<i32, Logical> {
        self.layer_work_area()
            .filter(|r| !r.is_empty())
            .unwrap_or_else(|| Rectangle::from_size(self.output_size()))
    }

    fn is_ssd(t: &Task) -> bool {
        t.window.toplevel().is_some_and(|top| {
            top.current_state().decoration_mode == Some(DecorationMode::ServerSide)
        })
    }

    /// Drawn on the desktop right now.
    fn visible(&self, t: &Task) -> bool {
        let root = self.tasks.root_of(t.id);
        let root_min = self.tasks.get(root).is_some_and(|r| r.place.minimized);
        t.place.placed && !t.place.minimized && !root_min && !t.window.geometry().size.is_empty()
    }

    /// The client area as shown (maximised and full-screen windows included).
    pub fn shown_rect(&self, t: &Task) -> Rectangle<i32, Logical> {
        let size = t.window.geometry().size;
        if t.place.fullscreen {
            return Rectangle::new((0, 0).into(), size);
        }
        if t.place.maximized {
            return Rectangle::new(self.maximized_rect(t).loc, size);
        }
        Rectangle::new(t.place.rect.loc, size)
    }

    /// The frame rectangle, for windows Station decorates.
    fn frame_of(&self, t: &Task) -> Option<Rectangle<i32, Logical>> {
        (Self::is_ssd(t) && !t.place.fullscreen).then(|| self.metrics().outer(self.shown_rect(t)))
    }

    /// The client area of a maximised window: the frame's border is off the
    /// screen, the title bar runs along the top of the work area (Win 3.1).
    /// Programs with their own title bar get the whole work area.
    fn maximized_rect(&self, t: &Task) -> Rectangle<i32, Logical> {
        let m = self.metrics();
        let w = self.work_area();
        if !Self::is_ssd(t) {
            return w;
        }
        let b = m.border();
        m.inner(Rectangle::new(
            (w.loc.x - b, w.loc.y - b).into(),
            (w.size.w + 2 * b, w.size.h + 2 * b).into(),
        ))
    }

    /// The largest normal client area that fits on the screen with its frame.
    fn max_client(&self) -> Size<i32, Logical> {
        let m = self.metrics();
        let w = self.work_area();
        (w.size.w - 2 * m.border(), w.size.h - m.top() - m.border()).into()
    }

    /// The DOS Prompt's natural size: 80×25 cells at our art scale.
    fn dos_size(&self) -> Size<i32, Logical> {
        let u = self.metrics().u;
        let s: Size<i32, Logical> = (640 * u, 400 * u).into();
        let max = self.max_client();
        (s.w.min(max.w), s.h.min(max.h)).into()
    }

    /// Size for a window first seen in RetroDOS: the DOS Prompt's own, or
    /// three quarters of the screen (Windows 3.1's default).
    fn default_size(&self, t: &Task) -> Size<i32, Logical> {
        if t.kind == TaskKind::Console {
            return self.dos_size();
        }
        if t.dialog {
            return t.window.geometry().size;
        }
        let max = self.max_client();
        (max.w * 3 / 4, max.h * 3 / 4).into()
    }

    /// The title shown in the frame (the console is the "DOS Prompt").
    pub fn window_title(&self, t: &Task) -> String {
        let title = self.task_title(t);
        if t.kind == TaskKind::Console && (title.is_empty() || title == "RetroDOS") {
            return "DOS Prompt".into();
        }
        if title.is_empty() {
            return t.name.clone();
        }
        title
    }

    fn icon_art(t: &Task) -> IconArt {
        if t.kind == TaskKind::Console {
            IconArt::Dos
        } else {
            IconArt::Program
        }
    }

    // --------------------------------------------------------- configure

    /// The configure the Desktop wants for a task.
    pub fn desktop_configure(&mut self, id: u64) {
        let out = self.output_size();
        let focused = self.tasks.focused() == Some(id);
        let dos = self.dos_size();
        let Some(t) = self.tasks.get(id) else { return };
        let max = self.maximized_rect(t).size;
        let Some(top) = t.window.toplevel() else {
            return;
        };
        let p = t.place.clone();
        let size = if p.fullscreen {
            Some(out)
        } else if p.maximized {
            Some(max)
        } else if let Some((_, want, _)) = p.resizing {
            Some(want.size)
        } else if p.placed && !p.rect.size.is_empty() {
            Some(p.rect.size)
        } else if t.kind == TaskKind::Console {
            Some(dos)
        } else {
            None
        };
        top.with_pending_state(|s| {
            s.size = size;
            let mut set = |st: xdg_toplevel::State, on: bool| {
                if on {
                    s.states.set(st);
                } else {
                    s.states.unset(st);
                }
            };
            set(xdg_toplevel::State::Fullscreen, p.fullscreen);
            set(xdg_toplevel::State::Maximized, p.maximized && !p.fullscreen);
            set(xdg_toplevel::State::Activated, focused && !p.minimized);
            set(xdg_toplevel::State::Suspended, p.minimized);
        });
        if top.is_initial_configure_sent() {
            top.send_pending_configure();
        }
    }

    /// Put the window where its Place says (Space location = client area).
    fn sync_location(&mut self, id: u64) {
        let Some(t) = self.tasks.get(id) else { return };
        let loc = self.shown_rect(t).loc;
        let w = t.window.clone();
        if self.space.element_location(&w) != Some(loc) {
            self.space.map_element(w, loc, false);
        }
        self.dirty = true;
    }

    /// A client committed: place new windows, follow size changes.
    pub fn desktop_commit(&mut self, id: u64) {
        let Some(t) = self.tasks.get(id) else { return };
        let geo = t.window.geometry().size;
        if geo.is_empty() {
            return;
        }
        // Still showing a full-screen buffer from RetroDOS: wait for the new one.
        let stale_fullscreen = !t.place.fullscreen
            && t.window.toplevel().is_some_and(|top| {
                top.current_state()
                    .states
                    .contains(xdg_toplevel::State::Fullscreen)
            });
        if stale_fullscreen {
            return;
        }
        if !t.place.placed {
            self.place_new(id);
            return;
        }
        let p = t.place.clone();
        if !p.maximized && !p.fullscreen {
            let mut rect = p.rect;
            let mut resizing = p.resizing;
            if let Some((edges, want, before)) = p.resizing {
                if geo != before || geo == want.size {
                    rect.loc.x = if edges & LEFT != 0 {
                        want.loc.x + want.size.w - geo.w
                    } else {
                        want.loc.x
                    };
                    rect.loc.y = if edges & TOP != 0 {
                        want.loc.y + want.size.h - geo.h
                    } else {
                        want.loc.y
                    };
                    resizing = None;
                }
            }
            rect.size = geo;
            if let Some(t) = self.tasks.get_mut(id) {
                t.place.rect = rect;
                t.place.resizing = resizing;
            }
        }
        self.sync_location(id);
    }

    /// First placement: dialogs centred over their window, others cascaded.
    fn place_new(&mut self, id: u64) {
        let m = self.metrics();
        let work = self.work_area();
        let maxc = self.max_client();
        let parent = self
            .tasks
            .parent_of(id)
            .and_then(|p| self.tasks.get(p))
            .map(|p| self.shown_rect(p));
        let Some(t) = self.tasks.get(id) else { return };
        let geo = t.window.geometry().size;
        let size: Size<i32, Logical> = (geo.w.min(maxc.w).max(1), geo.h.min(maxc.h).max(1)).into();
        let resize = size != geo;
        let loc: Point<i32, Logical> = match parent {
            Some(pr) => {
                let x = pr.loc.x + (pr.size.w - size.w) / 2;
                let y = pr.loc.y + (pr.size.h - size.h) / 2;
                let x = x.clamp(
                    work.loc.x + m.border(),
                    (work.loc.x + work.size.w - size.w - m.border()).max(work.loc.x + m.border()),
                );
                let y = y.clamp(
                    work.loc.y + m.top(),
                    (work.loc.y + work.size.h - size.h - m.border()).max(work.loc.y + m.top()),
                );
                (x, y).into()
            }
            // Fixed-size windows (alerts, About boxes) sit in the middle.
            None if fixed_size(t) => (
                work.loc.x + (work.size.w - size.w) / 2,
                work.loc.y + (work.size.h - size.h) / 2,
            )
                .into(),
            None => self.cascade_position(size),
        };
        if let Some(t) = self.tasks.get_mut(id) {
            t.place.rect = Rectangle::new(loc, size);
            t.place.placed = true;
        }
        if resize {
            self.desktop_configure(id);
        }
        self.sync_location(id);
    }

    /// Next cascade position (Windows 3.1: each new window one title bar
    /// lower and to the right; back to the corner when it would not fit).
    fn cascade_position(&mut self, size: Size<i32, Logical>) -> Point<i32, Logical> {
        let m = self.metrics();
        let work = self.work_area();
        let step = (chrome::TITLE + chrome::BORDER) * m.u;
        let at = |k: i32| -> Point<i32, Logical> {
            (
                work.loc.x + m.border() + k * step,
                work.loc.y + m.top() + k * step,
            )
                .into()
        };
        let fits = |p: Point<i32, Logical>| {
            p.x + size.w + m.border() <= work.loc.x + work.size.w
                && p.y + size.h + m.border() <= work.loc.y + work.size.h
        };
        if !fits(at(self.desktop.cascade)) {
            self.desktop.cascade = 0;
        }
        let p = at(self.desktop.cascade);
        self.desktop.cascade += 1;
        p
    }

    // ---------------------------------------------------- personality

    pub fn desktop_bar_pid(&self) -> Option<u32> {
        self.desktop.bar_pid
    }

    /// Start the System Bar (retro-bar) with the Desktop; again if it
    /// crashes, three times at most. RETROOS_NO_BAR=1 leaves it out.
    pub fn ensure_bar(&mut self) {
        if self.personality != Personality::Desktop || std::env::var_os("RETROOS_NO_BAR").is_some()
        {
            return;
        }
        let running = self
            .desktop
            .bar_pid
            .is_some_and(|pid| self.children.iter().any(|(c, _)| c.id() == pid));
        if running || self.desktop.bar_starts >= 3 {
            return;
        }
        self.desktop.bar_starts += 1;
        match self.spawn(&["retro-bar".to_string()], None, &[], None) {
            Ok(pid) => self.desktop.bar_pid = Some(pid),
            Err(e) => {
                tracing::warn!("cannot start the System Bar: {e}");
                self.desktop.bar_starts = 3;
            }
        }
    }

    /// Switch personality live: RetroDOS ↔ Desktop (docs/DESIGN.md §5.4).
    pub fn set_personality(&mut self, p: Personality) {
        if p == self.personality {
            return;
        }
        tracing::info!(?p, "personality");
        self.personality = p;
        self.desktop.drag = None;
        self.desktop.menu = None;
        self.desktop.switcher = None;
        self.desktop.message = None;
        self.desktop.pressed = None;
        self.desktop.grab = false;
        self.desktop.cursor = None;
        let ids: Vec<u64> = self.tasks.iter().map(|t| t.id).collect();
        match p {
            Personality::Desktop => {
                // Windows first seen in RetroDOS get a place now; the rest
                // go back where they were.
                for id in &ids {
                    let Some(t) = self.tasks.get(*id) else {
                        continue;
                    };
                    if t.place.placed {
                        continue;
                    }
                    let size = self.default_size(t);
                    let dialog = t.dialog;
                    let loc = if dialog {
                        let out = self.output_size();
                        ((out.w - size.w) / 2, (out.h - size.h) / 2).into()
                    } else {
                        self.cascade_position(size)
                    };
                    if let Some(t) = self.tasks.get_mut(*id) {
                        t.place.rect = Rectangle::new(loc, size);
                        t.place.placed = !size.is_empty();
                    }
                }
                for id in &ids {
                    self.desktop_configure(*id);
                    self.sync_location(*id);
                }
                if let Some(f) = self.tasks.focused() {
                    self.focus_task(f);
                }
                self.ensure_bar();
            }
            Personality::Text => {
                for t in self.tasks.iter_mut() {
                    t.place.minimized = false;
                    t.place.slot = None;
                    t.place.fullscreen = false;
                }
                for id in &ids {
                    if let Some(t) = self.tasks.get(*id) {
                        let w = t.window.clone();
                        self.space.map_element(w, (0, 0), false);
                    }
                    self.configure_task(*id);
                }
                // The DOS Prompt you were in (or the last one) fills the screen.
                let console = self.tasks.mru().into_iter().find(|id| {
                    self.tasks
                        .get(*id)
                        .is_some_and(|t| t.kind == TaskKind::Console)
                });
                if let Some(id) = console.or(self.tasks.focused()) {
                    self.focus_task(id);
                }
            }
        }
        self.dirty = true;
    }

    // --------------------------------------------------------- actions

    pub fn minimize(&mut self, id: u64) {
        let used: Vec<usize> = self.tasks.iter().filter_map(|t| t.place.slot).collect();
        let slot = (0..).find(|s| !used.contains(s)).unwrap_or(0);
        let Some(t) = self.tasks.get_mut(id) else {
            return;
        };
        if t.dialog || t.place.minimized {
            return;
        }
        t.place.minimized = true;
        t.place.slot = Some(slot);
        self.desktop.menu = None;
        self.tasks.send_to_back(id);
        let next = self
            .tasks
            .mru()
            .into_iter()
            .find(|i| self.tasks.get(*i).is_some_and(|t| self.visible(t)));
        match next {
            Some(n) => self.focus_task(n),
            None => {
                // Nothing else open: the icon stays active (Windows 3.1).
                self.tasks.set_focus(id);
                if let Some(kb) = self.seat.get_keyboard() {
                    kb.set_focus(self, None, SERIAL_COUNTER.next_serial());
                }
            }
        }
        self.desktop_configure(id);
        self.dirty = true;
    }

    /// Back to the normal state from minimised, maximised or full screen.
    pub fn restore(&mut self, id: u64) {
        let Some(t) = self.tasks.get_mut(id) else {
            return;
        };
        if t.place.minimized {
            t.place.minimized = false;
            t.place.slot = None;
        } else if t.place.fullscreen {
            t.place.fullscreen = false;
        } else {
            t.place.maximized = false;
        }
        self.desktop_configure(id);
        self.sync_location(id);
        self.focus_task(id);
    }

    pub fn maximize(&mut self, id: u64) {
        let Some(t) = self.tasks.get_mut(id) else {
            return;
        };
        if t.dialog {
            return;
        }
        t.place.minimized = false;
        t.place.slot = None;
        t.place.maximized = true;
        self.desktop_configure(id);
        self.sync_location(id);
        self.focus_task(id);
    }

    pub fn set_fullscreen(&mut self, id: u64, on: bool) {
        let Some(t) = self.tasks.get_mut(id) else {
            return;
        };
        t.place.fullscreen = on;
        if on {
            t.place.minimized = false;
            t.place.slot = None;
        }
        self.desktop_configure(id);
        self.sync_location(id);
        self.focus_task(id);
    }

    /// Ask a window to close; the main DOS Prompt explains instead.
    pub fn close_task(&mut self, id: u64) {
        let Some(t) = self.tasks.get(id) else { return };
        if t.kind == TaskKind::Console && t.pid.is_some() && t.pid == self.primary_pid {
            self.show_message(
                "DOS Prompt",
                &[
                    "This is the main RetroDOS session.",
                    "To end it, type EXIT at its prompt.",
                ],
            );
            return;
        }
        if let Some(top) = t.window.toplevel() {
            top.send_close();
        }
    }

    pub fn show_message(&mut self, title: &str, lines: &[&str]) {
        self.desktop.menu = None;
        self.desktop.message = Some(Message {
            title: title.into(),
            lines: lines.iter().map(|s| s.to_string()).collect(),
            back: self.tasks.focused(),
            buffer: None,
        });
        if let Some(kb) = self.seat.get_keyboard() {
            kb.set_focus(self, None, SERIAL_COUNTER.next_serial());
        }
        self.dirty = true;
    }

    fn close_message(&mut self) {
        if let Some(m) = self.desktop.message.take() {
            if let Some(b) = m.back.filter(|b| self.tasks.get(*b).is_some()) {
                self.focus_task(b);
            }
        }
        self.dirty = true;
    }

    /// Give the keyboard back to the focused window (after a menu or box).
    fn refocus(&mut self) {
        if let Some(f) = self.tasks.focused() {
            if self.tasks.get(f).is_some_and(|t| !t.place.minimized) {
                self.focus_task(f);
            }
        }
        self.dirty = true;
    }

    // ------------------------------------------------------ window menu

    fn menu_items(&self, id: u64) -> Vec<(MenuItem, MenuCmd)> {
        let Some(t) = self.tasks.get(id) else {
            return vec![];
        };
        let p = &t.place;
        let normal = !p.minimized && !p.maximized && !p.fullscreen;
        let mut v = Vec::new();
        if !t.dialog {
            v.push((MenuItem::new("&Restore", "", !normal), MenuCmd::Restore));
        }
        v.push((MenuItem::new("&Move", "", normal), MenuCmd::Move));
        if !t.dialog {
            v.push((
                MenuItem::new("&Size", "", normal && !fixed_size(t)),
                MenuCmd::Size,
            ));
            v.push((
                MenuItem::new("Mi&nimize", "", !p.minimized),
                MenuCmd::Minimize,
            ));
            v.push((
                MenuItem::new("Ma&ximize", "", !p.maximized && !fixed_size(t)),
                MenuCmd::Maximize,
            ));
        }
        v.push((MenuItem::separator(), MenuCmd::Close));
        v.push((MenuItem::new("&Close", "Alt+F4", true), MenuCmd::Close));
        v.push((MenuItem::separator(), MenuCmd::SwitchTo));
        v.push((MenuItem::new("S&witch To...", "", true), MenuCmd::SwitchTo));
        v
    }

    /// Open the window menu under the control-menu box (or above an icon).
    pub fn open_window_menu(&mut self, id: u64, held: bool) {
        let items = self.menu_items(id);
        if items.is_empty() {
            return;
        }
        let m = self.metrics();
        let only: Vec<MenuItem> = items.iter().map(|(i, _)| i.clone()).collect();
        let (mw, mh, _) = chrome::menu_layout(&only);
        let (mw, mh) = ((mw + 1) * m.u, (mh + 1) * m.u);
        let Some(t) = self.tasks.get(id) else { return };
        let at: Point<i32, Logical> = if t.place.minimized {
            let p = self.icon_pos(t.place.slot.unwrap_or(0));
            (p.x, p.y - mh).into()
        } else if let Some(f) = self.frame_of(t) {
            (
                f.loc.x + m.border(),
                f.loc.y + m.border() + chrome::TITLE * m.u,
            )
                .into()
        } else {
            self.shown_rect(t).loc
        };
        let out = self.output_size();
        let at = (
            at.x.clamp(0, (out.w - mw).max(0)),
            at.y.clamp(0, (out.h - mh).max(0)),
        )
            .into();
        let sel = items
            .iter()
            .position(|(i, _)| i.enabled && !i.is_separator());
        self.desktop.menu = Some(WinMenu {
            id,
            items,
            sel,
            at,
            buffer: None,
            held,
        });
        if let Some(kb) = self.seat.get_keyboard() {
            kb.set_focus(self, None, SERIAL_COUNTER.next_serial());
        }
        self.dirty = true;
    }

    fn close_menu(&mut self) {
        self.desktop.menu = None;
        self.refocus();
    }

    fn menu_run(&mut self, cmd: MenuCmd) {
        let Some(menu) = self.desktop.menu.take() else {
            return;
        };
        let id = menu.id;
        self.dirty = true;
        match cmd {
            MenuCmd::Restore => self.restore(id),
            MenuCmd::Minimize => self.minimize(id),
            MenuCmd::Maximize => self.maximize(id),
            MenuCmd::Close => {
                self.refocus();
                self.close_task(id);
            }
            MenuCmd::SwitchTo => self.open_task_list(),
            MenuCmd::Move | MenuCmd::Size => {
                let Some(f) = self.tasks.get(id).and_then(|t| self.frame_of(t)) else {
                    self.refocus();
                    return;
                };
                self.desktop.drag = Some(Drag {
                    id,
                    kind: if cmd == MenuCmd::Move {
                        DragKind::Move
                    } else {
                        DragKind::Resize(0)
                    },
                    start: None,
                    from: f,
                    to: f,
                    shown: true,
                });
            }
        }
    }

    /// A key while the window menu is open.
    pub fn menu_key(&mut self, sym: u32) {
        let Some(menu) = self.desktop.menu.as_mut() else {
            return;
        };
        let n = menu.items.len();
        let usable = |i: usize, items: &[(MenuItem, MenuCmd)]| {
            items[i].0.enabled && !items[i].0.is_separator()
        };
        let step = |menu: &mut WinMenu, d: isize| {
            let mut i = menu.sel.map(|s| s as isize).unwrap_or(-1);
            for _ in 0..n {
                i = (i + d).rem_euclid(n as isize);
                if usable(i as usize, &menu.items) {
                    menu.sel = Some(i as usize);
                    break;
                }
            }
            menu.buffer = None;
        };
        match sym {
            keysyms::KEY_Up | keysyms::KEY_KP_Up => step(menu, -1),
            keysyms::KEY_Down | keysyms::KEY_KP_Down => step(menu, 1),
            keysyms::KEY_Escape => self.close_menu(),
            keysyms::KEY_Return | keysyms::KEY_KP_Enter => {
                if let Some(cmd) = menu.sel.map(|s| menu.items[s].1) {
                    self.menu_run(cmd);
                }
            }
            _ => {
                let c = char::from_u32(sym).map(|c| c.to_ascii_lowercase());
                let hit = menu
                    .items
                    .iter()
                    .find(|(i, _)| i.enabled && c.is_some() && i.mnemonic() == c)
                    .map(|(_, cmd)| *cmd);
                if let Some(cmd) = hit {
                    self.menu_run(cmd);
                }
            }
        }
        self.dirty = true;
    }

    fn menu_item_at(&self, pos: Point<f64, Logical>) -> Option<Option<usize>> {
        let menu = self.desktop.menu.as_ref()?;
        let u = self.metrics().u as f64;
        let only: Vec<MenuItem> = menu.items.iter().map(|(i, _)| i.clone()).collect();
        let (mw, mh, _) = chrome::menu_layout(&only);
        let x = (pos.x - menu.at.x as f64) / u;
        let y = (pos.y - menu.at.y as f64) / u;
        if x < 0.0 || y < 0.0 || x >= mw as f64 || y >= mh as f64 {
            return None;
        }
        Some(chrome::menu_hit(&only, x as i32, y as i32))
    }

    // ------------------------------------------------------- Alt+Tab etc.

    /// Alt+Tab: show the box and step through windows in stacking order.
    pub fn switch_step(&mut self, forward: bool) {
        if self.desktop.switcher.is_none() {
            let ids: Vec<u64> = self
                .tasks
                .mru()
                .into_iter()
                .filter(|id| self.tasks.get(*id).is_some_and(|t| !t.dialog))
                .collect();
            if ids.is_empty() {
                return;
            }
            self.desktop.menu = None;
            self.desktop.switcher = Some(Switcher {
                ids,
                index: 0,
                back: self.tasks.focused(),
                buffer: None,
            });
            if let Some(kb) = self.seat.get_keyboard() {
                kb.set_focus(self, None, SERIAL_COUNTER.next_serial());
            }
        }
        let Some(sw) = self.desktop.switcher.as_mut() else {
            return;
        };
        let n = sw.ids.len();
        sw.index = if forward {
            (sw.index + 1) % n
        } else {
            (sw.index + n - 1) % n
        };
        sw.buffer = None;
        self.dirty = true;
    }

    /// Alt released: switch to the chosen window (restoring an icon).
    pub fn switch_finish(&mut self) {
        let Some(sw) = self.desktop.switcher.take() else {
            return;
        };
        if let Some(id) = sw
            .ids
            .get(sw.index)
            .copied()
            .filter(|id| self.tasks.get(*id).is_some())
        {
            self.focus_task(id);
        }
        self.dirty = true;
    }

    pub fn switch_cancel(&mut self) {
        if let Some(sw) = self.desktop.switcher.take() {
            if let Some(b) = sw.back.filter(|b| self.tasks.get(*b).is_some()) {
                self.focus_task(b);
            }
        }
        self.dirty = true;
    }

    /// Alt+Esc: the focused window goes to the back, the next one comes up.
    pub fn alt_esc(&mut self) {
        let Some(f) = self.tasks.focused() else {
            return;
        };
        let root = self.tasks.root_of(f);
        self.tasks.send_to_back(root);
        let next = self.tasks.mru().into_iter().find(|i| {
            *i != root
                && self
                    .tasks
                    .get(*i)
                    .is_some_and(|t| self.visible(t) && !t.dialog)
        });
        if let Some(n) = next {
            self.focus_task(n);
        } else {
            self.tasks.set_focus(root);
        }
        self.dirty = true;
    }

    // ------------------------------------------------------ move / size

    /// Begin a mouse move or resize (title bar, border, or the client's request).
    pub fn begin_drag(&mut self, id: u64, kind: DragKind) {
        let Some(f) = self
            .tasks
            .get(id)
            .and_then(|t| self.frame_of(t).or_else(|| Some(self.shown_rect(t))))
        else {
            return;
        };
        let Some(t) = self.tasks.get(id) else { return };
        if t.place.maximized || t.place.fullscreen || t.place.minimized {
            return;
        }
        self.desktop.drag = Some(Drag {
            id,
            kind,
            start: Some(self.pointer_location),
            from: f,
            to: f,
            shown: matches!(kind, DragKind::Resize(_)),
        });
        self.desktop.cursor = Some(match kind {
            DragKind::Resize(e) => CursorShape::for_edges(e),
            DragKind::Move => CursorShape::Arrow,
        });
        self.dirty = true;
    }

    /// The outline for a frame moved by `(dx, dy)`.
    fn drag_rect(&self, d: &Drag, dx: i32, dy: i32) -> Rectangle<i32, Logical> {
        let m = self.metrics();
        let work = self.work_area();
        let mut r = d.from;
        match d.kind {
            DragKind::Move => {
                // Keep part of the title bar on the screen.
                let keep = 3 * chrome::TITLE * m.u;
                r.loc.x = (r.loc.x + dx).clamp(
                    work.loc.x - r.size.w + keep,
                    work.loc.x + work.size.w - keep,
                );
                r.loc.y = (r.loc.y + dy)
                    .clamp(work.loc.y - m.border(), work.loc.y + work.size.h - m.top());
            }
            DragKind::Resize(e) => {
                let Some(t) = self.tasks.get(d.id) else {
                    return r;
                };
                let inner = m.inner(d.from);
                let (mut x0, mut y0) = (inner.loc.x, inner.loc.y);
                let (mut x1, mut y1) = (x0 + inner.size.w, y0 + inner.size.h);
                if e & LEFT != 0 {
                    x0 += dx;
                }
                if e & RIGHT != 0 {
                    x1 += dx;
                }
                if e & TOP != 0 {
                    y0 += dy;
                }
                if e & BOTTOM != 0 {
                    y1 += dy;
                }
                let want: Size<i32, Logical> = (x1 - x0, y1 - y0).into();
                let s = clamp_size(t, want, m.min_client());
                if e & LEFT != 0 {
                    x0 = x1 - s.w;
                } else {
                    x1 = x0 + s.w;
                }
                if e & TOP != 0 {
                    y0 = y1 - s.h;
                } else {
                    y1 = y0 + s.h;
                }
                r = m.outer(Rectangle::new((x0, y0).into(), (x1 - x0, y1 - y0).into()));
            }
        }
        r
    }

    fn drag_finish(&mut self, apply: bool) {
        let Some(d) = self.desktop.drag.take() else {
            return;
        };
        self.dirty = true;
        if !apply || !d.shown || d.to == d.from {
            self.refocus();
            return;
        }
        let m = self.metrics();
        let ssd = self.tasks.get(d.id).is_some_and(Self::is_ssd);
        let inner = if ssd { m.inner(d.to) } else { d.to };
        let Some(t) = self.tasks.get_mut(d.id) else {
            return;
        };
        match d.kind {
            DragKind::Move => {
                t.place.rect.loc = inner.loc;
                self.sync_location(d.id);
            }
            DragKind::Resize(e) => {
                let before = t.window.geometry().size;
                t.place.resizing = Some((e, inner, before));
                self.desktop_configure(d.id);
            }
        }
        self.refocus();
    }

    /// A key during Move or Size from the window menu.
    pub fn drag_key(&mut self, sym: u32) {
        let Some(d) = self.desktop.drag.as_mut() else {
            return;
        };
        let step = 8 * Metrics::new(self.scale, self.art_scale).u;
        let (mut dx, mut dy) = (0, 0);
        match sym {
            keysyms::KEY_Left | keysyms::KEY_KP_Left => dx = -step,
            keysyms::KEY_Right | keysyms::KEY_KP_Right => dx = step,
            keysyms::KEY_Up | keysyms::KEY_KP_Up => dy = -step,
            keysyms::KEY_Down | keysyms::KEY_KP_Down => dy = step,
            keysyms::KEY_Return | keysyms::KEY_KP_Enter => return self.drag_finish(true),
            keysyms::KEY_Escape => return self.drag_finish(false),
            _ => return,
        }
        if d.kind == DragKind::Resize(0) {
            // The first arrow picks the edge to move.
            d.kind = DragKind::Resize(match (dx.signum(), dy.signum()) {
                (-1, _) => LEFT,
                (1, _) => RIGHT,
                (_, -1) => TOP,
                _ => BOTTOM,
            });
        }
        // Accumulate relative to the start.
        let m = Metrics::new(self.scale, self.art_scale);
        let (cx, cy) = match d.kind {
            DragKind::Move => (d.to.loc.x - d.from.loc.x, d.to.loc.y - d.from.loc.y),
            DragKind::Resize(e) => {
                let (fi, ti) = (m.inner(d.from), m.inner(d.to));
                (
                    if e & LEFT != 0 {
                        ti.loc.x - fi.loc.x
                    } else {
                        ti.size.w - fi.size.w
                    },
                    if e & TOP != 0 {
                        ti.loc.y - fi.loc.y
                    } else {
                        ti.size.h - fi.size.h
                    },
                )
            }
        };
        let d = self.desktop.drag.take().unwrap();
        let to = self.drag_rect(&d, cx + dx, cy + dy);
        self.desktop.drag = Some(Drag { to, ..d });
        self.dirty = true;
    }

    // ------------------------------------------------------ mouse input

    pub fn hit_test(&self, pos: Point<f64, Logical>) -> Hit {
        let m = self.metrics();
        for id in self.tasks.z_order() {
            let Some(t) = self.tasks.get(id) else {
                continue;
            };
            if !self.visible(t) {
                continue;
            }
            let r = self.shown_rect(t);
            if t.place.fullscreen {
                return Hit::Window(id, Part::Client);
            }
            let origin = r.loc - t.window.geometry().loc;
            if t.window
                .surface_under(pos - origin.to_f64(), WindowSurfaceType::ALL)
                .is_some()
            {
                return Hit::Window(id, Part::Client);
            }
            if let Some(f) = self.frame_of(t) {
                let (x, y) = (
                    pos.x.floor() as i32 - f.loc.x,
                    pos.y.floor() as i32 - f.loc.y,
                );
                let resizable = !t.place.maximized && !fixed_size(t);
                if let Some(part) = chrome::frame_hit(
                    f.size.w,
                    f.size.h,
                    &m,
                    x,
                    y,
                    resizable,
                    !t.dialog && !fixed_size(t),
                ) {
                    return Hit::Window(id, part);
                }
            } else if r.to_f64().contains(pos) {
                return Hit::Window(id, Part::Client);
            }
        }
        for t in self.tasks.iter() {
            if let (true, Some(slot)) = (t.place.minimized, t.place.slot) {
                let p = self.icon_pos(slot);
                let r = Rectangle::<i32, Logical>::new(
                    p,
                    (chrome::ICON_W * m.u, chrome::ICON_H * m.u).into(),
                );
                if r.to_f64().contains(pos) {
                    return Hit::Icon(t.id);
                }
            }
        }
        Hit::Desktop
    }

    /// Mouse moved. Returns true when Station keeps the pointer.
    pub fn desktop_motion(&mut self, pos: Point<f64, Logical>) -> bool {
        if let Some(d) = self.desktop.drag.take() {
            if let Some(start) = d.start {
                let (dx, dy) = (pos.x - start.x, pos.y - start.y);
                let shown = d.shown || dx.abs() >= DRAG_START || dy.abs() >= DRAG_START;
                let to = self.drag_rect(&d, dx.round() as i32, dy.round() as i32);
                self.desktop.drag = Some(Drag { to, shown, ..d });
            } else {
                self.desktop.drag = Some(d);
            }
            self.dirty = true;
            return true;
        }
        if let Some(sel) = self.menu_item_at(pos) {
            if let Some(menu) = self.desktop.menu.as_mut() {
                if menu.sel != sel && sel.is_some() {
                    menu.sel = sel;
                    menu.buffer = None;
                    self.dirty = true;
                }
            }
        }
        if let Some((id, part, _)) = self.desktop.pressed {
            let over = self.hit_test(pos) == Hit::Window(id, part);
            self.desktop.pressed = Some((id, part, over));
            self.dirty = true;
            return true;
        }
        if self.desktop.captures_pointer() {
            self.desktop.cursor = Some(CursorShape::Arrow);
            return true;
        }
        let client_grab = self.seat.get_pointer().is_some_and(|p| p.is_grabbed());
        self.desktop.cursor = if client_grab {
            None
        } else if self.layer_surface_under(pos, &Self::UPPER).is_some() {
            Some(CursorShape::Arrow)
        } else {
            match self.hit_test(pos) {
                Hit::Window(_, Part::Client) => None,
                Hit::Window(_, Part::Border(e)) if e != 0 => Some(CursorShape::for_edges(e)),
                _ => Some(CursorShape::Arrow),
            }
        };
        false
    }

    /// A mouse button. Returns true when Station handled it (the program
    /// does not see it).
    pub fn desktop_button(&mut self, button: u32, pressed: bool, time: u32) -> bool {
        const BTN_LEFT: u32 = 0x110;
        let pos = self.pointer_location;
        if !pressed {
            return self.desktop_release(pos);
        }
        if self.desktop.message.is_some() {
            if button == BTN_LEFT && self.message_ok_at(pos) {
                self.close_message();
            }
            self.desktop.grab = true;
            return true;
        }
        if self.desktop.drag.is_some() {
            // A click ends keyboard Move/Size.
            self.drag_finish(true);
            self.desktop.grab = true;
            return true;
        }
        let hit = self.hit_test(pos);
        let double = matches!(self.desktop.last_click, Some((h, t)) if h == hit && time.wrapping_sub(t) < DOUBLE_CLICK_MS);
        self.desktop.last_click = Some((hit, time));
        if self.desktop.menu.is_some() {
            match self.menu_item_at(pos) {
                Some(Some(i)) => {
                    let cmd = self.desktop.menu.as_ref().map(|m| m.items[i].1);
                    if let Some(c) = cmd {
                        self.menu_run(c);
                    }
                }
                Some(None) => {}
                None => {
                    let owner = self.desktop.menu.as_ref().map(|m| m.id);
                    self.close_menu();
                    // Double-click on the box or icon that opened the menu.
                    if double && button == BTN_LEFT {
                        match hit {
                            Hit::Window(id, Part::Control) if Some(id) == owner => {
                                self.close_task(id)
                            }
                            Hit::Icon(id) if Some(id) == owner => self.restore(id),
                            _ => {}
                        }
                    }
                }
            }
            self.desktop.grab = true;
            return true;
        }
        if let Some((layer, _, _)) = self.layer_surface_under(pos, &Self::UPPER) {
            // The System Bar: the click is the program's.
            self.focus_layer(&layer);
            return false;
        }
        if hit == Hit::Desktop {
            if let Some((layer, _, _)) = self.layer_surface_under(pos, &Self::LOWER) {
                self.focus_layer(&layer);
                return false;
            }
        }
        match hit {
            Hit::Window(id, part) => {
                let focused = self.tasks.focused() == Some(id);
                if !focused || part != Part::Client {
                    self.focus_task(id);
                }
                if part == Part::Client {
                    return false;
                }
                self.desktop.grab = true;
                if button != BTN_LEFT {
                    return true;
                }
                match part {
                    Part::Caption if double => {
                        let max = self.tasks.get(id).is_some_and(|t| t.place.maximized);
                        if max {
                            self.restore(id);
                        } else if self
                            .tasks
                            .get(id)
                            .is_some_and(|t| !t.dialog && !fixed_size(t))
                        {
                            self.maximize(id);
                        }
                    }
                    Part::Caption => self.begin_drag(id, DragKind::Move),
                    Part::Border(e) if e != 0 => self.begin_drag(id, DragKind::Resize(e)),
                    Part::Control if double => self.close_task(id),
                    Part::Control => self.open_window_menu(id, true),
                    Part::Minimize | Part::Maximize => {
                        self.desktop.pressed = Some((id, part, true));
                        self.dirty = true;
                    }
                    _ => {}
                }
                true
            }
            Hit::Icon(id) => {
                self.desktop.grab = true;
                if double {
                    self.restore(id);
                } else {
                    self.tasks.set_focus(id);
                    if let Some(kb) = self.seat.get_keyboard() {
                        kb.set_focus(self, None, SERIAL_COUNTER.next_serial());
                    }
                    self.open_window_menu(id, true);
                }
                true
            }
            Hit::Desktop => {
                self.desktop.grab = true;
                true
            }
        }
    }

    fn desktop_release(&mut self, pos: Point<f64, Logical>) -> bool {
        let grabbed = std::mem::take(&mut self.desktop.grab);
        if self
            .desktop
            .drag
            .as_ref()
            .is_some_and(|d| d.start.is_some())
        {
            self.drag_finish(true);
            return true;
        }
        if let Some((id, part, over)) = self.desktop.pressed.take() {
            self.dirty = true;
            if over {
                match part {
                    Part::Minimize => self.minimize(id),
                    Part::Maximize => {
                        let restore = self.tasks.get(id).is_some_and(|t| t.place.maximized);
                        if restore {
                            self.restore(id);
                        } else {
                            self.maximize(id);
                        }
                    }
                    _ => {}
                }
            }
            return true;
        }
        if let Some(menu) = self.desktop.menu.as_mut() {
            let held = std::mem::take(&mut menu.held);
            if held {
                if let Some(Some(i)) = self.menu_item_at(pos) {
                    let cmd = self.desktop.menu.as_ref().map(|m| m.items[i].1);
                    if let Some(c) = cmd {
                        self.menu_run(c);
                    }
                }
            }
            return true;
        }
        grabbed
    }

    fn message_ok_at(&self, pos: Point<f64, Logical>) -> bool {
        match self
            .desktop
            .message
            .as_ref()
            .and_then(|m| m.buffer.as_ref())
        {
            Some((_, _, ok)) => ok.to_f64().contains(pos),
            None => false,
        }
    }

    pub fn message_key(&mut self, sym: u32) {
        if matches!(
            sym,
            keysyms::KEY_Return | keysyms::KEY_KP_Enter | keysyms::KEY_Escape | keysyms::KEY_space
        ) {
            self.close_message();
        }
    }

    // -------------------------------------------------------- drawing

    fn icon_pos(&self, slot: usize) -> Point<i32, Logical> {
        let m = self.metrics();
        let work = self.work_area();
        let (iw, ih) = (chrome::ICON_W * m.u, (chrome::ICON_H + 4) * m.u);
        let per_row = ((work.size.w - 4 * m.u) / iw).max(1) as usize;
        let (row, col) = ((slot / per_row) as i32, (slot % per_row) as i32);
        let snap = |v: i32| v - v.rem_euclid(m.u);
        (
            snap(work.loc.x + 4 * m.u + col * iw),
            snap(work.loc.y + work.size.h - (row + 1) * ih),
        )
            .into()
    }

    /// Rebuild what changed and the list of things to draw.
    pub fn desktop_prepare(&mut self) {
        let m = self.metrics();
        let mut draw = Vec::new();
        let out = self.output_size();
        let focused = self.tasks.focused();

        // Message box, window menu, Alt+Tab box: on top of everything.
        if let Some(msg) = self.desktop.message.as_mut() {
            if msg.buffer.is_none() {
                let (c, (bx, by, bw, bh)) = chrome::draw_message(&msg.title, &msg.lines, &m);
                let (w, h) = (c.width as i32 / m.scale, c.height as i32 / m.scale);
                let at: Point<i32, Logical> = ((out.w - w) / 2, (out.h - h) / 2).into();
                let ok = Rectangle::new(
                    (at.x + bx * m.u, at.y + by * m.u).into(),
                    (bw * m.u, bh * m.u).into(),
                );
                msg.buffer = Some((chrome::to_buffer(&c, m.scale), at, ok));
            }
            if let Some((b, at, _)) = &msg.buffer {
                draw.push(DrawItem::Buffer(b.clone(), *at, None));
            }
        }
        if let Some(menu) = self.desktop.menu.as_mut() {
            if menu.buffer.is_none() {
                let only: Vec<MenuItem> = menu.items.iter().map(|(i, _)| i.clone()).collect();
                menu.buffer = Some(chrome::to_buffer(
                    &retro_gfx::widgets::draw_menu(&only, menu.sel, m.px),
                    m.scale,
                ));
            }
            if let Some(b) = &menu.buffer {
                draw.push(DrawItem::Buffer(b.clone(), menu.at, None));
            }
        }
        let switch_target = self
            .desktop
            .switcher
            .as_ref()
            .and_then(|s| s.ids.get(s.index).copied());
        if let Some(id) = switch_target {
            let (art, title) = match self.tasks.get(id) {
                Some(t) => (Self::icon_art(t), self.window_title(t)),
                None => (IconArt::Program, String::new()),
            };
            if let Some(sw) = self.desktop.switcher.as_mut() {
                if sw.buffer.is_none() {
                    let c = chrome::draw_switch_box(art, &title, &m);
                    let size = (c.width as i32 / m.scale, c.height as i32 / m.scale).into();
                    sw.buffer = Some((chrome::to_buffer(&c, m.scale), size));
                }
                if let Some((b, size)) = &sw.buffer {
                    draw.push(DrawItem::Buffer(
                        b.clone(),
                        ((out.w - size.w) / 2, (out.h - size.h) / 2).into(),
                        None,
                    ));
                }
            }
        }
        if let Some(d) = self.desktop.drag.as_ref().filter(|d| d.shown) {
            let rects = chrome::outline_rects(d.to, &m);
            while self.desktop.outline_ids.len() < rects.len() {
                self.desktop.outline_ids.push(Id::new());
            }
            for ((r, c), id) in rects.into_iter().zip(&self.desktop.outline_ids) {
                draw.push(DrawItem::Solid(id.clone(), r, c));
            }
        }

        // Overlay layers, then the System Bar (unless a full-screen window
        // is on top), then windows, top first.
        use smithay::wayland::shell::wlr_layer::Layer;
        let fullscreen_on_top = self
            .tasks
            .z_order()
            .into_iter()
            .filter_map(|id| self.tasks.get(id))
            .find(|t| self.visible(t))
            .is_some_and(|t| t.place.fullscreen);
        let upper: &[Layer] = if fullscreen_on_top {
            &[Layer::Overlay]
        } else {
            &[Layer::Overlay, Layer::Top]
        };
        for (l, at) in self.layers_on(upper) {
            draw.push(DrawItem::Layer(l, at));
        }
        for id in self.tasks.z_order() {
            let Some(t) = self.tasks.get(id) else {
                continue;
            };
            if !self.visible(t) {
                continue;
            }
            let rect = self.shown_rect(t);
            draw.push(DrawItem::Window(id, rect.loc));
            if let Some(f) = self.frame_of(t) {
                let style = TitleStyle {
                    title: self.window_title(t),
                    active: focused == Some(id),
                    maximized: t.place.maximized,
                    buttons: !t.dialog && !fixed_size(t),
                    pressed: match self.desktop.pressed {
                        Some((pid, part, true)) if pid == id => Some(part),
                        _ => None,
                    },
                };
                let decor = self.desktop.decor.entry(id).or_insert_with(|| Decor {
                    ids: (0..20).map(|_| Id::new()).collect(),
                    title: None,
                });
                let key = (rect.size.w, style, m);
                if decor.title.as_ref().map(|(k, _)| k) != Some(&key) {
                    let c = chrome::draw_title(rect.size.w, &m, &key.1);
                    decor.title = Some((key, chrome::to_buffer(&c, m.scale)));
                }
                if let Some((_, b)) = &decor.title {
                    draw.push(DrawItem::Buffer(
                        b.clone(),
                        (rect.loc.x, f.loc.y + m.border()).into(),
                        None,
                    ));
                }
                for ((r, c), sid) in chrome::frame_rects(f.size.w, f.size.h, &m)
                    .into_iter()
                    .zip(&decor.ids)
                {
                    let r = Rectangle::new(r.loc + f.loc, r.size);
                    draw.push(DrawItem::Solid(sid.clone(), r, c));
                }
            }
            if t.place.fullscreen {
                // Covers everything below.
                self.desktop.draw = draw;
                return;
            }
        }

        // Icons of minimised windows, on the desktop.
        let icons: Vec<(u64, usize, IconArt, String)> = self
            .tasks
            .iter()
            .filter(|t| t.place.minimized && !t.dialog)
            .filter_map(|t| Some((t.id, t.place.slot?, Self::icon_art(t), self.window_title(t))))
            .collect();
        for (id, slot, art, label) in icons {
            let pos = self.icon_pos(slot);
            let key = (label, focused == Some(id), pos, m);
            let stale = self.desktop.icons.get(&id).map(|(k, _)| k) != Some(&key);
            if stale {
                let c = chrome::draw_icon(art, &key.0, key.1, &m, pos.x * m.scale, pos.y * m.scale);
                self.desktop
                    .icons
                    .insert(id, (key, chrome::to_buffer(&c, m.scale)));
            }
            if let Some((_, b)) = self.desktop.icons.get(&id) {
                draw.push(DrawItem::Buffer(b.clone(), pos, None));
            }
        }

        // Desktop icons and backgrounds that programs put below the windows.
        for (l, at) in self.layers_on(&[Layer::Bottom, Layer::Background]) {
            draw.push(DrawItem::Layer(l, at));
        }

        // The dithered desktop, in full-width bands.
        const BAND: i32 = 64;
        let key = (out.w, out.h, m);
        if self.desktop.band.as_ref().map(|(k, _)| k) != Some(&key) {
            let c = chrome::pattern_band((out.w * m.scale) as u32, BAND, &m);
            self.desktop.band = Some((key, chrome::to_buffer(&c, m.scale)));
        }
        let band_h = BAND * m.px / m.scale;
        let n = ((out.h + band_h - 1) / band_h).max(1) as usize;
        while self.desktop.band_ids.len() < n {
            self.desktop.band_ids.push(Id::new());
        }
        if let Some((_, b)) = &self.desktop.band {
            for (i, id) in self.desktop.band_ids.iter().take(n).enumerate() {
                draw.push(DrawItem::Buffer(
                    b.clone(),
                    (0, i as i32 * band_h).into(),
                    Some(id.clone()),
                ));
            }
        }
        self.desktop.draw = draw;
    }

    /// Forget per-window caches of a closed task.
    pub fn desktop_forget(&mut self, id: u64) {
        self.desktop.decor.remove(&id);
        self.desktop.icons.remove(&id);
        if self.desktop.menu.as_ref().is_some_and(|m| m.id == id) {
            self.desktop.menu = None;
        }
        if self.desktop.drag.as_ref().is_some_and(|d| d.id == id) {
            self.desktop.drag = None;
        }
        if self.desktop.pressed.is_some_and(|(p, _, _)| p == id) {
            self.desktop.pressed = None;
        }
        if let Some(sw) = self.desktop.switcher.as_mut() {
            sw.ids.retain(|i| *i != id);
            if sw.ids.is_empty() {
                self.desktop.switcher = None;
            } else {
                sw.index = sw.index.min(sw.ids.len() - 1);
                sw.buffer = None;
            }
        }
    }
}

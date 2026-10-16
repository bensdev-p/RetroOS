//! retro-console — the RetroOS text console.
//!
//! A Wayland terminal emulator that is the RetroDOS screen: Phosphor Mono at
//! an integer scale, the Phosphor16 palette, a blinking DOS underline cursor
//! and an inverse-block mouse cursor. In text mode Station shows it full
//! screen; in the Desktop it is the "DOS Prompt" window.
//!
//!   retro-console [--once] [--exec PROGRAM [ARGS…]]     (default program: retrodos)
//!
//! Without --once the program is restarted when it exits (RetroDOS); with
//! --once the console exits with the program's status (login screen, Setup).

mod keys;
mod pty;
mod screen;

use alacritty_terminal::term::TermMode;
use screen::Screen;
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState, FrameCallbackData};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::calloop::generic::Generic;
use smithay_client_toolkit::reexports::calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::calloop::{
    EventLoop, Interest, LoopHandle, Mode, PostAction,
};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::globals::registry_queue_init;
use smithay_client_toolkit::reexports::client::protocol::{
    wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface,
};
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::keyboard::{
    KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers,
};
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, PointerHandler};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::xdg::window::{
    Window, WindowConfigure, WindowDecorations, WindowHandler,
};
use smithay_client_toolkit::shell::xdg::XdgShell;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{delegate_registry, registry_handlers};
use std::io::{Read, Write};
use std::time::{Duration, Instant};

struct App {
    registry: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    buffer: Option<Buffer>,
    window: Window,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    loop_handle: LoopHandle<'static, App>,

    screen: Option<Screen>,
    pty: Option<pty::Pty>,
    argv: Vec<String>,
    term_name: String,
    logical: (u32, u32),
    scale: i32,
    configured: bool,
    frame_pending: bool,
    needs_draw: bool,
    mods: Modifiers,
    respawns: Vec<Instant>,
    exit: bool,
    title: String,
    /// --once: leave when the program exits (login screen, Setup) instead of restarting it.
    once: bool,
    exit_code: i32,
}

impl App {
    fn phys(&self) -> (u32, u32) {
        (
            self.logical.0 * self.scale as u32,
            self.logical.1 * self.scale as u32,
        )
    }

    fn start_shell(&mut self) {
        let Some(screen) = &self.screen else { return };
        let (c, r) = (screen.cols as u16, screen.rows as u16);
        let env = [
            ("TERM", self.term_name.clone()),
            ("RETROOS_CONSOLE", "1".to_string()),
            ("COLUMNS", c.to_string()),
            ("LINES", r.to_string()),
        ];
        match pty::Pty::spawn(&self.argv, c, r, &env) {
            Ok(p) => {
                let fd = p.master.try_clone().expect("dup pty");
                self.pty = Some(p);
                let _ = self.loop_handle.insert_source(
                    Generic::new(fd, Interest::READ, Mode::Level),
                    |_, f, app: &mut App| {
                        let mut buf = [0u8; 65536];
                        loop {
                            // SAFETY: read-only use of the registered fd.
                            match unsafe { f.get_mut() }.read(&mut buf) {
                                Ok(0) => return Ok(app.shell_gone()),
                                Ok(n) => {
                                    if let Some(s) = app.screen.as_mut() {
                                        s.feed(&buf[..n]);
                                    }
                                    app.needs_draw = true;
                                }
                                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                                Err(_) => return Ok(app.shell_gone()), // EIO: the shell closed the pty
                            }
                        }
                        app.flush_outbox();
                        Ok(PostAction::Continue)
                    },
                );
            }
            Err(e) => {
                if let Some(s) = self.screen.as_mut() {
                    s.feed(
                        format!("\r\nretro-console: cannot start {}: {e}\r\n", self.argv[0])
                            .as_bytes(),
                    );
                }
                self.needs_draw = true;
            }
        }
    }

    /// The shell exited: start a new one (with a pause and a limit, so a
    /// broken shell can't spin).
    fn shell_gone(&mut self) -> PostAction {
        let code = self.pty.as_ref().and_then(|p| p.try_wait());
        self.pty = None;
        if self.once {
            self.exit = true;
            self.exit_code = code.unwrap_or(0);
            return PostAction::Remove;
        }
        let now = Instant::now();
        self.respawns
            .retain(|t| now.duration_since(*t) < Duration::from_secs(10));
        self.respawns.push(now);
        if self.respawns.len() > 5 {
            if let Some(s) = self.screen.as_mut() {
                s.feed(b"\r\nThe command interpreter keeps stopping. Press Ctrl+Alt+Tab to switch tasks.\r\n");
            }
            self.needs_draw = true;
            return PostAction::Remove;
        }
        tracing_note(&format!("shell exited ({code:?}); restarting"));
        let _ = self.loop_handle.insert_source(
            Timer::from_duration(Duration::from_millis(300)),
            |_, _, app| {
                if let Some(s) = app.screen.as_mut() {
                    s.feed(b"\x1b[0m\r\n");
                }
                app.start_shell();
                TimeoutAction::Drop
            },
        );
        PostAction::Remove
    }

    fn write_pty(&mut self, bytes: &[u8]) {
        if let Some(p) = self.pty.as_mut() {
            let _ = p.master.write_all(bytes);
        }
    }

    fn flush_outbox(&mut self) {
        let Some(s) = self.screen.as_ref() else {
            return;
        };
        let (w, title) = {
            let mut o = s.outbox.borrow_mut();
            (std::mem::take(&mut o.pty_write), o.title.take())
        };
        if !w.is_empty() {
            self.write_pty(&w);
        }
        if let Some(t) = title {
            self.title = if t.is_empty() { "RetroDOS".into() } else { t };
            self.window.set_title(self.title.clone());
        }
    }

    fn draw(&mut self, qh: &QueueHandle<Self>) {
        if !self.configured || self.frame_pending {
            return;
        }
        let (w, h) = self.phys();
        let Some(screen) = self.screen.as_mut() else {
            return;
        };
        let changed = screen.render();
        if !changed && self.buffer.is_some() && !self.needs_draw {
            return;
        }
        self.needs_draw = false;
        let stride = w as i32 * 4;
        let buffer = self.buffer.get_or_insert_with(|| {
            self.pool
                .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Xrgb8888)
                .expect("buffer")
                .0
        });
        let canvas = match self.pool.canvas(buffer) {
            Some(c) => c,
            None => {
                let (b, c) = self
                    .pool
                    .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Xrgb8888)
                    .expect("buffer");
                *buffer = b;
                c
            }
        };
        for (dst, src) in canvas.chunks_exact_mut(4).zip(screen.canvas.px.iter()) {
            dst.copy_from_slice(&src.to_le_bytes());
        }
        let surface = self.window.wl_surface();
        surface.set_buffer_scale(self.scale);
        surface.damage_buffer(0, 0, w as i32, h as i32);
        surface.frame(qh, FrameCallbackData(surface.clone()));
        buffer.attach_to(surface).expect("attach");
        self.window.commit();
        self.frame_pending = true;
    }

    fn relayout(&mut self) {
        let (w, h) = self.phys();
        if w == 0 || h == 0 {
            return;
        }
        match self.screen.as_mut() {
            None => {
                self.screen = Some(Screen::new(w, h));
                self.start_shell();
            }
            Some(s) => {
                if s.resize(w, h) {
                    let (c, r) = (s.cols as u16, s.rows as u16);
                    if let Some(p) = &self.pty {
                        p.resize(c, r);
                    }
                }
            }
        }
        self.buffer = None;
        self.needs_draw = true;
    }

    fn mouse_report(&mut self, button: u32, pressed: bool, cell: (usize, usize)) {
        let Some(s) = &self.screen else { return };
        let mode = s.mode();
        if !mode.intersects(TermMode::MOUSE_MODE) {
            return;
        }
        let (line, col) = cell;
        let seq = if mode.contains(TermMode::SGR_MOUSE) {
            format!(
                "\x1b[<{button};{};{}{}",
                col + 1,
                line + 1,
                if pressed { 'M' } else { 'm' }
            )
        } else {
            let b = if pressed { button } else { 3 };
            format!(
                "\x1b[M{}{}{}",
                (32 + b) as u8 as char,
                (33 + col.min(222)) as u8 as char,
                (33 + line.min(222)) as u8 as char
            )
        };
        self.write_pty(seq.as_bytes());
    }
}

fn tracing_note(msg: &str) {
    eprintln!("retro-console: {msg}");
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        factor: i32,
    ) {
        if factor != self.scale && factor > 0 {
            self.scale = factor;
            self.relayout();
            self.draw(qh);
        }
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
        self.draw(qh);
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

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl WindowHandler for App {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &Window,
        cfg: WindowConfigure,
        _: u32,
    ) {
        let w = cfg
            .new_size
            .0
            .map(|v| v.get())
            .unwrap_or(if self.logical.0 > 0 {
                self.logical.0
            } else {
                640
            });
        let h = cfg
            .new_size
            .1
            .map(|v| v.get())
            .unwrap_or(if self.logical.1 > 0 {
                self.logical.1
            } else {
                400
            });
        let first = !self.configured;
        self.configured = true;
        if first || (w, h) != self.logical {
            self.logical = (w, h);
            self.relayout();
        }
        self.frame_pending = false;
        self.draw(qh);
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        cap: Capability,
    ) {
        if cap == Capability::Keyboard && self.keyboard.is_none() {
            let kb = self.seat_state.get_keyboard_with_repeat(
                qh,
                &seat,
                None,
                self.loop_handle.clone(),
                Box::new(|app: &mut App, _kb, ev| app.key(ev)),
            );
            self.keyboard = kb.ok();
        }
        if cap == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
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
            if let Some(k) = self.keyboard.take() {
                k.release();
            }
        }
        if cap == Capability::Pointer {
            if let Some(p) = self.pointer.take() {
                p.release();
            }
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl App {
    fn key(&mut self, ev: KeyEvent) {
        // Shift+PgUp/PgDn scroll back through history.
        if self.mods.shift && matches!(ev.keysym, Keysym::Page_Up | Keysym::Page_Down) {
            if let Some(s) = self.screen.as_mut() {
                let n = (s.rows as i32 / 2).max(1);
                s.scroll(if ev.keysym == Keysym::Page_Up { n } else { -n });
                self.needs_draw = true;
            }
            return;
        }
        let app_cursor = self
            .screen
            .as_ref()
            .map(|s| s.mode().contains(TermMode::APP_CURSOR))
            .unwrap_or(false);
        if let Some(bytes) = keys::encode(&ev, &self.mods, app_cursor) {
            if let Some(s) = self.screen.as_mut() {
                s.scroll(i32::MIN / 2); // typing snaps back to the bottom
                s.cursor_on = true;
            }
            self.write_pty(&bytes);
        }
    }
}

impl KeyboardHandler for App {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
    }
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
    }
    fn press_key(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        ev: KeyEvent,
    ) {
        self.key(ev);
        self.draw(qh);
    }
    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _ev: KeyEvent,
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
        self.mods = m;
    }
}

impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        ptr: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for e in events {
            if &e.surface != self.window.wl_surface() {
                continue;
            }
            let scale = self.scale as f64;
            let cell = self
                .screen
                .as_ref()
                .and_then(|s| s.cell_at(e.position.0 * scale, e.position.1 * scale));
            match e.kind {
                PointerEventKind::Enter { serial } => {
                    // The console draws its own (DOS-style block) mouse cursor.
                    ptr.set_cursor(serial, None, 0, 0);
                    if let Some(s) = self.screen.as_mut() {
                        s.mouse_cell = cell;
                    }
                }
                PointerEventKind::Leave { .. } => {
                    if let Some(s) = self.screen.as_mut() {
                        s.mouse_cell = None;
                    }
                }
                PointerEventKind::Motion { .. } => {
                    if let Some(s) = self.screen.as_mut() {
                        s.mouse_cell = cell;
                    }
                }
                PointerEventKind::Press { button, .. }
                | PointerEventKind::Release { button, .. } => {
                    let pressed = matches!(e.kind, PointerEventKind::Press { .. });
                    let b = match button {
                        0x110 => 0, // BTN_LEFT
                        0x112 => 1, // BTN_MIDDLE
                        0x111 => 2, // BTN_RIGHT
                        _ => continue,
                    };
                    if let Some(c) = cell {
                        self.mouse_report(b, pressed, c);
                    }
                }
                PointerEventKind::Axis { vertical, .. } => {
                    let steps = if vertical.discrete != 0 {
                        vertical.discrete
                    } else {
                        (vertical.absolute / 10.0) as i32
                    };
                    if steps == 0 {
                        continue;
                    }
                    let reporting = self
                        .screen
                        .as_ref()
                        .map(|s| s.mode().intersects(TermMode::MOUSE_MODE))
                        .unwrap_or(false);
                    if reporting {
                        if let Some(c) = cell {
                            self.mouse_report(if steps < 0 { 64 } else { 65 }, true, c);
                        }
                    } else if let Some(s) = self.screen.as_mut() {
                        s.scroll(-steps * 3);
                    }
                }
            }
        }
        self.needs_draw = true;
        self.draw(qh);
    }
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(App);
smithay_client_toolkit::delegate_dispatch2!(App);

/// TERM for programs in the console. We emulate xterm faithfully, and an
/// xterm TERM keeps ssh and remote tools working (docs/DECISIONS.md D-016).
fn terminfo_name() -> String {
    std::env::var("RETROOS_TERM").unwrap_or_else(|_| "xterm-256color".into())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let once = args.iter().take_while(|a| *a != "--exec").any(|a| a == "--once");
    let argv = match args.iter().position(|a| a == "--exec") {
        Some(i) if i + 1 < args.len() => args[i + 1..].to_vec(),
        _ => vec!["retrodos".to_string()],
    };

    let conn = Connection::connect_to_env().unwrap_or_else(|e| {
        eprintln!("retro-console: no Wayland display ({e})");
        std::process::exit(1);
    });
    let (globals, queue) = registry_queue_init(&conn).expect("registry");
    let qh = queue.handle();
    let mut event_loop: EventLoop<App> = EventLoop::try_new().expect("event loop");
    WaylandSource::new(conn.clone(), queue)
        .insert(event_loop.handle())
        .expect("wayland source");

    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor");
    let xdg = XdgShell::bind(&globals, &qh).expect("xdg_wm_base");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm");
    let surface = compositor.create_surface(&qh);
    let window = xdg.create_window(surface, WindowDecorations::RequestServer, &qh);
    window.set_title("RetroDOS");
    window.set_app_id("retroos.console");
    window.commit();
    let pool = SlotPool::new(1920 * 1080 * 4, &shm).expect("shm pool");

    let mut app = App {
        registry: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pool,
        buffer: None,
        window,
        keyboard: None,
        pointer: None,
        loop_handle: event_loop.handle(),
        screen: None,
        pty: None,
        argv,
        term_name: terminfo_name(),
        logical: (0, 0),
        scale: 1,
        configured: false,
        frame_pending: false,
        needs_draw: true,
        mods: Modifiers::default(),
        respawns: Vec::new(),
        exit: false,
        title: "RetroDOS".into(),
        once,
        exit_code: 0,
    };

    // Cursor blink: 2 Hz, 50% duty (docs/STYLE.md §3).
    event_loop
        .handle()
        .insert_source(
            Timer::from_duration(Duration::from_millis(250)),
            move |_, _, app| {
                if let Some(s) = app.screen.as_mut() {
                    s.cursor_on = !s.cursor_on;
                }
                TimeoutAction::ToDuration(Duration::from_millis(250))
            },
        )
        .expect("blink timer");

    while !app.exit {
        if event_loop
            .dispatch(Duration::from_millis(250), &mut app)
            .is_err()
        {
            break;
        }
        app.flush_outbox();
        app.draw(&qh);
        let _ = conn.flush();
    }
    std::process::exit(app.exit_code);
}

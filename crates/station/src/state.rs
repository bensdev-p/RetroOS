//! Station's state: Wayland globals, the task list, and personality logic.

use crate::tasks::{Task, TaskKind, Tasks};
use smithay::desktop::{PopupManager, Space, Window, WindowSurfaceType};
use smithay::input::pointer::CursorImageStatus;
use smithay::input::{Seat, SeatState};
use smithay::output::Output;
use smithay::reexports::calloop::{
    generic::Generic, Interest, LoopHandle, LoopSignal, Mode, PostAction,
};
use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
use smithay::reexports::wayland_server::backend::{ClientData, ClientId, DisconnectReason};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{Display, DisplayHandle, Resource};
use smithay::utils::{Logical, Point, Size};
use smithay::wayland::compositor::{with_states, CompositorClientState, CompositorState};
use smithay::wayland::output::OutputManagerState;
use smithay::wayland::selection::data_device::DataDeviceState;
use smithay::wayland::shell::xdg::decoration::XdgDecorationState;
use smithay::wayland::shell::xdg::{ToplevelSurface, XdgShellState, XdgToplevelSurfaceData};
use smithay::wayland::shm::ShmState;
use smithay::wayland::socket::ListeningSocketSource;
use std::ffi::OsString;
use std::process::Child;
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Personality {
    /// RetroDOS: everything full-screen, Ctrl+Alt+Tab task swapper.
    Text,
    /// RetroOS Desktop (Milestone 3).
    Desktop,
}

pub struct Station {
    pub dh: DisplayHandle,
    pub loop_handle: LoopHandle<'static, Station>,
    pub loop_signal: LoopSignal,
    pub socket_name: OsString,
    pub start: Instant,

    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub xdg_decoration_state: XdgDecorationState,
    pub shm_state: ShmState,
    pub output_manager_state: OutputManagerState,
    pub seat_state: SeatState<Station>,
    pub data_device_state: DataDeviceState,
    pub popups: PopupManager,
    pub seat: Seat<Station>,

    pub space: Space<Window>,
    pub tasks: Tasks,
    pub personality: Personality,
    pub output: Option<Output>,
    /// Integer output scale (docs/DESIGN.md §6.3).
    pub scale: i32,

    pub pointer_location: Point<f64, Logical>,
    pub cursor_status: CursorImageStatus,
    /// Something changed on screen; render at the next opportunity.
    pub dirty: bool,

    pub children: Vec<(Child, Option<u64>)>,
    /// The client started with --exec; if it exits for good, Station exits.
    pub primary_pid: Option<u32>,
    pub exit_code: i32,
    pub backend: crate::Backend,
    pub broker: crate::broker::Broker,
}

#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

/// Integer scale so the logical desktop is at least 960×540 (1080p → 2×, 4K → 4×).
pub fn integer_scale(w: i32, h: i32) -> i32 {
    (w / 960).min(h / 540).clamp(1, 8)
}

impl Station {
    pub fn new(
        display: Display<Station>,
        loop_handle: LoopHandle<'static, Station>,
        loop_signal: LoopSignal,
        personality: Personality,
        backend: crate::Backend,
    ) -> Station {
        let dh = display.handle();
        let compositor_state = CompositorState::new::<Station>(&dh);
        let xdg_shell_state = XdgShellState::new::<Station>(&dh);
        let xdg_decoration_state = XdgDecorationState::new::<Station>(&dh);
        let shm_state = ShmState::new::<Station>(&dh, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Station>(&dh);
        let mut seat_state = SeatState::new();
        let data_device_state = DataDeviceState::new::<Station>(&dh);
        let mut seat: Seat<Station> = seat_state.new_wl_seat(&dh, "seat0");
        let xkb = crate::input::xkb_config();
        if seat.add_keyboard(xkb, 400, 25).is_err() {
            tracing::warn!("keymap from /etc/default/keyboard failed; using us");
            let _ = seat.add_keyboard(Default::default(), 400, 25);
        }
        seat.add_pointer();

        let socket = ListeningSocketSource::new_auto().expect("wayland socket");
        let socket_name = socket.socket_name().to_os_string();
        loop_handle
            .insert_source(socket, |stream, _, st: &mut Station| {
                if let Err(e) = st
                    .dh
                    .insert_client(stream, Arc::new(ClientState::default()))
                {
                    tracing::warn!("client insert failed: {e}");
                }
            })
            .expect("socket source");
        loop_handle
            .insert_source(
                Generic::new(display, Interest::READ, Mode::Level),
                |_, display, st| {
                    // SAFETY: the display is owned by this source and never dropped while dispatching.
                    unsafe {
                        display.get_mut().dispatch_clients(st).unwrap();
                    }
                    Ok(PostAction::Continue)
                },
            )
            .expect("display source");

        let broker = crate::broker::Broker::new(&loop_handle, &socket_name);

        Station {
            dh,
            loop_handle,
            loop_signal,
            socket_name,
            start: Instant::now(),
            compositor_state,
            xdg_shell_state,
            xdg_decoration_state,
            shm_state,
            output_manager_state,
            seat_state,
            data_device_state,
            popups: PopupManager::default(),
            seat,
            space: Space::default(),
            tasks: Tasks::default(),
            personality,
            output: None,
            scale: 1,
            pointer_location: (0.0, 0.0).into(),
            cursor_status: CursorImageStatus::default_named(),
            dirty: true,
            children: Vec::new(),
            primary_pid: None,
            exit_code: 0,
            backend,
            broker,
        }
    }

    /// Logical size of the (single, in text mode) output.
    pub fn output_size(&self) -> Size<i32, Logical> {
        self.output
            .as_ref()
            .and_then(|o| {
                o.current_mode()
                    .map(|m| m.size.to_logical(o.current_scale().integer_scale()))
            })
            .unwrap_or_else(|| (960, 540).into())
    }

    pub fn now_ms(&self) -> u32 {
        self.start.elapsed().as_millis() as u32
    }

    // --------------------------------------------------------------- tasks

    pub fn add_toplevel(&mut self, surface: ToplevelSurface) {
        let window = Window::new_wayland_window(surface.clone());
        let pid = self
            .dh
            .get_client(surface.wl_surface().id())
            .ok()
            .and_then(|c| c.get_credentials(&self.dh).ok())
            .map(|c| c.pid as u32);
        let parent = surface.parent();
        let kind = if pid.is_some() && pid == self.primary_pid {
            TaskKind::Console
        } else {
            TaskKind::App
        };
        let launch = pid.and_then(|p| self.launch_for_pid(p));
        let id = self.tasks.add(Task {
            id: 0,
            window: window.clone(),
            pid,
            kind,
            launch,
            dialog: parent.is_some(),
        });
        self.space.map_element(window, (0, 0), false);
        self.configure_task(id);
        self.focus_task(id);
        tracing::info!(id, ?pid, ?kind, "new task");
    }

    /// The broker launch request whose process (or an ancestor) owns `pid`.
    fn launch_for_pid(&self, pid: u32) -> Option<u64> {
        let mut p = pid;
        for _ in 0..16 {
            if let Some((_, l)) = self.children.iter().find(|(c, _)| c.id() == p) {
                return *l;
            }
            p = parent_pid(p)?;
            if p <= 1 {
                return None;
            }
        }
        None
    }

    /// Send the configure that the current personality wants.
    pub fn configure_task(&mut self, id: u64) {
        let size = self.output_size();
        let Some(t) = self.tasks.get(id) else { return };
        let Some(top) = t.window.toplevel() else {
            return;
        };
        let focused = self.tasks.focused() == Some(id);
        let dialog = t.dialog;
        top.with_pending_state(|s| {
            if dialog {
                s.size = None;
                s.states.unset(xdg_toplevel::State::Fullscreen);
            } else {
                s.size = Some(size);
                s.states.set(xdg_toplevel::State::Fullscreen);
            }
            if focused {
                s.states.set(xdg_toplevel::State::Activated);
            } else {
                s.states.unset(xdg_toplevel::State::Activated);
            }
        });
        if top.is_initial_configure_sent() {
            top.send_pending_configure();
        }
    }

    pub fn focus_task(&mut self, id: u64) {
        let prev = self.tasks.focused();
        self.tasks.set_focus(id);
        if let Some(p) = prev.filter(|p| *p != id) {
            self.configure_task(p);
        }
        self.configure_task(id);
        if let Some(t) = self.tasks.get(id) {
            let w = t.window.clone();
            self.space.raise_element(&w, true);
            // Dialogs sit centred over their parent.
            if t.dialog {
                let geo = w.geometry().size;
                let out = self.output_size();
                let loc = ((out.w - geo.w).max(0) / 2, (out.h - geo.h).max(0) / 2);
                self.space.map_element(w.clone(), loc, true);
            }
            let surface = w.toplevel().map(|t| t.wl_surface().clone());
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            if let Some(kb) = self.seat.get_keyboard() {
                kb.set_focus(self, surface, serial);
            }
        }
        self.dirty = true;
    }

    pub fn remove_toplevel(&mut self, surface: &WlSurface) {
        let Some(id) = self.tasks.find_surface(surface) else {
            return;
        };
        let t = self.tasks.remove(id);
        if let Some(t) = t {
            self.space.unmap_elem(&t.window);
            tracing::info!(id, "task closed");
            if self.tasks.focused().is_none() || self.tasks.focused() == Some(id) {
                // Back to whoever launched it, else the most recent task.
                let back = t
                    .launch
                    .and_then(|l| self.broker.requester_task(l))
                    .filter(|tid| self.tasks.get(*tid).is_some())
                    .or_else(|| self.tasks.most_recent());
                if let Some(b) = back {
                    self.focus_task(b);
                }
            }
        }
        self.dirty = true;
    }

    pub fn cycle_tasks(&mut self, forward: bool) {
        if let Some(id) = self.tasks.cycle(forward) {
            self.focus_task(id);
        }
    }

    /// Title of a task's toplevel.
    pub fn task_title(&self, t: &Task) -> String {
        t.window
            .toplevel()
            .map(|top| {
                with_states(top.wl_surface(), |s| {
                    s.data_map
                        .get::<XdgToplevelSurfaceData>()
                        .and_then(|d| {
                            d.lock()
                                .ok()
                                .map(|d| d.title.clone().or(d.app_id.clone()).unwrap_or_default())
                        })
                        .unwrap_or_default()
                })
            })
            .unwrap_or_default()
    }

    /// Surface under the pointer (focused task only, popups first).
    pub fn surface_under(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        let id = self.tasks.focused()?;
        let t = self.tasks.get(id)?;
        let loc = self.space.element_location(&t.window)?;
        t.window
            .surface_under(pos - loc.to_f64(), WindowSurfaceType::ALL)
            .map(|(s, p)| (s, (p + loc).to_f64()))
    }

    // ------------------------------------------------------------ children

    /// Spawn a client with this compositor's environment.
    pub fn spawn(
        &mut self,
        argv: &[String],
        cwd: Option<&str>,
        env: &[(String, String)],
        launch: Option<u64>,
    ) -> std::io::Result<u32> {
        let mut cmd = std::process::Command::new(&argv[0]);
        cmd.args(&argv[1..]);
        cmd.env("WAYLAND_DISPLAY", &self.socket_name);
        cmd.env("RETROOS_STATION", self.broker.path());
        cmd.env_remove("DISPLAY");
        if let Some(c) = cwd {
            cmd.current_dir(c);
        }
        for (k, v) in env {
            cmd.env(k, v);
        }
        let child = cmd.spawn()?;
        let pid = child.id();
        self.children.push((child, launch));
        Ok(pid)
    }

    /// Reap exited children; report launch exits to the broker.
    pub fn reap(&mut self) {
        let mut exited = Vec::new();
        self.children.retain_mut(|(c, launch)| match c.try_wait() {
            Ok(Some(status)) => {
                exited.push((c.id(), status.code().unwrap_or(1), *launch));
                false
            }
            _ => true,
        });
        for (pid, code, launch) in exited {
            tracing::info!(pid, code, "child exited");
            if let Some(l) = launch {
                self.broker.launch_exited(l, code);
            }
            if Some(pid) == self.primary_pid {
                tracing::info!("primary client exited; leaving");
                self.exit_code = code;
                self.loop_signal.stop();
            }
        }
    }
}

fn parent_pid(pid: u32) -> Option<u32> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // "pid (comm) state ppid ..." — comm may contain spaces, so split after ')'.
    let rest = &s[s.rfind(')')? + 2..];
    rest.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn scales() {
        assert_eq!(super::integer_scale(1920, 1080), 2);
        assert_eq!(super::integer_scale(3840, 2160), 4);
        assert_eq!(super::integer_scale(1024, 768), 1);
        assert_eq!(super::integer_scale(2560, 1440), 2);
    }
}

//! X11 programs (D-025): Station starts Xwayland the first time a program
//! that links libX11/libxcb but not libwayland-client is launched, and is
//! its window manager (smithay's X11Wm). An X11 window becomes a task like
//! any other: full screen in RetroDOS, framed and placed in the Desktop.
//! Override-redirect windows (menus, tooltips) are not tasks: they are drawn
//! where they put themselves, above everything.

use crate::chrome::{BOTTOM, LEFT, RIGHT, TOP};
use crate::desktop::DragKind;
use crate::state::{Personality, Station};
use crate::tasks::{Task, TaskKind};
use smithay::desktop::Window;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Rectangle};
use smithay::wayland::xwayland_shell::{XWaylandShellHandler, XWaylandShellState};
use smithay::xwayland::xwm::{Reorder, ResizeEdge, WmWindowType, XwmId};
use smithay::xwayland::{X11Surface, X11Wm, XWayland, XWaylandEvent, XwmHandler};
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// The executable `cmd` names (a path, or a name looked up in PATH).
fn resolve(cmd: &str) -> Option<PathBuf> {
    if cmd.contains('/') {
        return Some(PathBuf::from(cmd)).filter(|p| p.is_file());
    }
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|d| d.join(cmd))
            .find(|p| p.is_file())
    })
}

/// Libraries an X11-only program links: X11 or XCB, and no Wayland client.
pub fn x11_only(needed: &[String]) -> bool {
    let x11 = needed.iter().any(|l| {
        l.starts_with("libX11.") || l.starts_with("libxcb.") || l.starts_with("libX11-xcb.")
    });
    let wayland = needed.iter().any(|l| l.starts_with("libwayland-client."));
    x11 && !wayland
}

/// Does `cmd` need an X server?
pub fn needs_x11(cmd: &str) -> bool {
    resolve(cmd)
        .and_then(|p| retro_apps::elf_needed(Path::new(&p)))
        .is_some_and(|n| x11_only(&n))
}

impl Station {
    /// The X display programs get, starting Xwayland the first time.
    /// Programs started right away just wait in connect() until it is ready.
    pub fn xwayland_display(&mut self) -> Option<u32> {
        if self.xdisplay.is_some() || std::env::var_os("RETROOS_NO_XWAYLAND").is_some() {
            return self.xdisplay;
        }
        // systemd makes /tmp/.X11-unix at boot; elsewhere (tests) make it.
        let dir = Path::new("/tmp/.X11-unix");
        if !dir.exists() && std::fs::create_dir(dir).is_ok() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o1777));
        }
        let spawned = XWayland::spawn(
            &self.dh,
            None,
            std::iter::empty::<(String, String)>(),
            true,
            Stdio::null(),
            // Its complaints go to Station's log (the journal on the Pi).
            Stdio::inherit(),
            |_| {},
        );
        let (xw, client) = match spawned {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("cannot start Xwayland: {e}");
                return None;
            }
        };
        let xdisp = xw.display_number();
        let res = self
            .loop_handle
            .insert_source(xw, move |ev, _, st: &mut Station| match ev {
                XWaylandEvent::Ready { x11_socket, .. } => {
                    match X11Wm::start_wm(st.loop_handle.clone(), x11_socket, client.clone()) {
                        Ok(wm) => {
                            tracing::info!(x_display = xdisp, "Xwayland ready");
                            st.xwm = Some(wm);
                        }
                        Err(e) => tracing::warn!("X11 window manager: {e}"),
                    }
                }
                XWaylandEvent::Error => {
                    tracing::warn!("Xwayland failed to start");
                    st.xdisplay = None;
                }
            });
        if let Err(e) = res {
            tracing::warn!("Xwayland event source: {e}");
            return None;
        }
        tracing::info!(x_display = xdisp, "starting Xwayland");
        self.xdisplay = Some(xdisp);
        Some(xdisp)
    }

    fn add_x11_task(&mut self, x: X11Surface) {
        if self.tasks.find_x11(&x).is_some() {
            return;
        }
        let window = Window::new_x11_window(x.clone());
        let pid = x.pid().or_else(|| x.get_client_pid().ok());
        let launch = pid.and_then(|p| self.launch_for_pid(p));
        let name = launch
            .and_then(|l| self.broker.launch_name(l))
            .or_else(|| {
                let comm = std::fs::read_to_string(format!("/proc/{}/comm", pid?)).ok()?;
                Some(comm.trim().to_ascii_uppercase())
            })
            .unwrap_or_else(|| x.class().to_ascii_uppercase());
        let dialog = x.is_transient_for().is_some()
            || matches!(
                x.window_type(),
                Some(WmWindowType::Dialog | WmWindowType::Utility)
            );
        let id = self.tasks.add(Task {
            id: 0,
            window: window.clone(),
            pid,
            kind: TaskKind::App,
            launch,
            name,
            dialog,
            place: Default::default(),
        });
        self.space.map_element(window, (0, 0), false);
        self.configure_task(id);
        self.focus_task(id);
        if self.personality == Personality::Desktop {
            // X11 windows have their size before their first commit: place now.
            self.desktop_commit(id);
        }
        tracing::info!(id, ?pid, "new X11 task");
    }

    /// Tell an X11 window where it is and what state it is in.
    pub fn x11_configure(&mut self, id: u64) {
        let out = self.output_size();
        let desktop = self.personality == Personality::Desktop;
        let focused = self.tasks.focused() == Some(id);
        let Some(t) = self.tasks.get(id) else { return };
        let Some(x) = t.x11().cloned() else { return };
        let p = t.place.clone();
        let geo = x.geometry();
        let (rect, fullscreen, maximized) = if !desktop {
            if t.dialog {
                let s = geo.size;
                let loc = ((out.w - s.w).max(0) / 2, (out.h - s.h).max(0) / 2);
                (Rectangle::new(loc.into(), s), false, false)
            } else {
                (Rectangle::from_size(out), true, false)
            }
        } else if p.fullscreen {
            (self.fullscreen_rect(t), true, false)
        } else if p.maximized {
            (self.maximized_rect(t), false, true)
        } else if p.placed {
            (self.shown_rect(t), false, false)
        } else {
            (geo, false, false)
        };
        let _ = x.set_fullscreen(fullscreen);
        let _ = x.set_maximized(maximized);
        let _ = x.set_activated(focused);
        let _ = x.set_suspended(p.minimized && desktop);
        let _ = x.configure(rect);
    }

    /// The X11 window gets the keyboard: activated, raised in X's stacking.
    pub fn x11_activate(&mut self, x: &X11Surface) {
        for t in self.tasks.iter() {
            if let Some(other) = t.x11().filter(|o| *o != x) {
                let _ = other.set_activated(false);
            }
        }
        let _ = x.set_activated(true);
        if let Some(wm) = self.xwm.as_mut() {
            let _ = wm.raise_window(x);
        }
    }

    fn x11_task(&self, x: &X11Surface) -> Option<u64> {
        self.tasks.find_x11(x)
    }

    fn forget_x11(&mut self, x: &X11Surface) {
        self.x11_overrides.retain(|o| o.x11_surface() != Some(x));
        if let Some(id) = self.x11_task(x) {
            self.remove_task(id);
        }
        self.dirty = true;
    }

    /// Override-redirect windows (menus, tooltips), topmost last.
    pub fn x11_override_windows(&self) -> &[Window] {
        &self.x11_overrides
    }
}

impl XwmHandler for Station {
    fn xwm_state(&mut self, _: XwmId) -> &mut X11Wm {
        self.xwm.as_mut().expect("X11 window manager running")
    }

    fn new_window(&mut self, _: XwmId, _: X11Surface) {}

    fn new_override_redirect_window(&mut self, _: XwmId, _: X11Surface) {}

    fn map_window_request(&mut self, _: XwmId, x: X11Surface) {
        let _ = x.set_mapped(true);
        self.add_x11_task(x);
    }

    fn mapped_override_redirect_window(&mut self, _: XwmId, x: X11Surface) {
        self.x11_overrides.retain(|o| o.x11_surface() != Some(&x));
        self.x11_overrides.push(Window::new_x11_window(x));
        self.dirty = true;
    }

    fn unmapped_window(&mut self, _: XwmId, x: X11Surface) {
        self.forget_x11(&x);
        if !x.is_override_redirect() {
            let _ = x.set_mapped(false);
        }
    }

    fn destroyed_window(&mut self, _: XwmId, x: X11Surface) {
        self.forget_x11(&x);
    }

    fn configure_request(
        &mut self,
        _: XwmId,
        x: X11Surface,
        _req_x: Option<i32>,
        _req_y: Option<i32>,
        w: Option<u32>,
        h: Option<u32>,
        _: Option<Reorder>,
    ) {
        let Some(id) = self.x11_task(&x) else {
            // Not mapped yet: grant the size (where it goes is ours to say).
            let mut g = x.geometry();
            if let Some(w) = w {
                g.size.w = w as i32;
            }
            if let Some(h) = h {
                g.size.h = h as i32;
            }
            let _ = x.configure(g);
            return;
        };
        // A normal window in the Desktop may change its own size.
        let free = self.personality == Personality::Desktop
            && self
                .tasks
                .get(id)
                .is_some_and(|t| !t.place.maximized && !t.place.fullscreen);
        if free {
            if let Some(t) = self.tasks.get_mut(id) {
                if let Some(w) = w {
                    t.place.rect.size.w = w as i32;
                }
                if let Some(h) = h {
                    t.place.rect.size.h = h as i32;
                }
            }
        }
        self.x11_configure(id);
        self.dirty = true;
    }

    fn configure_notify(
        &mut self,
        _: XwmId,
        _: X11Surface,
        _: Rectangle<i32, Logical>,
        _: Option<smithay::xwayland::xwm::X11Window>,
    ) {
        // Override-redirect windows move themselves: draw them again.
        self.dirty = true;
    }

    fn maximize_request(&mut self, _: XwmId, x: X11Surface) {
        if let (Some(id), Personality::Desktop) = (self.x11_task(&x), self.personality) {
            self.maximize(id);
        }
    }

    fn unmaximize_request(&mut self, _: XwmId, x: X11Surface) {
        if let (Some(id), Personality::Desktop) = (self.x11_task(&x), self.personality) {
            self.restore(id);
        }
    }

    fn fullscreen_request(&mut self, _: XwmId, x: X11Surface) {
        if let (Some(id), Personality::Desktop) = (self.x11_task(&x), self.personality) {
            self.set_fullscreen(id, true);
        }
    }

    fn unfullscreen_request(&mut self, _: XwmId, x: X11Surface) {
        if let (Some(id), Personality::Desktop) = (self.x11_task(&x), self.personality) {
            self.set_fullscreen(id, false);
        }
    }

    fn minimize_request(&mut self, _: XwmId, x: X11Surface) {
        if let (Some(id), Personality::Desktop) = (self.x11_task(&x), self.personality) {
            self.minimize(id);
        }
    }

    fn unminimize_request(&mut self, _: XwmId, x: X11Surface) {
        if let (Some(id), Personality::Desktop) = (self.x11_task(&x), self.personality) {
            self.restore(id);
        }
    }

    fn resize_request(&mut self, _: XwmId, x: X11Surface, _: u32, edge: ResizeEdge) {
        let edges = match edge {
            ResizeEdge::Top => TOP,
            ResizeEdge::Bottom => BOTTOM,
            ResizeEdge::Left => LEFT,
            ResizeEdge::Right => RIGHT,
            ResizeEdge::TopLeft => TOP | LEFT,
            ResizeEdge::TopRight => TOP | RIGHT,
            ResizeEdge::BottomLeft => BOTTOM | LEFT,
            ResizeEdge::BottomRight => BOTTOM | RIGHT,
        };
        if let Some(id) = self.x11_task(&x) {
            self.drag_task(id, DragKind::Resize(edges));
        }
    }

    fn move_request(&mut self, _: XwmId, x: X11Surface, _: u32) {
        if let Some(id) = self.x11_task(&x) {
            self.drag_task(id, DragKind::Move);
        }
    }
}

impl XWaylandShellHandler for Station {
    fn xwayland_shell_state(&mut self) -> &mut XWaylandShellState {
        &mut self.xwayland_shell_state
    }

    fn surface_associated(&mut self, _: XwmId, _: WlSurface, x: X11Surface) {
        // The window has its surface now: if it has the keyboard, hand it over.
        if let Some(id) = self.x11_task(&x) {
            if self.tasks.focused() == Some(id) {
                self.focus_task(id);
            }
        }
        self.dirty = true;
    }
}

smithay::delegate_xwayland_shell!(Station);

#[cfg(test)]
mod tests {
    use super::x11_only;

    #[test]
    fn x11_only_programs_are_recognised() {
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(x11_only(&v(&["libX11.so.6", "libc.so.6"])));
        assert!(x11_only(&v(&["libxcb.so.1"])));
        assert!(
            !x11_only(&v(&["libX11.so.6", "libwayland-client.so.0"])),
            "GTK: Wayland"
        );
        assert!(!x11_only(&v(&["libc.so.6"])));
    }
}

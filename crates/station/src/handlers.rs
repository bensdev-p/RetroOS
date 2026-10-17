//! Wayland protocol handlers.

use crate::state::{ClientState, Station};
use smithay::backend::renderer::utils::on_commit_buffer_handler;
use smithay::desktop::{find_popup_root_surface, get_popup_toplevel_coords, PopupKind};
use smithay::input::pointer::CursorImageStatus;
use smithay::input::{Seat, SeatHandler, SeatState};
use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecorationMode;
use smithay::reexports::wayland_server::protocol::{wl_buffer, wl_seat, wl_surface::WlSurface};
use smithay::reexports::wayland_server::{Client, Resource};
use smithay::utils::{Rectangle, Serial};
use smithay::wayland::buffer::BufferHandler;
use smithay::wayland::compositor::{get_parent, is_sync_subsurface, CompositorClientState, CompositorHandler, CompositorState};
use smithay::wayland::output::OutputHandler;
use smithay::wayland::selection::data_device::{
    set_data_device_focus, ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
};
use smithay::wayland::selection::SelectionHandler;
use smithay::wayland::shell::xdg::decoration::XdgDecorationHandler;
use smithay::wayland::shell::xdg::{PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState};
use smithay::backend::allocator::dmabuf::Dmabuf;
use smithay::wayland::dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier};
use smithay::wayland::shm::{ShmHandler, ShmState};
use smithay::{
    delegate_compositor, delegate_data_device, delegate_dmabuf, delegate_output, delegate_seat, delegate_shm, delegate_xdg_decoration,
    delegate_xdg_shell,
};

impl CompositorHandler for Station {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            if let Some(id) = self.tasks.find_surface(&root) {
                let t = self.tasks.get(id).unwrap();
                t.window.on_commit();
                // Initial configure, sent once the client has committed its first state.
                if let Some(top) = t.window.toplevel() {
                    if !top.is_initial_configure_sent() {
                        top.send_configure();
                    }
                }
                if t.dialog && self.tasks.focused() == Some(id) {
                    // Recentre dialogs as their size settles.
                    let w = t.window.clone();
                    let geo = w.geometry().size;
                    let out = self.output_size();
                    self.space.map_element(
                        w,
                        ((out.w - geo.w).max(0) / 2, (out.h - geo.h).max(0) / 2),
                        false,
                    );
                }
            }
        }
        self.popups.commit(surface);
        if let Some(PopupKind::Xdg(xdg)) = self.popups.find_popup(surface) {
            if !xdg.is_initial_configure_sent() {
                let _ = xdg.send_configure();
            }
        }
        self.dirty = true;
    }
}

impl BufferHandler for Station {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl ShmHandler for Station {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl SeatHandler for Station {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Station> {
        &mut self.seat_state
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        self.cursor_status = image;
        self.dirty = true;
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        let client = focused.and_then(|s| self.dh.get_client(s.id()).ok());
        set_data_device_focus(&self.dh, seat, client);
    }
}

impl SelectionHandler for Station {
    type SelectionUserData = ();
}

impl DataDeviceHandler for Station {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}
impl ClientDndGrabHandler for Station {}
impl ServerDndGrabHandler for Station {}

impl OutputHandler for Station {}

impl XdgShellHandler for Station {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        self.add_toplevel(surface);
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        self.remove_toplevel(surface.wl_surface());
    }

    fn new_popup(&mut self, surface: PopupSurface, positioner: PositionerState) {
        surface.with_pending_state(|s| s.geometry = positioner.get_geometry());
        self.unconstrain_popup(&surface);
        let _ = self.popups.track_popup(PopupKind::Xdg(surface));
    }

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        positioner: PositionerState,
        token: u32,
    ) {
        surface.with_pending_state(|s| {
            s.geometry = positioner.get_geometry();
            s.positioner = positioner;
        });
        self.unconstrain_popup(&surface);
        surface.send_repositioned(token);
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {}

    fn fullscreen_request(
        &mut self,
        surface: ToplevelSurface,
        _output: Option<smithay::reexports::wayland_server::protocol::wl_output::WlOutput>,
    ) {
        // Text mode: everything is already full-screen.
        if let Some(id) = self.tasks.find_surface(surface.wl_surface()) {
            self.configure_task(id);
        }
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self.tasks.find_surface(surface.wl_surface()) {
            self.configure_task(id);
        }
    }

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        if let Some(id) = self.tasks.find_surface(surface.wl_surface()) {
            self.configure_task(id);
        }
    }

    fn title_changed(&mut self, _surface: ToplevelSurface) {
        self.dirty = true;
    }
}

impl Station {
    fn unconstrain_popup(&self, popup: &PopupSurface) {
        let Ok(root) = find_popup_root_surface(&PopupKind::Xdg(popup.clone())) else {
            return;
        };
        let Some(id) = self.tasks.find_surface(&root) else {
            return;
        };
        let Some(t) = self.tasks.get(id) else { return };
        let Some(loc) = self.space.element_location(&t.window) else {
            return;
        };
        let mut target = Rectangle::from_size(self.output_size());
        target.loc -= loc;
        target.loc -= get_popup_toplevel_coords(&PopupKind::Xdg(popup.clone()));
        popup.with_pending_state(|s| s.geometry = s.positioner.get_unconstrained_geometry(target));
    }
}

impl XdgDecorationHandler for Station {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        // Text mode draws no frames; Desktop (M3) will use server-side frames.
        toplevel.with_pending_state(|s| s.decoration_mode = Some(DecorationMode::ServerSide));
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, _mode: DecorationMode) {
        toplevel.with_pending_state(|s| s.decoration_mode = Some(DecorationMode::ServerSide));
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        toplevel.with_pending_state(|s| s.decoration_mode = Some(DecorationMode::ServerSide));
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
    }
}

delegate_compositor!(Station);
delegate_shm!(Station);
smithay::delegate_viewporter!(Station);
delegate_dmabuf!(Station);

impl DmabufHandler for Station {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        dmabuf: Dmabuf,
        notifier: ImportNotifier,
    ) {
        let ok = match &mut self.backend {
            crate::Backend::Udev(ud) => ud.import_dmabuf(&dmabuf),
            _ => false,
        };
        if ok {
            let _ = notifier.successful::<Station>();
        } else {
            notifier.failed();
        }
    }
}
delegate_seat!(Station);
delegate_data_device!(Station);
delegate_output!(Station);
delegate_xdg_shell!(Station);
delegate_xdg_decoration!(Station);

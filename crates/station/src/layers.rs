//! wlr-layer-shell: surfaces that belong to the Desktop itself rather than
//! to a window — the System Bar along the top (an exclusive zone that
//! windows keep clear of), and desktop icons below the windows. Shown only
//! in the Desktop personality; in RetroDOS they wait, hidden.

use crate::state::{Personality, Station};
use smithay::desktop::{layer_map_for_output, LayerSurface, PopupKind, WindowSurfaceType};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Point, Rectangle};
use smithay::wayland::compositor::with_states;
use smithay::wayland::shell::wlr_layer::{
    KeyboardInteractivity, Layer, LayerSurfaceData, WlrLayerShellHandler, WlrLayerShellState,
};
use smithay::wayland::shell::xdg::PopupSurface;

impl WlrLayerShellHandler for Station {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
    }

    fn new_layer_surface(
        &mut self,
        surface: smithay::wayland::shell::wlr_layer::LayerSurface,
        _output: Option<smithay::reexports::wayland_server::protocol::wl_output::WlOutput>,
        _layer: Layer,
        namespace: String,
    ) {
        let Some(output) = self.output.clone() else {
            return;
        };
        tracing::info!(%namespace, "new layer surface");
        let mut map = layer_map_for_output(&output);
        if let Err(e) = map.map_layer(&LayerSurface::new(surface, namespace)) {
            tracing::warn!("layer surface: {e}");
        }
    }

    fn new_popup(
        &mut self,
        parent: smithay::wayland::shell::wlr_layer::LayerSurface,
        popup: PopupSurface,
    ) {
        self.unconstrain_layer_popup(parent.wl_surface(), &popup);
    }

    fn layer_destroyed(&mut self, surface: smithay::wayland::shell::wlr_layer::LayerSurface) {
        let Some(output) = self.output.clone() else {
            return;
        };
        let changed = {
            let mut map = layer_map_for_output(&output);
            let found = map
                .layers()
                .find(|l| l.layer_surface() == &surface)
                .cloned();
            if let Some(l) = found {
                map.unmap_layer(&l);
            }
            map.arrange()
        };
        if changed {
            self.work_area_changed();
        }
        self.dirty = true;
    }
}
smithay::delegate_layer_shell!(Station);

impl Station {
    /// A commit on a layer surface: lay the layers out again, send the
    /// first configure. Returns false when `surface` is not a layer surface.
    pub fn layer_commit(&mut self, surface: &WlSurface) -> bool {
        let Some(output) = self.output.clone() else {
            return false;
        };
        let changed = {
            let mut map = layer_map_for_output(&output);
            if map
                .layer_for_surface(surface, WindowSurfaceType::TOPLEVEL)
                .is_none()
            {
                return false;
            }
            let changed = map.arrange();
            let sent = with_states(surface, |s| {
                s.data_map
                    .get::<LayerSurfaceData>()
                    .map(|d| d.lock().unwrap().initial_configure_sent)
                    .unwrap_or(true)
            });
            if !sent {
                if let Some(l) = map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL) {
                    l.layer_surface().send_configure();
                }
            }
            changed
        };
        if changed {
            self.work_area_changed();
        }
        self.dirty = true;
        true
    }

    /// The screen minus the layers' exclusive zones (the System Bar).
    pub fn layer_work_area(&self) -> Option<Rectangle<i32, Logical>> {
        let output = self.output.as_ref()?;
        Some(layer_map_for_output(output).non_exclusive_zone())
    }

    /// Maximised windows follow the work area; title bars that ended up
    /// under the System Bar move down out of it.
    fn work_area_changed(&mut self) {
        if self.personality != Personality::Desktop {
            return;
        }
        let top = self.work_area().loc.y;
        let m = self.metrics();
        let low: Vec<u64> = self
            .tasks
            .iter()
            .filter(|t| t.place.placed && !t.place.maximized && !t.place.fullscreen)
            .filter(|t| t.place.rect.loc.y - m.top() < top)
            .map(|t| t.id)
            .collect();
        for id in low {
            if let Some(t) = self.tasks.get_mut(id) {
                t.place.rect.loc.y = top + m.top();
            }
            self.desktop_commit(id);
        }
        let ids: Vec<u64> = self
            .tasks
            .iter()
            .filter(|t| t.place.maximized)
            .map(|t| t.id)
            .collect();
        for id in ids {
            self.desktop_configure(id);
            self.desktop_commit(id);
        }
    }

    /// Layers on `layers`, front first, with where they are.
    pub fn layers_on(&self, layers: &[Layer]) -> Vec<(LayerSurface, Point<i32, Logical>)> {
        let Some(output) = self.output.as_ref() else {
            return vec![];
        };
        let map = layer_map_for_output(output);
        let mut v = Vec::new();
        for l in layers {
            // layers_on lists bottom to top within a layer.
            for s in map.layers_on(*l).rev() {
                if let Some(g) = map.layer_geometry(s) {
                    v.push((s.clone(), g.loc));
                }
            }
        }
        v
    }

    /// The layer surface (and sub-surface or popup) under the pointer.
    pub fn layer_surface_under(
        &self,
        pos: Point<f64, Logical>,
        layers: &[Layer],
    ) -> Option<(LayerSurface, WlSurface, Point<f64, Logical>)> {
        for (l, loc) in self.layers_on(layers) {
            if let Some((s, p)) = l.surface_under(pos - loc.to_f64(), WindowSurfaceType::ALL) {
                return Some((l, s, (p + loc).to_f64()));
            }
        }
        None
    }

    /// Clicking a layer surface that takes the keyboard gives it the keyboard.
    pub fn focus_layer(&mut self, layer: &LayerSurface) {
        let interactivity = layer.cached_state().keyboard_interactivity;
        if interactivity == KeyboardInteractivity::None {
            return;
        }
        let surface = layer.wl_surface().clone();
        if let Some(kb) = self.seat.get_keyboard() {
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            kb.set_focus(self, Some(surface), serial);
        }
    }

    /// Frame callbacks for layer surfaces (Desktop only).
    pub fn layer_frames(&self, output: &smithay::output::Output, now: std::time::Duration) {
        if self.personality != Personality::Desktop {
            return;
        }
        let map = layer_map_for_output(output);
        for l in map.layers() {
            l.send_frame(output, now, Some(std::time::Duration::ZERO), |_, _| {
                Some(output.clone())
            });
        }
    }

    /// Is `root` a layer surface (popup grabs on the System Bar's menus)?
    pub fn is_layer_surface(&self, root: &WlSurface) -> bool {
        self.output.as_ref().is_some_and(|o| {
            layer_map_for_output(o)
                .layer_for_surface(root, WindowSurfaceType::TOPLEVEL)
                .is_some()
        })
    }

    /// Keep a layer surface's popup (a System Bar menu) on the screen.
    pub fn unconstrain_layer_popup(&self, parent: &WlSurface, popup: &PopupSurface) {
        let Some(output) = self.output.as_ref() else {
            return;
        };
        let map = layer_map_for_output(output);
        let Some(layer) = map.layer_for_surface(parent, WindowSurfaceType::TOPLEVEL) else {
            return;
        };
        let Some(geo) = map.layer_geometry(layer) else {
            return;
        };
        let mut target = Rectangle::from_size(self.output_size());
        target.loc -= geo.loc;
        target.loc -= smithay::desktop::get_popup_toplevel_coords(&PopupKind::Xdg(popup.clone()));
        popup.with_pending_state(|s| s.geometry = s.positioner.get_unconstrained_geometry(target));
    }
}

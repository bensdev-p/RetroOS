//! Touch for programs (D-050). A finger on the client area of a program
//! from elsewhere (Firefox, GTK, Qt, X11 programs through Xwayland) is
//! sent to it as wl_touch, so pages scroll and pinch under the fingers;
//! more fingers that follow go to the same program. RetroOS's own windows
//! (app ids `retroos.*`: retro-ui programs, DOS Prompts), the System Bar,
//! Station's frames, menus and desktop keep the one-finger-is-the-mouse
//! behaviour of D-042 — they are drawn for the mouse, and title bars must
//! still drag.

use crate::input::BTN_LEFT;
use crate::state::{Personality, Station};
use crate::tasks::TaskKind;
use smithay::backend::input::{ButtonState, TouchSlot};
use smithay::input::touch::{DownEvent, MotionEvent, UpEvent};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::Resource;
use smithay::utils::{Logical, Point, SERIAL_COUNTER};

/// Fingers being sent to a program as touch.
#[derive(Default)]
pub struct TouchSeq {
    /// The surface they went down on and its place on the screen.
    pub focus: Option<(WlSurface, Point<f64, Logical>)>,
    pub slots: Vec<TouchSlot>,
    /// Where the first finger is now.
    pub first: Option<Point<f64, Logical>>,
}

/// Whether a program takes real touch: everything but RetroOS's own.
pub fn takes_touch(app_id: Option<&str>, kind: TaskKind) -> bool {
    kind != TaskKind::Console && !app_id.is_some_and(|a| a.starts_with("retroos."))
}

/// Where a finger went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Finger {
    /// To a program as wl_touch.
    Touch,
    /// It is the mouse (the left button).
    Mouse,
    /// Neither (a second finger while the first is the mouse).
    Ignored,
}

impl Station {
    /// A finger on the touch screen, at `loc` in the picture's coordinates:
    /// touch for a program from elsewhere, else the left mouse button
    /// (D-042); other fingers while one is the mouse are ignored.
    pub fn finger_down(&mut self, slot: TouchSlot, loc: Point<f64, Logical>, time: u32) -> Finger {
        if self.touch_slot.is_some() {
            return Finger::Ignored;
        }
        if self.touch_down(slot, loc, time) {
            return Finger::Touch;
        }
        self.touch_slot = Some(slot);
        self.pointer_motion(loc, time);
        self.pointer_button(BTN_LEFT, ButtonState::Pressed, time);
        Finger::Mouse
    }

    pub fn finger_motion(&mut self, slot: TouchSlot, loc: Point<f64, Logical>, time: u32) {
        if self.touch_motion(slot, loc, time) {
            return;
        }
        if self.touch_slot == Some(slot) {
            self.pointer_motion(loc, time);
        }
    }

    pub fn finger_up(&mut self, slot: TouchSlot, time: u32) {
        if self.touch_up(slot, time) {
            return;
        }
        if self.touch_slot == Some(slot) {
            self.touch_slot = None;
            self.pointer_button(BTN_LEFT, ButtonState::Released, time);
        }
    }

    pub fn finger_cancel(&mut self, time: u32) {
        if self.touch_cancel() {
            return;
        }
        if self.touch_slot.take().is_some() {
            self.pointer_button(BTN_LEFT, ButtonState::Released, time);
        }
    }

    /// A program asked to be moved or sized (its own title bar, Firefox's
    /// tab strip) while a finger is on it: the program stops getting that
    /// finger, which becomes the mouse there, so the move follows the
    /// finger (HW D.5: it followed the pointer, wherever that was).
    /// Returns where the finger is.
    pub fn touch_to_mouse(&mut self) -> Option<Point<f64, Logical>> {
        let slot = *self.touch.slots.first()?;
        let loc = self.touch.first?;
        if let Some(touch) = self.seat.get_touch() {
            touch.cancel(self);
        }
        self.touch = TouchSeq::default();
        self.touch_slot = Some(slot);
        Some(loc)
    }

    /// The task, surface and surface origin a new finger at `loc` goes to
    /// as touch; None: the finger is the mouse.
    fn touch_target(
        &self,
        loc: Point<f64, Logical>,
    ) -> Option<(u64, WlSurface, Point<f64, Logical>)> {
        if self.task_list.is_some() {
            return None;
        }
        if self.personality == Personality::Desktop && self.desktop_busy() {
            return None;
        }
        let (surface, origin) = self.surface_under(loc)?;
        let client = surface.client()?;
        let t = self
            .tasks
            .iter()
            .find(|t| t.surface().and_then(|s| s.client()).as_ref() == Some(&client))?;
        let app = crate::desktop::app_id(t);
        takes_touch(app.as_deref(), t.kind).then_some((t.id, surface, origin))
    }

    /// A finger came down; true if it went to a program as touch.
    fn touch_down(&mut self, slot: TouchSlot, loc: Point<f64, Logical>, time: u32) -> bool {
        let Some(touch) = self.seat.get_touch() else {
            return false;
        };
        let focus = match self.touch.focus.clone() {
            // More fingers on the same program.
            Some(f) => f,
            None => {
                let Some((id, surface, origin)) = self.touch_target(loc) else {
                    return false;
                };
                if self.personality == Personality::Desktop && self.tasks.focused() != Some(id) {
                    self.focus_task(id);
                }
                (surface, origin)
            }
        };
        self.touch.focus = Some(focus.clone());
        if self.touch.slots.is_empty() {
            self.touch.first = Some(loc);
        }
        self.touch.slots.push(slot);
        let serial = SERIAL_COUNTER.next_serial();
        touch.down(
            self,
            Some(focus),
            &DownEvent {
                slot,
                location: loc,
                serial,
                time,
            },
        );
        touch.frame(self);
        true
    }

    /// True if the finger is one sent as touch.
    fn touch_motion(&mut self, slot: TouchSlot, loc: Point<f64, Logical>, time: u32) -> bool {
        if !self.touch.slots.contains(&slot) {
            return false;
        }
        if self.touch.slots.first() == Some(&slot) {
            self.touch.first = Some(loc);
        }
        if let Some(touch) = self.seat.get_touch() {
            let focus = self.touch.focus.clone();
            touch.motion(
                self,
                focus,
                &MotionEvent {
                    slot,
                    location: loc,
                    time,
                },
            );
            touch.frame(self);
        }
        true
    }

    fn touch_up(&mut self, slot: TouchSlot, time: u32) -> bool {
        let Some(i) = self.touch.slots.iter().position(|s| *s == slot) else {
            return false;
        };
        self.touch.slots.remove(i);
        if let Some(touch) = self.seat.get_touch() {
            let serial = SERIAL_COUNTER.next_serial();
            touch.up(self, &UpEvent { slot, serial, time });
            touch.frame(self);
        }
        if self.touch.slots.is_empty() {
            self.touch.focus = None;
        }
        true
    }

    fn touch_cancel(&mut self) -> bool {
        if self.touch.slots.is_empty() {
            return false;
        }
        if let Some(touch) = self.seat.get_touch() {
            touch.cancel(self);
        }
        self.touch = TouchSeq::default();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_own_windows_keep_the_mouse() {
        assert!(takes_touch(Some("firefox"), TaskKind::App));
        assert!(
            takes_touch(None, TaskKind::App),
            "X11 programs without a class"
        );
        assert!(!takes_touch(Some("retroos.control"), TaskKind::App));
        assert!(!takes_touch(Some("retroos.console"), TaskKind::App));
        assert!(!takes_touch(Some("firefox"), TaskKind::Console));
    }
}

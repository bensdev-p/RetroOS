//! Keyboard, pointer and the Station hotkeys.
//!
//!   Ctrl+Alt+Tab / Ctrl+Alt+Shift+Tab   next / previous task (task swapper)
//!   Ctrl+Alt+F1..F12                    switch virtual terminal
//!   Print Screen                        save a screenshot to C:\SCREENS

use crate::state::Station;
use smithay::backend::input::{
    AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent,
    KeyState, KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent, PointerMotionEvent,
};
use smithay::input::keyboard::{keysyms, FilterResult, XkbConfig};
use smithay::input::pointer::{AxisFrame, ButtonEvent, MotionEvent, RelativeMotionEvent};
use smithay::utils::{Logical, Point, SERIAL_COUNTER};

#[derive(Debug, Clone, Copy)]
pub enum Action {
    NextTask,
    PrevTask,
    Vt(i32),
    Screenshot,
}

/// Keyboard settings from /etc/default/keyboard (Debian's console-setup file).
pub fn xkb_config() -> XkbConfig<'static> {
    let text = std::fs::read_to_string("/etc/default/keyboard").unwrap_or_default();
    let get = |k: &str| -> String {
        text.lines()
            .find_map(|l| l.trim().strip_prefix(k).and_then(|r| r.strip_prefix('=')))
            .map(|v| v.trim().trim_matches('"').to_string())
            .unwrap_or_default()
    };
    let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
    XkbConfig {
        rules: "",
        model: leak(get("XKBMODEL")),
        layout: leak(get("XKBLAYOUT")),
        variant: leak(get("XKBVARIANT")),
        options: Some(get("XKBOPTIONS")).filter(|s| !s.is_empty()),
    }
}

impl Station {
    pub fn process_input_event<B: InputBackend>(&mut self, event: InputEvent<B>) {
        match event {
            InputEvent::Keyboard { event } => {
                let code: u32 = event.key_code().into();
                self.handle_key(code, event.state(), Event::time_msec(&event));
            }
            InputEvent::PointerMotion { event } => {
                let delta = event.delta();
                let loc = self.clamp(self.pointer_location + delta);
                self.pointer_motion(loc, event.time_msec());
                if let Some(ptr) = self.seat.get_pointer() {
                    let under = self.surface_under(loc);
                    ptr.relative_motion(
                        self,
                        under,
                        &RelativeMotionEvent {
                            delta,
                            delta_unaccel: event.delta_unaccel(),
                            utime: event.time(),
                        },
                    );
                    ptr.frame(self);
                }
            }
            InputEvent::PointerMotionAbsolute { event } => {
                let size = self.output_size();
                let loc = event.position_transformed(size);
                self.pointer_motion(loc, event.time_msec());
            }
            InputEvent::PointerButton { event } => {
                let Some(ptr) = self.seat.get_pointer() else {
                    return;
                };
                let serial = SERIAL_COUNTER.next_serial();
                ptr.button(
                    self,
                    &ButtonEvent {
                        button: event.button_code(),
                        state: event.state(),
                        serial,
                        time: event.time_msec(),
                    },
                );
                ptr.frame(self);
                let _ = ButtonState::Pressed;
            }
            InputEvent::PointerAxis { event } => {
                let Some(ptr) = self.seat.get_pointer() else {
                    return;
                };
                let source = event.source();
                let mut frame = AxisFrame::new(event.time_msec()).source(source);
                for axis in [Axis::Horizontal, Axis::Vertical] {
                    let amount = event
                        .amount(axis)
                        .unwrap_or_else(|| event.amount_v120(axis).unwrap_or(0.0) * 15.0 / 120.0);
                    if amount != 0.0 {
                        frame = frame.value(axis, amount);
                        if let Some(v120) = event.amount_v120(axis) {
                            frame = frame.v120(axis, v120 as i32);
                        }
                    } else if source == AxisSource::Finger {
                        frame = frame.stop(axis);
                    }
                }
                ptr.axis(self, frame);
                ptr.frame(self);
            }
            _ => {}
        }
    }

    /// One key event (xkb keycode = evdev + 8), hotkeys first.
    pub fn handle_key(&mut self, code: u32, state: KeyState, time: u32) {
        let serial = SERIAL_COUNTER.next_serial();
        let Some(kb) = self.seat.get_keyboard() else {
            return;
        };
        let action =
            kb.input::<Action, _>(self, code.into(), state, serial, time, |_, mods, handle| {
                if state != KeyState::Pressed {
                    return FilterResult::Forward;
                }
                let sym = handle.modified_sym().raw();
                if mods.ctrl && mods.alt {
                    if sym == keysyms::KEY_Tab || sym == keysyms::KEY_ISO_Left_Tab {
                        return FilterResult::Intercept(
                            if mods.shift || sym == keysyms::KEY_ISO_Left_Tab {
                                Action::PrevTask
                            } else {
                                Action::NextTask
                            },
                        );
                    }
                    if (keysyms::KEY_XF86Switch_VT_1..=keysyms::KEY_XF86Switch_VT_12).contains(&sym)
                    {
                        return FilterResult::Intercept(Action::Vt(
                            (sym - keysyms::KEY_XF86Switch_VT_1 + 1) as i32,
                        ));
                    }
                }
                if sym == keysyms::KEY_Print {
                    return FilterResult::Intercept(Action::Screenshot);
                }
                FilterResult::Forward
            });
        if let Some(a) = action {
            self.run_action(a);
        }
    }

    fn clamp(&self, p: Point<f64, Logical>) -> Point<f64, Logical> {
        let s = self.output_size();
        (
            p.x.clamp(0.0, s.w as f64 - 1.0),
            p.y.clamp(0.0, s.h as f64 - 1.0),
        )
            .into()
    }

    pub fn pointer_motion(&mut self, loc: Point<f64, Logical>, time: u32) {
        self.pointer_location = loc;
        let under = self.surface_under(loc);
        if let Some(ptr) = self.seat.get_pointer() {
            ptr.motion(
                self,
                under,
                &MotionEvent {
                    location: loc,
                    serial: SERIAL_COUNTER.next_serial(),
                    time,
                },
            );
            ptr.frame(self);
        }
        self.dirty = true;
    }

    pub fn run_action(&mut self, a: Action) {
        tracing::debug!(?a, "hotkey");
        match a {
            Action::NextTask => self.cycle_tasks(true),
            Action::PrevTask => self.cycle_tasks(false),
            Action::Vt(n) => crate::change_vt(self, n),
            Action::Screenshot => {
                let dir = std::env::var_os("HOME")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_default()
                    .join("SCREENS");
                let _ = std::fs::create_dir_all(&dir);
                let n = std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0) + 1;
                let path = dir.join(format!("SCREEN{n:02}.PNG"));
                if let Err(e) = crate::screenshot(self, &path) {
                    tracing::warn!("screenshot failed: {e}");
                }
            }
        }
    }
}

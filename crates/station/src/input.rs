//! Keyboard, pointer and the Station hotkeys.
//!
//!   Ctrl+Alt+Tab / Ctrl+Alt+Shift+Tab   next / previous task (task swapper)
//!   Ctrl+Alt+Esc, Ctrl+Alt+Del          Task List (tasklist.rs)
//!   Ctrl+Alt+F1..F12                    switch virtual terminal
//!   Print Screen                        save a screenshot to C:\SCREENS
//! and in the Desktop (desktop.rs): Alt+Tab, Alt+Esc, Alt+F4, Alt+Space,
//! Alt+Enter, and Ctrl+Alt+F1 = Exit to DOS (the session's own VT is 1).

use crate::state::{Personality, Station};
use crate::tasks::TaskKind;
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
    TaskList,
    TaskListKey(u32),
    /// Alt+Tab (true) / Alt+Shift+Tab.
    Switch(bool),
    SwitchCancel,
    AltEsc,
    Close,
    WindowMenu,
    Fullscreen,
    ExitToDos,
    MenuKey(u32),
    MessageKey(u32),
    DragKey(u32),
    /// Taken by Station, does nothing.
    Swallow,
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
                self.pointer_button(event.button_code(), event.state(), event.time_msec());
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
        let action = kb.input::<Action, _>(
            self,
            code.into(),
            state,
            serial,
            time,
            |st, mods, handle| {
                if state != KeyState::Pressed {
                    return FilterResult::Forward;
                }
                let sym = handle.modified_sym().raw();
                if mods.ctrl
                    && mods.alt
                    && (sym == keysyms::KEY_Escape || sym == keysyms::KEY_Delete)
                {
                    return FilterResult::Intercept(Action::TaskList);
                }
                let switch_vt =
                    (keysyms::KEY_XF86Switch_VT_1..=keysyms::KEY_XF86Switch_VT_12).contains(&sym);
                if st.task_list.is_some() && !switch_vt {
                    return FilterResult::Intercept(Action::TaskListKey(sym));
                }
                if st.personality == Personality::Desktop {
                    if let Some(a) = st.desktop_hotkey(sym, mods.ctrl, mods.alt, mods.shift) {
                        return FilterResult::Intercept(a);
                    }
                }
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
            },
        );
        if let Some(a) = action {
            self.run_action(a);
        }
        // Alt let go during Alt+Tab: switch.
        if self.desktop.switcher.is_some()
            && !self
                .seat
                .get_keyboard()
                .is_some_and(|k| k.modifier_state().alt)
        {
            self.switch_finish();
        }
    }

    /// Desktop keys Station takes for itself.
    fn desktop_hotkey(&self, sym: u32, ctrl: bool, alt: bool, shift: bool) -> Option<Action> {
        let d = &self.desktop;
        if d.message.is_some() {
            return Some(Action::MessageKey(sym));
        }
        if d.menu.is_some() {
            return Some(Action::MenuKey(sym));
        }
        if d.captures_keys() && d.switcher.is_none() {
            return Some(Action::DragKey(sym));
        }
        let tab = sym == keysyms::KEY_Tab || sym == keysyms::KEY_ISO_Left_Tab;
        let back = shift || sym == keysyms::KEY_ISO_Left_Tab;
        if d.switcher.is_some() {
            return Some(match sym {
                _ if tab => Action::Switch(!back),
                keysyms::KEY_Escape => Action::SwitchCancel,
                _ => Action::Swallow,
            });
        }
        if alt && !ctrl {
            let console = self
                .tasks
                .focused()
                .and_then(|f| self.tasks.get(f))
                .is_some_and(|t| t.kind == TaskKind::Console);
            return match sym {
                _ if tab => Some(Action::Switch(!back)),
                keysyms::KEY_Escape => Some(Action::AltEsc),
                keysyms::KEY_F4 => Some(Action::Close),
                keysyms::KEY_space => Some(Action::WindowMenu),
                keysyms::KEY_Return if console => Some(Action::Fullscreen),
                _ => None,
            };
        }
        if ctrl && alt && sym == keysyms::KEY_XF86Switch_VT_1 {
            return Some(Action::ExitToDos);
        }
        None
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
        self.pointer_moved = true;
        if self.personality == Personality::Desktop {
            self.desktop_motion(loc);
        }
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

    /// A mouse button: the Desktop's frames and menus first, then the program.
    pub fn pointer_button(&mut self, button: u32, state: ButtonState, time: u32) {
        let Some(ptr) = self.seat.get_pointer() else {
            return;
        };
        let desktop = self.personality == Personality::Desktop && self.task_list.is_none();
        if desktop
            && !ptr.is_grabbed()
            && self.desktop_button(button, state == ButtonState::Pressed, time)
        {
            // Programs under the pointer lose it while Station has it.
            let loc = self.pointer_location;
            self.pointer_motion(loc, time);
            return;
        }
        let serial = SERIAL_COUNTER.next_serial();
        ptr.button(
            self,
            &ButtonEvent {
                button,
                state,
                serial,
                time,
            },
        );
        ptr.frame(self);
    }

    pub fn run_action(&mut self, a: Action) {
        tracing::debug!(?a, "hotkey");
        match a {
            Action::NextTask => self.cycle_tasks(true),
            Action::PrevTask => self.cycle_tasks(false),
            Action::Vt(n) => crate::change_vt(self, n),
            Action::TaskList => {
                if self.task_list.is_none() {
                    self.open_task_list();
                }
            }
            Action::TaskListKey(sym) => self.task_list_key(sym),
            Action::Switch(fwd) => self.switch_step(fwd),
            Action::SwitchCancel => self.switch_cancel(),
            Action::AltEsc => self.alt_esc(),
            Action::Close => {
                if let Some(f) = self.tasks.focused() {
                    self.close_task(f);
                }
            }
            Action::WindowMenu => {
                if let Some(f) = self.tasks.focused() {
                    self.open_window_menu(f, false);
                }
            }
            Action::Fullscreen => {
                if let Some(f) = self.tasks.focused() {
                    let on = self.tasks.get(f).is_some_and(|t| !t.place.fullscreen);
                    self.set_fullscreen(f, on);
                }
            }
            Action::ExitToDos => self.set_personality(Personality::Text),
            Action::MenuKey(sym) => self.menu_key(sym),
            Action::MessageKey(sym) => self.message_key(sym),
            Action::DragKey(sym) => self.drag_key(sym),
            Action::Swallow => {}
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

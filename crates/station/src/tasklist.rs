//! The Task List (DOS 5 Shell / Windows 3.1): Ctrl+Alt+Esc or Ctrl+Alt+Del
//! shows every task full-screen, drawn by Station itself so it works even
//! when the program on screen has stopped responding.
//!
//!   ↑ ↓      choose a task         Enter / S   switch to it
//!   E / Del  end it (ask to close) K           end it now (kill)
//!   Esc      back to the task you were in

use crate::state::Station;
use crate::tasks::TaskKind;
use retro_gfx::font::phosphor_mono;
use retro_gfx::grid::fit_scale;
use retro_gfx::{Canvas, Grid, Scheme};
use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::memory::MemoryRenderBuffer;
use smithay::input::keyboard::keysyms;
use smithay::utils::{Transform, SERIAL_COUNTER};

pub struct TaskList {
    pub cursor: usize,
    /// Task ids in display order, taken when the list opened.
    ids: Vec<u64>,
    /// The task to return to on Esc.
    back: Option<u64>,
    buffer: Option<MemoryRenderBuffer>,
    note: Option<String>,
}

const BUTTONS: &[&str] = &["Switch To", "End Task", "Cancel"];

impl TaskList {
    /// The rendered list (after `Station::task_list_buffer`).
    pub fn buffer(&self) -> Option<&MemoryRenderBuffer> {
        self.buffer.as_ref()
    }
}

impl Station {
    pub fn open_task_list(&mut self) {
        let ids: Vec<u64> = self
            .tasks
            .iter()
            .filter(|t| !t.dialog)
            .map(|t| t.id)
            .collect();
        let back = self.tasks.focused();
        let cursor = back
            .and_then(|b| ids.iter().position(|i| *i == b))
            .unwrap_or(0);
        self.task_list = Some(TaskList {
            cursor,
            ids,
            back,
            buffer: None,
            note: None,
        });
        // Nothing gets keys while the list is up.
        if let Some(kb) = self.seat.get_keyboard() {
            kb.set_focus(self, None, SERIAL_COUNTER.next_serial());
        }
        self.dirty = true;
    }

    fn close_task_list(&mut self, to: Option<u64>) {
        self.task_list = None;
        if let Some(id) = to
            .filter(|id| self.tasks.get(*id).is_some())
            .or(self.tasks.most_recent())
        {
            self.focus_task(id);
        }
        self.dirty = true;
    }

    /// A key pressed while the list is open.
    pub fn task_list_key(&mut self, sym: u32) {
        let Some(tl) = self.task_list.as_mut() else {
            return;
        };
        tl.buffer = None;
        tl.note = None;
        let n = tl.ids.len();
        let current = tl.ids.get(tl.cursor).copied();
        let c = char::from_u32(sym).map(|c| c.to_ascii_lowercase());
        match sym {
            keysyms::KEY_Up | keysyms::KEY_KP_Up => tl.cursor = tl.cursor.saturating_sub(1),
            keysyms::KEY_Down | keysyms::KEY_KP_Down => {
                tl.cursor = (tl.cursor + 1).min(n.saturating_sub(1))
            }
            keysyms::KEY_Escape => {
                let back = tl.back;
                self.close_task_list(back);
            }
            keysyms::KEY_Return | keysyms::KEY_KP_Enter => self.close_task_list(current),
            keysyms::KEY_Delete => self.end_task(current, false),
            _ => match c {
                Some('s') => self.close_task_list(current),
                Some('e') => self.end_task(current, false),
                Some('k') => self.end_task(current, true),
                Some('c') => {
                    let back = tl.back;
                    self.close_task_list(back);
                }
                _ => {}
            },
        }
        self.dirty = true;
    }

    fn end_task(&mut self, id: Option<u64>, force: bool) {
        let Some(t) = id.and_then(|id| self.tasks.get(id)) else {
            return;
        };
        let note = if t.kind == TaskKind::Console && t.pid == self.primary_pid {
            Some("This is the main RetroDOS session; type EXIT there to end it.".to_string())
        } else if force {
            match t.pid {
                Some(pid) => {
                    unsafe { libc::kill(pid as i32, libc::SIGKILL) };
                    Some(format!("{} was ended.", t.name))
                }
                None => Some("This task cannot be ended from here.".into()),
            }
        } else {
            if let Some(top) = t.window.toplevel() {
                top.send_close();
            }
            Some(format!(
                "Asked {} to close. If it does not, press K to end it now.",
                t.name
            ))
        };
        if let Some(tl) = self.task_list.as_mut() {
            tl.note = note;
        }
    }

    /// Called when a task goes away while the list is open.
    pub fn task_list_forget(&mut self, id: u64) {
        if let Some(tl) = self.task_list.as_mut() {
            tl.ids.retain(|i| *i != id);
            tl.cursor = tl.cursor.min(tl.ids.len().saturating_sub(1));
            tl.buffer = None;
            if tl.back == Some(id) {
                tl.back = None;
            }
        }
    }

    /// The list as a full-screen image (rebuilt only when it changed).
    pub fn task_list_buffer(&mut self) -> Option<&MemoryRenderBuffer> {
        let size = self.output_size();
        let scale = self.scale.max(1);
        let (pw, ph) = ((size.w * scale) as u32, (size.h * scale) as u32);
        let lines: Vec<String> = match &self.task_list {
            Some(tl) if tl.buffer.is_none() => tl
                .ids
                .iter()
                .filter_map(|id| self.tasks.get(*id))
                .map(|t| {
                    let title = self.task_title(t);
                    let name = if t.kind == TaskKind::Console {
                        "RETRODOS".to_string()
                    } else {
                        t.name.clone()
                    };
                    if title.is_empty() || title.eq_ignore_ascii_case(&name) {
                        name
                    } else {
                        format!("{name} - {title}")
                    }
                })
                .collect(),
            Some(_) => Vec::new(),
            None => return None,
        };
        let tl = self.task_list.as_mut()?;
        if tl.buffer.is_none() {
            let canvas = draw(&lines, tl.cursor, tl.note.as_deref(), pw, ph);
            let mut px = Vec::with_capacity((pw * ph * 4) as usize);
            for p in &canvas.px {
                px.extend_from_slice(&(p | 0xFF00_0000).to_le_bytes());
            }
            tl.buffer = Some(MemoryRenderBuffer::from_slice(
                &px,
                Fourcc::Xrgb8888,
                (pw as i32, ph as i32),
                scale,
                Transform::Normal,
                None,
            ));
        }
        tl.buffer.as_ref()
    }
}

/// Draw the list into a canvas of the output's physical size.
fn draw(lines: &[String], cursor: usize, note: Option<&str>, w: u32, h: u32) -> Canvas {
    let f = phosphor_mono();
    let scale = fit_scale(w.max(8), h.max(16), 8, 16, 80, 25);
    let (cols, rows) = (w / (8 * scale), h / (16 * scale));
    let scheme = Scheme::CLASSIC;
    let mut g = Grid::new(cols, rows, &scheme);
    // Phosphor16 indices: 0 black, 1 cobalt, 3 teal, 7 silver, 14 lemon, 15 white.
    g.fill(0, 0, cols, 1, ' ', 0, 3);
    g.put(1, 0, "RetroOS Task List", 0, 3);
    g.fill(0, rows - 1, cols, 1, ' ', 0, 3);
    g.put(
        1,
        rows - 1,
        "Enter Switch To   E End Task   K End Now   Esc Cancel",
        0,
        3,
    );
    let bw = 60.min(cols.saturating_sub(4));
    let list_rows = (lines.len() as u32).clamp(1, rows.saturating_sub(12));
    let bh = list_rows + 8;
    let bx = (cols - bw) / 2;
    let by = (rows.saturating_sub(bh)) / 2;
    g.dialog(bx, by, bw, bh, "Task List", 0, 7, true);
    let top = (cursor as u32).saturating_sub(list_rows - 1);
    for (i, l) in lines
        .iter()
        .enumerate()
        .skip(top as usize)
        .take(list_rows as usize)
    {
        let y = by + 2 + i as u32 - top;
        let selected = i == cursor;
        let (fg, bg) = if selected { (15, 1) } else { (0, 7) };
        g.fill(bx + 2, y, bw - 4, 1, ' ', fg, bg);
        let text: String = l.chars().take((bw - 6) as usize).collect();
        g.put(bx + 3, y, &text, fg, bg);
    }
    if let Some(n) = note {
        let text: String = n.chars().take((bw - 4) as usize).collect();
        g.put(bx + 2, by + bh - 4, &text, 4, 7);
    }
    let labels: Vec<String> = BUTTONS.iter().map(|b| format!("< {b} >")).collect();
    let total: u32 = labels.iter().map(|l| l.chars().count() as u32 + 2).sum();
    let mut x = bx + (bw.saturating_sub(total)) / 2;
    for l in &labels {
        g.put(x, by + bh - 2, l, 0, 7);
        x += l.chars().count() as u32 + 2;
    }
    let mut c = Canvas::new(w, h, 0);
    g.render(f, &mut c, scale);
    c
}

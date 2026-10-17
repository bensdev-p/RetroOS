//! The task list: every toplevel window is a task. In RetroDOS the focused
//! task fills the screen and Ctrl+Alt+Tab cycles (the DOS 5 task swapper).

use smithay::desktop::Window;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskKind {
    /// A RetroDOS console session.
    Console,
    /// Any other program.
    App,
}

pub struct Task {
    pub id: u64,
    pub window: Window,
    pub pid: Option<u32>,
    pub kind: TaskKind,
    /// Broker launch request that started this task, if any.
    pub launch: Option<u64>,
    /// Program name: what it was launched as (FIREFOX), else the process name.
    pub name: String,
    /// Has a parent toplevel (dialogs are not full-screen).
    pub dialog: bool,
}

#[derive(Default)]
pub struct Tasks {
    list: Vec<Task>,
    /// Most-recently-focused first.
    mru: Vec<u64>,
    next_id: u64,
}

impl Tasks {
    pub fn add(&mut self, mut t: Task) -> u64 {
        self.next_id += 1;
        t.id = self.next_id;
        self.list.push(t);
        self.next_id
    }

    pub fn remove(&mut self, id: u64) -> Option<Task> {
        self.mru.retain(|i| *i != id);
        let pos = self.list.iter().position(|t| t.id == id)?;
        Some(self.list.remove(pos))
    }

    pub fn get(&self, id: u64) -> Option<&Task> {
        self.list.iter().find(|t| t.id == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Task> {
        self.list.iter()
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn focused(&self) -> Option<u64> {
        self.mru.first().copied()
    }

    pub fn most_recent(&self) -> Option<u64> {
        self.mru
            .first()
            .copied()
            .or_else(|| self.list.last().map(|t| t.id))
    }

    pub fn set_focus(&mut self, id: u64) {
        self.mru.retain(|i| *i != id);
        self.mru.insert(0, id);
    }

    pub fn find_surface(&self, s: &WlSurface) -> Option<u64> {
        self.list
            .iter()
            .find(|t| t.window.toplevel().is_some_and(|top| top.wl_surface() == s))
            .map(|t| t.id)
    }

    /// Next (or previous) task in creation order, skipping dialogs.
    pub fn cycle(&self, forward: bool) -> Option<u64> {
        let ids: Vec<u64> = self
            .list
            .iter()
            .filter(|t| !t.dialog)
            .map(|t| t.id)
            .collect();
        if ids.is_empty() {
            return None;
        }
        let cur = self
            .focused()
            .and_then(|f| ids.iter().position(|i| *i == f))
            .unwrap_or(0);
        let n = ids.len();
        Some(
            ids[if forward {
                (cur + 1) % n
            } else {
                (cur + n - 1) % n
            }],
        )
    }

    /// Children (dialogs) of a task's window, drawn over it.
    pub fn dialogs_of(&self, id: u64) -> Vec<&Task> {
        let Some(parent) = self
            .get(id)
            .and_then(|t| t.window.toplevel().map(|top| top.wl_surface().clone()))
        else {
            return vec![];
        };
        self.list
            .iter()
            .filter(|t| {
                t.dialog
                    && t.window.toplevel().and_then(|top| top.parent()).as_ref() == Some(&parent)
            })
            .collect()
    }
}

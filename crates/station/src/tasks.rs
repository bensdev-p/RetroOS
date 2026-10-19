//! The task list: every toplevel window is a task. In RetroDOS the focused
//! task fills the screen and Ctrl+Alt+Tab cycles (the DOS 5 task swapper);
//! in the Desktop the most recently focused window is on top.

use smithay::desktop::Window;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Rectangle, Size};

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
    /// Where the window sits in the Desktop.
    pub place: Place,
}

/// A window's state in the Desktop personality (kept while in RetroDOS, so
/// windows come back where they were).
#[derive(Clone, Debug, Default)]
pub struct Place {
    /// Client area in its normal (not maximised) state; valid once `placed`.
    pub rect: Rectangle<i32, Logical>,
    pub placed: bool,
    pub maximized: bool,
    pub minimized: bool,
    pub fullscreen: bool,
    /// Icon position (slot number) while minimised.
    pub slot: Option<usize>,
    /// A resize asked for and not yet committed (the opposite edges stay put).
    pub resizing: Option<Resizing>,
}

/// The edges that moved, the client area asked for, and the size before.
pub type Resizing = (u8, Rectangle<i32, Logical>, Size<i32, Logical>);

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

    /// Most-recently-focused first (never-focused tasks last).
    pub fn mru(&self) -> Vec<u64> {
        let mut v = self.mru.clone();
        v.extend(
            self.list
                .iter()
                .map(|t| t.id)
                .filter(|id| !self.mru.contains(id)),
        );
        v
    }

    /// Put a task at the bottom of the stack (Alt+Esc, minimise).
    pub fn send_to_back(&mut self, id: u64) {
        self.mru.retain(|i| *i != id);
        self.mru.push(id);
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Task> {
        self.list.iter_mut().find(|t| t.id == id)
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Task> {
        self.list.iter_mut()
    }

    /// The task a dialog belongs to.
    pub fn parent_of(&self, id: u64) -> Option<u64> {
        let t = self.get(id).filter(|t| t.dialog)?;
        let p = t.window.toplevel()?.parent()?;
        self.find_surface(&p).filter(|p| *p != id)
    }

    /// The top-level task a dialog (or a dialog of a dialog) belongs to.
    pub fn root_of(&self, id: u64) -> u64 {
        let mut cur = id;
        for _ in 0..8 {
            match self.parent_of(cur) {
                Some(p) => cur = p,
                None => break,
            }
        }
        cur
    }

    /// Desktop stacking order, top first: windows in focus order, each
    /// with its dialogs just above it.
    pub fn z_order(&self) -> Vec<u64> {
        let order = self.mru();
        let roots: Vec<u64> = order.iter().map(|id| self.root_of(*id)).collect();
        let mut out = Vec::with_capacity(order.len());
        for (i, r) in roots.iter().enumerate() {
            if roots[..i].contains(r) {
                continue;
            }
            for (d, rd) in order.iter().zip(&roots) {
                if rd == r && d != r {
                    out.push(*d);
                }
            }
            out.push(*r);
        }
        out
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

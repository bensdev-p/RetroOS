//! The launch broker: a small line-based protocol on a Unix socket that lets
//! RetroDOS (and tests) talk to Station. The socket path is exported to every
//! client as `$RETROOS_STATION`.
//!
//! Requests (one per line, fields separated by TAB):
//!   PING                          → OK station <version>
//!   TASKS                         → OK <n>, then n lines: TASK <id> <pid> <kind> <focused> <name> <title>
//!   SWITCH <id>                   → OK | ERR <msg>
//!   LAUNCH <wait|nowait> <cwd> [=NAME=VALUE…] <argv0> [argv…]
//!                                 → OK <launch-id> <pid>; later EXIT <launch-id> <code>
//!                                   (fields starting with '=' set environment variables)
//!   KILL <launch-id>              → OK | ERR <msg>   (SIGTERM to the launched process)
//!   END <task-id> [FORCE]         → OK | ERR <msg>   (ask the window to close; FORCE kills)
//!   KEYMAP                        → OK | ERR <msg>   (reload /etc/default/keyboard)
//!   SCREENSHOT <path>             → OK | ERR <msg>
//! Test-only (station --test-ipc):
//!   TYPE <text>   KEY <combo>…   QUIT

use crate::state::Station;
use smithay::reexports::calloop::{generic::Generic, Interest, LoopHandle, Mode, PostAction};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

pub struct Broker {
    path: PathBuf,
    conns: HashMap<u64, UnixStream>,
    /// launch id → (connection, requesting task)
    launches: HashMap<u64, (u64, Option<u64>)>,
    /// launch id → the name it was launched as (`=RETROOS_LAUNCH_NAME=`)
    names: HashMap<u64, String>,
    next: u64,
    pub test_ipc: bool,
}

impl Broker {
    pub fn new(handle: &LoopHandle<'static, Station>, socket_name: &OsStr) -> Broker {
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let path = dir.join(format!(
            "retroos-station-{}.sock",
            socket_name.to_string_lossy()
        ));
        let _ = std::fs::remove_file(&path);
        match UnixListener::bind(&path) {
            Ok(l) => {
                l.set_nonblocking(true).ok();
                let _ = handle.insert_source(
                    Generic::new(l, Interest::READ, Mode::Level),
                    |_, l, st: &mut Station| {
                        while let Ok((stream, _)) = l.accept() {
                            st.broker_accept(stream);
                        }
                        Ok(PostAction::Continue)
                    },
                );
            }
            Err(e) => tracing::warn!("broker socket {}: {e}", path.display()),
        }
        Broker {
            path,
            conns: HashMap::new(),
            launches: HashMap::new(),
            names: HashMap::new(),
            next: 0,
            test_ipc: false,
        }
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    fn send(&mut self, conn: u64, line: &str) {
        if let Some(s) = self.conns.get_mut(&conn) {
            if s.write_all(format!("{line}\n").as_bytes()).is_err() {
                self.conns.remove(&conn);
            }
        }
    }

    pub fn launch_name(&self, launch: u64) -> Option<String> {
        self.names.get(&launch).cloned()
    }

    pub fn launch_exited(&mut self, launch: u64, code: i32) {
        self.names.remove(&launch);
        if let Some((conn, _)) = self.launches.remove(&launch) {
            self.send(conn, &format!("EXIT\t{launch}\t{code}"));
        }
    }

    /// The task that asked for a launch (focus returns there when it exits).
    pub fn requester_task(&self, launch: u64) -> Option<u64> {
        self.launches.get(&launch).and_then(|(_, t)| *t)
    }
}

impl Drop for Broker {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn peer_pid(s: &UnixStream) -> Option<u32> {
    use std::os::fd::AsRawFd;
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let r = unsafe {
        libc::getsockopt(
            s.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    (r == 0).then_some(cred.pid as u32)
}

impl Station {
    fn broker_accept(&mut self, stream: UnixStream) {
        self.broker.next += 1;
        let id = self.broker.next;
        let Ok(reader) = stream.try_clone() else {
            return;
        };
        reader.set_nonblocking(true).ok();
        self.broker.conns.insert(id, stream);
        let mut buf: Vec<u8> = Vec::new();
        let _ = self.loop_handle.insert_source(
            Generic::new(reader, Interest::READ, Mode::Level),
            move |_, r, st: &mut Station| {
                let mut chunk = [0u8; 4096];
                loop {
                    // SAFETY: we only read from the stream; calloop keeps it registered.
                    match unsafe { r.get_mut() }.read(&mut chunk) {
                        Ok(0) => {
                            st.broker.conns.remove(&id);
                            return Ok(PostAction::Remove);
                        }
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(_) => {
                            st.broker.conns.remove(&id);
                            return Ok(PostAction::Remove);
                        }
                    }
                }
                while let Some(nl) = buf.iter().position(|b| *b == b'\n') {
                    let line = String::from_utf8_lossy(&buf[..nl])
                        .trim_end_matches('\r')
                        .to_string();
                    buf.drain(..=nl);
                    st.broker_request(id, &line);
                }
                Ok(PostAction::Continue)
            },
        );
    }

    fn broker_request(&mut self, conn: u64, line: &str) {
        let f: Vec<&str> = line.split('\t').collect();
        let reply = match f[0].to_ascii_uppercase().as_str() {
            "PING" => format!("OK\tstation\t{}", env!("CARGO_PKG_VERSION")),
            "TASKS" => {
                let focused = self.tasks.focused();
                let mut lines = vec![format!("OK\t{}", self.tasks.len())];
                for t in self.tasks.iter() {
                    lines.push(format!(
                        "TASK\t{}\t{}\t{:?}\t{}\t{}\t{}",
                        t.id,
                        t.pid.unwrap_or(0),
                        t.kind,
                        (focused == Some(t.id)) as u8,
                        t.name,
                        self.task_title(t).replace(['\t', '\n'], " ")
                    ));
                }
                lines.join("\n")
            }
            "SWITCH" => match f.get(1).and_then(|s| s.parse::<u64>().ok()) {
                Some(id) if self.tasks.get(id).is_some() => {
                    self.focus_task(id);
                    "OK".into()
                }
                _ => "ERR\tno such task".into(),
            },
            "LAUNCH" if f.len() >= 4 => {
                let cwd = Some(f[2]).filter(|s| !s.is_empty());
                let env: Vec<(String, String)> = f[3..]
                    .iter()
                    .map_while(|s| s.strip_prefix('='))
                    .filter_map(|kv| kv.split_once('='))
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect();
                let argv: Vec<String> = f[3..]
                    .iter()
                    .skip_while(|s| s.starts_with('='))
                    .map(|s| s.to_string())
                    .collect();
                if argv.is_empty() {
                    self.broker.send(conn, "ERR	no program");
                    return;
                }
                self.broker.next += 1;
                let lid = self.broker.next;
                let requester = self
                    .broker
                    .conns
                    .get(&conn)
                    .and_then(peer_pid)
                    .and_then(|p| self.task_for_pid(p));
                match self.spawn(&argv, cwd, &env, Some(lid)) {
                    Ok(pid) => {
                        // Graphical programs get the GPU renderer (and
                        // linux-dmabuf). Switching after the spawn overlaps
                        // Mesa's start-up with the program's own; the program
                        // cannot connect before we return to the event loop,
                        // so it still sees the GPU from its first request.
                        crate::udev::set_gpu(self, true);
                        self.broker.launches.insert(lid, (conn, requester));
                        if let Some((_, n)) = env.iter().find(|(k, _)| k == "RETROOS_LAUNCH_NAME") {
                            self.broker.names.insert(lid, n.clone());
                        }
                        format!("OK\t{lid}\t{pid}")
                    }
                    Err(e) => format!("ERR\t{e}"),
                }
            }
            "KILL" if f.len() == 2 => {
                let lid = f[1].parse::<u64>().ok();
                match self
                    .children
                    .iter()
                    .find(|(_, l)| lid.is_some() && *l == lid)
                {
                    Some((c, _)) => {
                        unsafe { libc::kill(c.id() as i32, libc::SIGTERM) };
                        "OK".into()
                    }
                    None => "ERR	no such program".into(),
                }
            }
            "END" if f.len() >= 2 => {
                let force = f.get(2).is_some_and(|s| s.eq_ignore_ascii_case("FORCE"));
                match f[1].parse::<u64>().ok().and_then(|id| self.tasks.get(id)) {
                    Some(t) if force => match t.pid {
                        Some(pid) if Some(pid) != self.primary_pid => {
                            unsafe { libc::kill(pid as i32, libc::SIGKILL) };
                            "OK".into()
                        }
                        _ => "ERR	cannot end this task".into(),
                    },
                    Some(t) => {
                        if let Some(top) = t.window.toplevel() {
                            top.send_close();
                        }
                        "OK".into()
                    }
                    None => "ERR	no such task".into(),
                }
            }
            "KEYMAP" => {
                // /etc/default/keyboard changed (KEYB): load it now.
                let cfg = crate::input::xkb_config();
                match self
                    .seat
                    .get_keyboard()
                    .map(|kb| kb.set_xkb_config(self, cfg))
                {
                    Some(Ok(())) => "OK".into(),
                    Some(Err(e)) => format!("ERR\t{e:?}"),
                    None => "ERR\tno keyboard".into(),
                }
            }
            "SCREENSHOT" if f.len() == 2 => {
                match crate::screenshot(self, std::path::Path::new(f[1])) {
                    Ok(()) => "OK".into(),
                    Err(e) => format!("ERR\t{e}"),
                }
            }
            "TYPE" if self.broker.test_ipc && f.len() >= 2 => {
                self.inject_text(&f[1..].join("\t"));
                "OK".into()
            }
            "KEY" if self.broker.test_ipc && f.len() >= 2 => {
                for combo in &f[1..] {
                    if !self.inject_combo(combo) {
                        self.broker.send(conn, &format!("ERR\tunknown key {combo}"));
                        return;
                    }
                }
                "OK".into()
            }
            "QUIT" if self.broker.test_ipc => {
                self.loop_signal.stop();
                "OK".into()
            }
            _ => "ERR\tunknown request".into(),
        };
        self.broker.send(conn, &reply);
    }

    /// The task whose process is `pid` or an ancestor of it.
    fn task_for_pid(&self, mut pid: u32) -> Option<u64> {
        for _ in 0..16 {
            if let Some(t) = self.tasks.iter().find(|t| t.pid == Some(pid)) {
                return Some(t.id);
            }
            let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            pid = s[s.rfind(')')? + 2..]
                .split_whitespace()
                .nth(1)?
                .parse()
                .ok()?;
            if pid <= 1 {
                return None;
            }
        }
        None
    }
}

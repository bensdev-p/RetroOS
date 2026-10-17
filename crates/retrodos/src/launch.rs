//! Client side of Station's launch broker (crates/station/src/broker.rs).

use crate::apps::{self, DesktopApp};
use crate::shell::Shell;
use crate::term;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct Broker {
    out: UnixStream,
    input: BufReader<UnixStream>,
    /// A line read partly before a timeout.
    partial: String,
}

pub struct TaskInfo {
    pub id: u64,
    pub pid: u32,
    pub console: bool,
    pub focused: bool,
    pub name: String,
    pub title: String,
}

impl Broker {
    /// Connect to the Station this shell runs under, if any.
    pub fn connect() -> Option<Broker> {
        let path = std::env::var_os("RETROOS_STATION")?;
        let s = UnixStream::connect(path).ok()?;
        let input = BufReader::new(s.try_clone().ok()?);
        Some(Broker {
            out: s,
            input,
            partial: String::new(),
        })
    }

    /// The next line; an empty string when a read timeout expires first.
    fn line(&mut self) -> Option<String> {
        loop {
            match self.input.read_line(&mut self.partial) {
                Ok(0) => return None,
                Ok(_) if self.partial.ends_with('\n') => {
                    let l = std::mem::take(&mut self.partial);
                    return Some(l.trim_end_matches(['\r', '\n']).to_string());
                }
                Ok(_) => continue,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return Some(String::new())
                }
                Err(_) => return None,
            }
        }
    }

    pub fn request(&mut self, fields: &[&str]) -> Result<Vec<String>, String> {
        let _ = self.input.get_ref().set_read_timeout(None);
        self.out
            .write_all(format!("{}\n", fields.join("\t")).as_bytes())
            .map_err(|_| "The display server is not responding".to_string())?;
        let reply = self.line().ok_or("The display server is not responding")?;
        let f: Vec<String> = reply.split('\t').map(String::from).collect();
        match f[0].as_str() {
            "OK" => Ok(f[1..].to_vec()),
            _ => Err(f.get(1).cloned().unwrap_or_else(|| "Request failed".into())),
        }
    }

    /// Start a program through Station. Returns the launch id and pid.
    /// `name` is what the user typed (FIREFOX); TASKS and KILL show and match it.
    pub fn launch(
        &mut self,
        name: &str,
        argv: &[String],
        cwd: &str,
        wait: bool,
    ) -> Result<(u64, u32), String> {
        let mut fields = vec![
            if wait { "wait" } else { "nowait" }.to_string(),
            cwd.to_string(),
            format!("=RETROOS_LAUNCH_NAME={name}"),
        ];
        // The shell's environment (SET changes it), minus what Station sets itself.
        for (k, v) in std::env::vars() {
            if matches!(
                k.as_str(),
                "WAYLAND_DISPLAY"
                    | "RETROOS_STATION"
                    | "RETROOS_LAUNCH_NAME"
                    | "DISPLAY"
                    | "TERM"
                    | "COLUMNS"
                    | "LINES"
            ) || k.starts_with("RETROOS_CONSOLE")
                || k.contains(['\t', '\n', '='])
                || v.contains(['\t', '\n'])
            {
                continue;
            }
            fields.push(format!("={k}={v}"));
        }
        fields.extend(argv.iter().cloned());
        let refs: Vec<&str> = fields.iter().map(String::as_str).collect();
        let mut req = vec!["LAUNCH"];
        req.extend(refs);
        let r = self.request(&req)?;
        let lid = r.first().and_then(|s| s.parse().ok()).ok_or("Bad reply")?;
        let pid = r.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        Ok((lid, pid))
    }

    /// Wait for a launched program to exit. Ctrl+C (Ctrl+Break) asks Station
    /// to end it. Returns its exit code.
    pub fn wait_exit(&mut self, lid: u64) -> i32 {
        let _ = self
            .input
            .get_ref()
            .set_read_timeout(Some(Duration::from_millis(200)));
        let mut asked = false;
        loop {
            if term::interrupted() && !asked {
                asked = true;
                let _ = self.out.write_all(format!("KILL\t{lid}\n").as_bytes());
            }
            let Some(l) = self.line() else { return 1 };
            let f: Vec<&str> = l.split('\t').collect();
            if f[0] == "EXIT" && f.get(1).and_then(|s| s.parse::<u64>().ok()) == Some(lid) {
                return f.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
            }
        }
    }

    pub fn tasks(&mut self) -> Result<Vec<TaskInfo>, String> {
        let r = self.request(&["TASKS"])?;
        let n: usize = r.first().and_then(|s| s.parse().ok()).unwrap_or(0);
        let mut out = Vec::new();
        for _ in 0..n {
            let l = self.line().ok_or("The display server is not responding")?;
            let f: Vec<&str> = l.split('\t').collect();
            if f.len() >= 7 && f[0] == "TASK" {
                out.push(TaskInfo {
                    id: f[1].parse().unwrap_or(0),
                    pid: f[2].parse().unwrap_or(0),
                    console: f[3] == "Console",
                    focused: f[4] == "1",
                    name: f[5].to_string(),
                    title: f[6..].join(" "),
                });
            }
        }
        Ok(out)
    }
}

/// How to run a program: decided by what it is, or forced by RUN /TEXT, RUN /GUI.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Auto,
    Text,
    Gui,
}

/// What a command name resolved to.
pub enum Target {
    Program(PathBuf),
    App(DesktopApp),
}

impl Shell {
    /// A program on PATH, else an installed application (DESIGN.md §7.4 steps 3–4).
    pub fn find_target(&self, cmd: &str) -> Option<Target> {
        if let Some(p) = self.find_program(cmd) {
            return Some(Target::Program(p));
        }
        if cmd.contains(['\\', '/', ':']) {
            return None;
        }
        apps::find_app(cmd).map(Target::App)
    }

    /// Run `cmd rest` as an external program, through Station when it is a
    /// GUI program. `wait` = DOS semantics (return when it exits).
    pub fn run_target(
        &mut self,
        cmd: &str,
        rest: &str,
        mode: Mode,
        wait: bool,
        stdin: Option<File>,
        stdout: Option<File>,
    ) -> i32 {
        let Some(target) = self.find_target(cmd) else {
            println!("Bad command or file name");
            return 1;
        };
        let args = self.external_args(rest);
        let (argv, gui) = match &target {
            Target::Program(p) => {
                let gui = match mode {
                    Mode::Auto => stdin.is_none() && stdout.is_none() && apps::is_gui_program(p),
                    m => m == Mode::Gui,
                };
                (vec![p.to_string_lossy().into_owned()], gui)
            }
            Target::App(a) => (
                a.exec.clone(),
                mode == Mode::Gui || (mode == Mode::Auto && !a.terminal),
            ),
        };
        let mut argv = argv;
        argv.extend(args);
        let label = cmd
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(cmd)
            .to_ascii_uppercase();
        if gui {
            match Broker::connect() {
                Some(mut b) => return self.launch_gui(&mut b, &argv, wait, &label),
                None if mode == Mode::Gui || !wait => {
                    println!("{label} needs the RetroOS display, which is not available here.");
                    return 1;
                }
                None => {} // e.g. over SSH: try it in the terminal
            }
        }
        if !wait {
            println!("START needs the RetroOS display for text programs; use RUN.");
            return 1;
        }
        let mut c = self.command(Path::new(&argv[0]), &argv[1..]);
        if let Some(f) = stdin {
            c.stdin(f);
        }
        if let Some(f) = stdout {
            c.stdout(f);
        }
        self.wait(c)
    }

    fn launch_gui(&mut self, b: &mut Broker, argv: &[String], wait: bool, label: &str) -> i32 {
        let cwd = self.dos.cwd_path().to_string_lossy().into_owned();
        match b.launch(label, argv, &cwd, wait) {
            Ok((lid, _)) if wait => {
                println!("Running {label}...  Ctrl+Alt+Tab switches back, Ctrl+C ends it.");
                let _ = std::io::stdout().flush();
                term::clear_interrupt();
                let code = b.wait_exit(lid);
                term::clear_interrupt();
                code
            }
            Ok(_) => {
                println!("Started {label}.  Ctrl+Alt+Tab switches to it.");
                0
            }
            Err(e) => {
                println!("Cannot start {label}: {e}");
                1
            }
        }
    }
}

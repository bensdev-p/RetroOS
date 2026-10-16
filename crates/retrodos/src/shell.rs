//! The RetroDOS command interpreter.

use crate::builtins;
use crate::lineedit::{Editor, Outcome};
use crate::parse::{self, Args, Redir, Stage};
use crate::term;
use retro_dosfs::{split_drive, Dos};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const VERSION: &str = "1.0";

/// Where a built-in reads and writes.
pub struct Io<'a> {
    pub out: &'a mut dyn Write,
    pub input: Option<&'a mut dyn Read>,
    /// True when output goes to the terminal (enables paging prompts, colour).
    pub tty: bool,
}

impl Io<'_> {
    /// Read all piped/redirected input, or None when stdin is the keyboard.
    pub fn read_input(&mut self) -> Option<Vec<u8>> {
        let r = self.input.as_mut()?;
        let mut v = Vec::new();
        let _ = r.read_to_end(&mut v);
        Some(v)
    }
}

pub struct Shell {
    pub dos: Dos,
    pub errorlevel: i32,
    pub echo: bool,
    pub editor: Editor,
    pub exit: bool,
    pub user: String,
    /// Nesting depth (COMMAND inside COMMAND).
    pub depth: u32,
}

/// A DOS-style error message with an errorlevel.
pub fn err(io: &mut Io, msg: &str) -> i32 {
    let _ = writeln!(io.out, "{msg}");
    1
}

impl Shell {
    pub fn new() -> Shell {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
        let mut dos = Dos::new(&home);
        dos.refresh_removable(&user);
        if let Ok(cwd) = std::env::current_dir() {
            let _ = dos.chdir(&cwd.to_string_lossy());
        }
        if std::env::var_os("PROMPT").is_none() {
            std::env::set_var("PROMPT", "$P$G");
        }
        std::env::set_var("RETRODOS", VERSION);
        let depth = std::env::var("RETRODOS_DEPTH")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        std::env::set_var("RETRODOS_DEPTH", (depth + 1).to_string());
        Shell {
            dos,
            errorlevel: 0,
            echo: true,
            editor: Editor::default(),
            exit: false,
            user,
            depth,
        }
    }

    // ----------------------------------------------------------------- env

    /// Case-insensitive environment lookup (DOS variable names are case-insensitive).
    pub fn getenv(&self, name: &str) -> Option<String> {
        match name.to_ascii_uppercase().as_str() {
            "ERRORLEVEL" => return Some(self.errorlevel.to_string()),
            "CD" => return Some(self.dos.cwd_display()),
            "DATE" => return Some(crate::fmt::date_long(crate::fmt::now())),
            "TIME" => return Some(crate::fmt::time_long(crate::fmt::now())),
            _ => {}
        }
        if let Ok(v) = std::env::var(name) {
            return Some(v);
        }
        std::env::vars()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v)
    }

    /// Set (or with `None`, remove) a variable, reusing an existing name's case.
    pub fn setenv(&mut self, name: &str, value: Option<&str>) {
        let key = std::env::vars()
            .map(|(k, _)| k)
            .find(|k| k.eq_ignore_ascii_case(name))
            .unwrap_or_else(|| name.to_ascii_uppercase());
        match value {
            Some(v) => std::env::set_var(&key, v),
            None => std::env::remove_var(&key),
        }
    }

    /// Expand %VAR% (and %% → %).
    pub fn expand_vars(&self, line: &str) -> String {
        let mut out = String::new();
        let mut rest = line;
        while let Some(i) = rest.find('%') {
            out.push_str(&rest[..i]);
            let after = &rest[i + 1..];
            if let Some(stripped) = after.strip_prefix('%') {
                out.push('%');
                rest = stripped;
                continue;
            }
            match after.find('%') {
                Some(j) if j > 0 && !after[..j].contains(char::is_whitespace) => {
                    out.push_str(&self.getenv(&after[..j]).unwrap_or_default());
                    rest = &after[j + 1..];
                }
                _ => {
                    out.push('%');
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        out
    }

    // -------------------------------------------------------------- prompt

    pub fn prompt(&self) -> String {
        let spec = self.getenv("PROMPT").unwrap_or_else(|| "$P$G".into());
        let mut out = String::new();
        let mut it = spec.chars();
        while let Some(c) = it.next() {
            if c != '$' {
                out.push(c);
                continue;
            }
            match it.next().map(|c| c.to_ascii_uppercase()) {
                Some('P') => out.push_str(&self.dos.cwd_display()),
                Some('N') => out.push(self.dos.current_drive()),
                Some('G') => out.push('>'),
                Some('L') => out.push('<'),
                Some('B') => out.push('|'),
                Some('Q') => out.push('='),
                Some('$') => out.push('$'),
                Some('_') => out.push('\n'),
                Some('E') => out.push('\x1b'),
                Some('H') => out.push('\x08'),
                Some('D') => out.push_str(&crate::fmt::date_long(crate::fmt::now())),
                Some('T') => out.push_str(&crate::fmt::time_long(crate::fmt::now())),
                Some('V') => out.push_str(&format!("RetroDOS Version {VERSION}")),
                Some(o) => {
                    out.push('$');
                    out.push(o);
                }
                None => out.push('$'),
            }
        }
        out
    }

    // ------------------------------------------------------------- the loop

    pub fn interactive(&mut self) {
        while !self.exit {
            self.dos.refresh_removable(&self.user.clone());
            let prompt = if self.echo {
                self.prompt()
            } else {
                String::new()
            };
            let dos = self.dos.clone();
            let completer = move |tok: &str| complete(&dos, tok);
            let outcome = self.editor.read_line(&prompt, &completer);
            match outcome {
                Outcome::Line(l) => {
                    self.execute(&l);
                    if self.echo && !l.trim().is_empty() {
                        println!();
                    }
                }
                Outcome::Cancelled => {}
                Outcome::Eof => break,
            }
        }
    }

    /// Execute one command line (which may chain commands with &, && and ||).
    pub fn execute(&mut self, line: &str) {
        term::clear_interrupt();
        let expanded = self.expand_vars(line);
        for (cond, part) in parse::split_chain(&expanded) {
            let run = match cond {
                parse::Cond::Always => true,
                parse::Cond::IfOk => self.errorlevel == 0,
                parse::Cond::IfFail => self.errorlevel != 0,
            };
            if run && !self.exit && !term::interrupted() {
                self.execute_pipeline(&part);
            }
        }
    }

    fn execute_pipeline(&mut self, line: &str) {
        let stages = parse::parse_line(line);
        if stages.is_empty() {
            return;
        }
        let code = if stages.len() == 1 {
            self.run_single(&stages[0])
        } else {
            self.run_pipeline(&stages)
        };
        self.errorlevel = code;
        let _ = io::stdout().flush();
    }

    fn run_single(&mut self, st: &Stage) -> i32 {
        let mut input_file = None;
        let mut out_file = None;
        for r in &st.redirs {
            match r {
                Redir::In(p) => match self.open_in(p) {
                    Ok(f) => input_file = Some(f),
                    Err(m) => return err_stdout(&m),
                },
                Redir::Out(p) | Redir::Append(p) => {
                    match self.open_out(p, matches!(r, Redir::Append(_))) {
                        Ok(f) => out_file = Some(f),
                        Err(m) => return err_stdout(&m),
                    }
                }
            }
        }
        if st.cmd.is_empty() {
            return 0;
        }
        if let Some((b, args)) = self.find_builtin(st) {
            let tty = out_file.is_none() && term::is_tty(1);
            let stdout = io::stdout();
            let mut lock;
            let mut file;
            let out: &mut dyn Write = match out_file.as_mut() {
                Some(f) => {
                    file = f;
                    &mut file
                }
                None => {
                    lock = stdout.lock();
                    &mut lock
                }
            };
            let mut io = Io {
                out,
                input: input_file.as_mut().map(|f| f as &mut dyn Read),
                tty,
            };
            return self.call_builtin(b, &args, &mut io);
        }
        if let Some(d) = drive_only(&st.cmd) {
            return match self.dos.set_drive(d) {
                Ok(()) => 0,
                Err(e) => err_stdout(&e.to_string()),
            };
        }
        let Some(prog) = self.find_program(&st.cmd) else {
            return err_stdout("Bad command or file name");
        };
        let args = self.external_args(&st.rest);
        let mut cmd = self.command(&prog, &args);
        if let Some(f) = input_file {
            cmd.stdin(f);
        }
        if let Some(f) = out_file {
            cmd.stdout(f);
        }
        self.wait(cmd)
    }

    fn run_pipeline(&mut self, stages: &[Stage]) -> i32 {
        // All external: real concurrent pipes. Otherwise: DOS-style sequential
        // execution through buffers (COMMAND.COM used temp files).
        let all_external = stages
            .iter()
            .all(|s| self.find_builtin(s).is_none() && drive_only(&s.cmd).is_none());
        if all_external {
            return self.run_external_pipeline(stages);
        }
        let mut buf: Option<Vec<u8>> = None;
        let mut code = 0;
        for (i, st) in stages.iter().enumerate() {
            let last = i + 1 == stages.len();
            let mut input: Option<Box<dyn Read + Send>> = buf
                .take()
                .map(|b| Box::new(io::Cursor::new(b)) as Box<dyn Read + Send>);
            let mut out_target: Option<File> = None;
            for r in &st.redirs {
                match r {
                    Redir::In(p) => match self.open_in(p) {
                        Ok(f) => input = Some(Box::new(f)),
                        Err(m) => return err_stdout(&m),
                    },
                    Redir::Out(p) | Redir::Append(p) => {
                        match self.open_out(p, matches!(r, Redir::Append(_))) {
                            Ok(f) => out_target = Some(f),
                            Err(m) => return err_stdout(&m),
                        }
                    }
                }
            }
            let mut captured: Vec<u8> = Vec::new();
            if let Some((b, args)) = self.find_builtin(st) {
                let tty = last && out_target.is_none() && term::is_tty(1);
                let stdout = io::stdout();
                let mut lock;
                let mut file;
                let out: &mut dyn Write = if let Some(f) = out_target.as_mut() {
                    file = f;
                    &mut file
                } else if last {
                    lock = stdout.lock();
                    &mut lock
                } else {
                    &mut captured
                };
                let mut io = Io {
                    out,
                    input: input.as_mut().map(|r| r.as_mut() as &mut dyn Read),
                    tty,
                };
                code = self.call_builtin(b, &args, &mut io);
            } else {
                let Some(prog) = self.find_program(&st.cmd) else {
                    return err_stdout("Bad command or file name");
                };
                let args = self.external_args(&st.rest);
                let mut cmd = self.command(&prog, &args);
                cmd.stdin(if input.is_some() {
                    Stdio::piped()
                } else {
                    Stdio::inherit()
                });
                if let Some(f) = out_target.take() {
                    cmd.stdout(f);
                } else if !last {
                    cmd.stdout(Stdio::piped());
                }
                term::sane();
                let mut child = match cmd.spawn() {
                    Ok(c) => c,
                    Err(e) => return err_stdout(&os_error(&e)),
                };
                if let (Some(mut inp), Some(mut stdin)) = (input, child.stdin.take()) {
                    // Feed input from a thread so large outputs can't deadlock.
                    std::thread::spawn(move || {
                        let _ = io::copy(&mut inp, &mut stdin);
                    });
                }
                if let Some(mut so) = child.stdout.take() {
                    let _ = so.read_to_end(&mut captured);
                }
                code = exit_code(child.wait());
                term::sane();
            }
            if !last {
                buf = Some(captured);
            }
        }
        code
    }

    fn run_external_pipeline(&mut self, stages: &[Stage]) -> i32 {
        let mut children = Vec::new();
        let mut prev: Option<std::process::ChildStdout> = None;
        term::sane();
        for (i, st) in stages.iter().enumerate() {
            let last = i + 1 == stages.len();
            let Some(prog) = self.find_program(&st.cmd) else {
                return err_stdout("Bad command or file name");
            };
            let args = self.external_args(&st.rest);
            let mut cmd = self.command(&prog, &args);
            if let Some(p) = prev.take() {
                cmd.stdin(p);
            }
            for r in &st.redirs {
                match r {
                    Redir::In(p) => match self.open_in(p) {
                        Ok(f) => {
                            cmd.stdin(f);
                        }
                        Err(m) => return err_stdout(&m),
                    },
                    Redir::Out(p) | Redir::Append(p) => {
                        match self.open_out(p, matches!(r, Redir::Append(_))) {
                            Ok(f) => {
                                cmd.stdout(f);
                            }
                            Err(m) => return err_stdout(&m),
                        }
                    }
                }
            }
            if !last && !st.redirs.iter().any(|r| !matches!(r, Redir::In(_))) {
                cmd.stdout(Stdio::piped());
            }
            match cmd.spawn() {
                Ok(mut c) => {
                    prev = c.stdout.take();
                    children.push(c);
                }
                Err(e) => return err_stdout(&os_error(&e)),
            }
        }
        let mut code = 0;
        for mut c in children {
            code = exit_code(c.wait());
        }
        term::sane();
        code
    }

    // ------------------------------------------------------------ builtins

    fn find_builtin(&self, st: &Stage) -> Option<(&'static builtins::Builtin, Args)> {
        if let Some(b) = builtins::find(&st.cmd) {
            return Some((b, Args::new(&st.rest)));
        }
        // Glued forms: "CD..", "CD\", "DIR/W", "ECHO.", "TYPE=X".
        let cut = st
            .cmd
            .find(['.', '\\', '/', '+', ',', ';', '=', '[', ']'])?;
        if cut == 0 {
            return None;
        }
        let b = builtins::find(&st.cmd[..cut])?;
        let mut rest = st.cmd[cut..].to_string();
        if !st.rest.is_empty() {
            rest.push(' ');
            rest.push_str(&st.rest);
        }
        let mut args = Args::new(&rest);
        args.glued = true;
        Some((b, args))
    }

    fn call_builtin(&mut self, b: &builtins::Builtin, args: &Args, io: &mut Io) -> i32 {
        if args.help() && b.names[0] != "ECHO" {
            let _ = writeln!(io.out, "{}\n\n{}", b.summary, b.usage.trim_end());
            return 0;
        }
        let code = (b.run)(self, args, io);
        let _ = io.out.flush();
        if term::interrupted() {
            let _ = writeln!(io.out, "^C");
            term::clear_interrupt();
        }
        code
    }

    // ------------------------------------------------------------ programs

    /// Find an executable for a command word: explicit paths, then the
    /// current directory, then PATH. Matching is case-insensitive.
    pub fn find_program(&self, cmd: &str) -> Option<PathBuf> {
        let is_exec = |p: &Path| {
            p.is_file()
                && p.metadata()
                    .map(|m| m.permissions().mode() & 0o111 != 0)
                    .unwrap_or(false)
        };
        if cmd.contains(['\\', '/']) || split_drive(cmd).0.is_some() {
            let p = self.dos.resolve(cmd).ok()?;
            return is_exec(&p).then_some(p);
        }
        let mut dirs = vec![self.dos.cwd_path()];
        if let Some(path) = self.getenv("PATH") {
            dirs.extend(path.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
        }
        for d in dirs {
            let exact = d.join(cmd);
            if is_exec(&exact) {
                return Some(exact);
            }
            if let Ok(Some(real)) = retro_dosfs::find_entry(&d, cmd) {
                let p = d.join(real);
                if is_exec(&p) {
                    return Some(p);
                }
            }
            // "EDIT.EXE" / "NANO.COM": DOS habits for Linux programs.
            for ext in [".exe", ".com"] {
                if let Some(stem) = cmd.to_ascii_lowercase().strip_suffix(ext) {
                    if let Ok(Some(real)) = retro_dosfs::find_entry(&d, stem) {
                        let p = d.join(real);
                        if is_exec(&p) {
                            return Some(p);
                        }
                    }
                }
            }
        }
        None
    }

    /// Arguments for external programs: quotes removed; arguments that are
    /// clearly DOS paths (`C:\X`, or existing `DIR\FILE`) become POSIX paths.
    pub fn external_args(&self, rest: &str) -> Vec<String> {
        parse::words(rest)
            .into_iter()
            .map(|w| {
                let (drive, _) = split_drive(&w);
                let dosish = drive.is_some_and(|d| self.dos.drive(d).is_some())
                    || (w.contains('\\') && !w.starts_with('-'));
                if dosish {
                    if let Ok(p) = self.dos.resolve(&w) {
                        if drive.is_some() || p.exists() {
                            return p.to_string_lossy().into_owned();
                        }
                    }
                }
                w
            })
            .collect()
    }

    pub fn command(&self, prog: &Path, args: &[String]) -> Command {
        let mut cmd = Command::new(prog);
        let cwd = self.dos.cwd_path();
        cmd.args(args).current_dir(&cwd).env("PWD", &cwd);
        unsafe {
            cmd.pre_exec(|| {
                for s in [
                    libc::SIGINT,
                    libc::SIGQUIT,
                    libc::SIGTSTP,
                    libc::SIGTTOU,
                    libc::SIGTTIN,
                    libc::SIGPIPE,
                ] {
                    libc::signal(s, libc::SIG_DFL);
                }
                Ok(())
            });
        }
        cmd
    }

    /// Run a prepared command in the foreground and return its exit code.
    pub fn wait(&self, mut cmd: Command) -> i32 {
        term::sane();
        let r = cmd.status();
        term::sane();
        // Restore our default cursor in case the program changed it.
        if term::is_tty(1) {
            print!("\x1b[0 q");
        }
        match r {
            Ok(s) => {
                if s.signal() == Some(libc::SIGINT) {
                    println!("^C");
                }
                exit_code(Ok(s))
            }
            Err(e) => err_stdout(&os_error(&e)),
        }
    }

    // ------------------------------------------------------------ files

    fn open_in(&self, p: &str) -> Result<File, String> {
        let path = self.device_or_path(p)?;
        File::open(&path).map_err(|_| "File not found".to_string())
    }

    fn open_out(&self, p: &str, append: bool) -> Result<File, String> {
        let path = self.device_or_path(p)?;
        OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(&path)
            .map_err(|e| os_error(&e))
    }

    /// DOS device names: NUL, CON, PRN/LPT1 (→ /dev/null, there are no printers here yet).
    fn device_or_path(&self, p: &str) -> Result<PathBuf, String> {
        let up = p.trim_end_matches(':').to_ascii_uppercase();
        Ok(match up.as_str() {
            "NUL" | "PRN" | "LPT1" | "AUX" => PathBuf::from("/dev/null"),
            "CON" => PathBuf::from("/dev/tty"),
            _ => self.dos.resolve(p).map_err(|e| e.to_string())?,
        })
    }
}

fn drive_only(cmd: &str) -> Option<char> {
    let b = cmd.as_bytes();
    (b.len() == 2 && b[1] == b':' && b[0].is_ascii_alphabetic())
        .then(|| (b[0] as char).to_ascii_uppercase())
}

fn err_stdout(msg: &str) -> i32 {
    println!("{msg}");
    1
}

pub fn exit_code(r: io::Result<std::process::ExitStatus>) -> i32 {
    match r {
        Ok(s) => s.code().unwrap_or_else(|| 128 + s.signal().unwrap_or(0)),
        Err(_) => 1,
    }
}

/// Translate an OS error into DOS wording.
pub fn os_error(e: &io::Error) -> String {
    match e.kind() {
        io::ErrorKind::NotFound => "File not found".into(),
        io::ErrorKind::PermissionDenied => "Access denied".into(),
        io::ErrorKind::AlreadyExists => "File already exists".into(),
        io::ErrorKind::StorageFull => "Insufficient disk space".into(),
        io::ErrorKind::ReadOnlyFilesystem => "Write protect error".into(),
        io::ErrorKind::DirectoryNotEmpty => "Directory not empty".into(),
        _ => match e.raw_os_error() {
            Some(libc::ENOEXEC) => "Cannot execute this program".into(),
            Some(libc::EIO) => "General failure".into(),
            _ => format!("{e}"),
        },
    }
}

/// Tab completion: complete the path in `tok` (DOS or POSIX syntax).
pub fn complete(dos: &Dos, tok: &str) -> Vec<String> {
    let raw = tok.trim_start_matches('"');
    let split = raw.rfind(['\\', '/', ':']).map(|i| i + 1).unwrap_or(0);
    let (dir_part, prefix) = raw.split_at(split);
    let dir = if dir_part.is_empty() {
        dos.cwd_path()
    } else {
        match dos.resolve(dir_part) {
            Ok(p) => p,
            Err(_) => return vec![],
        }
    };
    let lower = prefix.to_lowercase();
    let mut out: Vec<String> = retro_dosfs::list_names(&dir)
        .into_iter()
        .filter(|n| {
            n.to_lowercase().starts_with(&lower) && (!n.starts_with('.') || prefix.starts_with('.'))
        })
        .map(|n| {
            let is_dir = dir.join(&n).is_dir();
            let sep = if dir_part.contains('/') && !dir_part.contains('\\') {
                '/'
            } else {
                '\\'
            };
            let mut s = format!("{dir_part}{n}");
            if is_dir {
                s.push(sep);
            }
            if s.contains(' ') {
                s = format!("\"{s}\"");
            }
            s
        })
        .collect();
    out.sort_by_key(|s| s.to_lowercase());
    out
}

impl Default for Shell {
    fn default() -> Self {
        Shell::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn var_expansion() {
        let sh = Shell::new();
        std::env::set_var("RETRO_T1", "abc");
        assert_eq!(sh.expand_vars("x %retro_t1% y"), "x abc y");
        assert_eq!(sh.expand_vars("100%% done"), "100% done");
        assert_eq!(sh.expand_vars("%NOPE_NOT_SET%!"), "!");
        assert_eq!(sh.expand_vars("50% off"), "50% off");
    }

    #[test]
    fn prompt_codes() {
        let mut sh = Shell::new();
        sh.setenv("PROMPT", Some("[$N]$G"));
        let p = sh.prompt();
        assert!(p.starts_with('[') && p.ends_with("]>"));
    }
}

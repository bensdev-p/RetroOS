//! Installing, removing and updating software (DESIGN.md §10.5, D-055).
//!
//! Reading is done as the user: what is installed (`dpkg-query`), the
//! updates waiting (`apt-get -s`), the whole package index
//! (`apt-cache search`). Changes go through the root helper
//! `/usr/libexec/retroos/software`, which runs apt-get with its progress on
//! a status pipe; an administrator's password is needed (checked with sudo,
//! which remembers it for a few minutes). A job runs on a thread and the
//! front-end polls [`Job::progress`] / [`Job::finished`] while it draws.
//!
//! `RETROOS_TEST_SOFTWARE=1` swaps in a pretend system for host tests.

use crate::admin::{self, AdminError};
use std::collections::HashSet;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

const HELPER: &str = "software";
/// What the daily check found (written by the helper).
pub const STATE: &str = "/var/lib/retroos/updates";
/// The automatic security updates switch.
pub const CONF: &str = "/etc/retroos/updates.conf";
/// Where the helper says which job is running (`job`: its pid, then what
/// it does) and how far it has got (`progress`: apt's status lines).
pub const RUN: &str = "/run/retroos-software";

fn fake() -> bool {
    std::env::var_os("RETROOS_TEST_SOFTWARE").is_some()
}

/// What a job does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    Install(Vec<String>),
    Remove(Vec<String>),
    /// Fetch the package lists, then install every update.
    UpgradeAll,
    /// Fetch the package lists and count the updates.
    Check,
}

impl Op {
    fn args(&self) -> Vec<String> {
        let mut v: Vec<String> = match self {
            Op::Install(_) => vec!["install".into()],
            Op::Remove(_) => vec!["remove".into()],
            Op::UpgradeAll => vec!["upgrade".into()],
            Op::Check => vec!["check".into()],
        };
        if let Op::Install(p) | Op::Remove(p) = self {
            v.extend(p.iter().cloned());
        }
        v
    }
}

/// A package name the helper will accept (Debian policy's characters).
pub fn valid_package(p: &str) -> bool {
    p.len() >= 2
        && p.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && p.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "+-.".contains(c))
}

/// How far a job has got.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub percent: u32,
    /// "Downloading 3 of 11", "Installing chromium", …
    pub text: String,
}

/// One line from apt's status pipe, as overall progress: downloading is
/// the first half, unpacking and setting up the second.
pub fn parse_status(line: &str) -> Option<Progress> {
    let mut f = line.splitn(4, ':');
    let kind = f.next()?;
    let what = f.next()?;
    let pct: f32 = f.next()?.parse().ok()?;
    let msg = f.next().unwrap_or("").trim();
    let (base, text) = match kind {
        "dlstatus" => (0.0, msg.replace("Retrieving file", "Downloading")),
        "pmstatus" => (50.0, friendly_pm(what, msg)),
        "pmerror" => {
            return Some(Progress {
                percent: 100,
                text: msg.into(),
            })
        }
        _ => return None,
    };
    Some(Progress {
        percent: (base + pct.clamp(0.0, 100.0) / 2.0).round() as u32,
        text,
    })
}

fn friendly_pm(pkg: &str, msg: &str) -> String {
    if pkg == "dpkg-exec" {
        return "Finishing".into();
    }
    let m = msg.to_ascii_lowercase();
    let verb = if m.contains("remov") {
        "Removing"
    } else if m.contains("configur") {
        "Setting up"
    } else {
        "Installing"
    };
    format!("{verb} {pkg}")
}

/// A job's progress and, once finished, its outcome.
type Shared = Arc<Mutex<(Progress, Option<Result<(), String>>)>>;

/// A running job.
pub struct Job {
    pub op: Op,
    shared: Shared,
}

impl Job {
    pub fn progress(&self) -> Progress {
        self.shared.lock().map(|g| g.0.clone()).unwrap_or_default()
    }

    /// The outcome, once it has finished.
    pub fn finished(&self) -> Option<Result<(), String>> {
        self.shared.lock().ok().and_then(|g| g.1.clone())
    }
}

/// A job the helper is running, started by any window or the daily check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Running {
    /// "Installing retroarch, libretro-nestopia"
    pub what: String,
    pub progress: Progress,
}

/// The helper's job file and progress, read: None when no job runs (no
/// file, or its process has gone).
pub fn parse_running(job: &str, progress: &str, alive: impl Fn(u32) -> bool) -> Option<Running> {
    let mut l = job.lines();
    let pid: u32 = l.next()?.trim().parse().ok()?;
    if !alive(pid) {
        return None;
    }
    let mut words = l.next().unwrap_or("").split_whitespace();
    let verb = match words.next().unwrap_or("") {
        "install" => "Installing",
        "remove" => "Removing",
        "upgrade" => "Installing updates",
        "security" => "Installing security updates",
        "check" => "Checking for updates",
        _ => "Changing software",
    };
    let pkgs: Vec<&str> = words.collect();
    let what = if pkgs.is_empty() {
        verb.to_string()
    } else {
        format!("{verb} {}", pkgs.join(", "))
    };
    let mut p = Progress {
        percent: 0,
        text: "Starting...".into(),
    };
    for line in progress.lines() {
        if let Some(q) = parse_status(line) {
            p = q;
        } else if let Some(t) = line.strip_prefix("step:") {
            p.text = t.trim().to_string();
        }
    }
    Some(Running { what, progress: p })
}

/// The job the helper is running now, if any (HW 4.52: an install started
/// from another window, still going).
///
/// `$RETROOS_SOFTWARE_RUN` names another folder (tests; `~/` is the home
/// folder); the pretend system has no other jobs without it.
pub fn running() -> Option<Running> {
    let dir: std::path::PathBuf = match std::env::var("RETROOS_SOFTWARE_RUN") {
        Ok(d) => match d.strip_prefix("~/") {
            Some(rest) => std::path::Path::new(&std::env::var("HOME").ok()?).join(rest),
            None => d.into(),
        },
        Err(_) if fake() => return None,
        Err(_) => RUN.into(),
    };
    let job = std::fs::read_to_string(dir.join("job")).ok()?;
    let progress = std::fs::read_to_string(dir.join("progress")).unwrap_or_default();
    parse_running(&job, &progress, |pid| {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    })
}

/// Does changing software need the administrator's password now? (False
/// while sudo still remembers it, or for root.)
pub fn needs_password() -> bool {
    if fake() {
        return !fakesys::authorized();
    }
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } == 0 {
        return false;
    }
    !Command::new("sudo")
        .args(["-n", "-v"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Check the administrator's password (see [`needs_password`]).
pub fn authorize(password: &str) -> Result<(), AdminError> {
    if fake() {
        return fakesys::authorize(password);
    }
    admin::authorize(password)
}

/// Start `op` (after [`authorize`] when [`needs_password`]).
pub fn start(op: Op) -> Result<Job, String> {
    if let Op::Install(p) | Op::Remove(p) = &op {
        if p.is_empty() || !p.iter().all(|p| valid_package(p)) {
            return Err("Not a package name.".into());
        }
    }
    let shared = Arc::new(Mutex::new((
        Progress {
            percent: 0,
            text: "Starting...".into(),
        },
        None,
    )));
    if fake() {
        fakesys::run(op.clone(), shared.clone());
        return Ok(Job { op, shared });
    }
    let helper = format!("{}/{HELPER}", admin::LIBEXEC);
    // SAFETY: geteuid has no preconditions.
    let mut cmd = if unsafe { libc::geteuid() } == 0 {
        Command::new(&helper)
    } else {
        let mut c = Command::new("sudo");
        c.args(["-n", "--", &helper]);
        c
    };
    let mut child = cmd
        .args(op.args())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Cannot run the software helper: {e}"))?;
    let out = child.stdout.take();
    let err = child.stderr.take();
    let sh = shared.clone();
    std::thread::spawn(move || {
        if let Some(out) = out {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if let Some(p) = parse_status(&line) {
                    if let Ok(mut g) = sh.lock() {
                        g.0 = p;
                    }
                } else if let Some(t) = line.strip_prefix("step:") {
                    // The helper's own steps ("Reading the package lists").
                    if let Ok(mut g) = sh.lock() {
                        g.0.text = t.trim().to_string();
                    }
                }
            }
        }
        let mut msg = String::new();
        if let Some(mut e) = err {
            use std::io::Read;
            let _ = e.read_to_string(&mut msg);
        }
        let ok = child.wait().is_ok_and(|s| s.success());
        let r = if ok {
            Ok(())
        } else {
            Err(friendly_error(&msg))
        };
        if let Ok(mut g) = sh.lock() {
            if ok {
                g.0 = Progress {
                    percent: 100,
                    text: "Done.".into(),
                };
            }
            g.1 = Some(r);
        }
    });
    Ok(Job { op, shared })
}

/// apt's and sudo's words, made plain.
pub fn friendly_error(stderr: &str) -> String {
    let e = stderr.trim();
    if e.contains("password is required") || e.contains("not allowed") || e.contains("sudoers") {
        AdminError::NotAdmin
            .to_string()
            .replace("change this setting", "install software")
    } else if e.contains("Unable to locate package") || e.contains("has no installation candidate")
    {
        "That program is not in the package lists (no internet when they were read?).".into()
    } else if e.contains("Could not resolve") || e.contains("Temporary failure resolving") {
        "Cannot reach the software archive. Check the network.".into()
    } else if e.contains("Could not get lock") || e.contains("is held by process") {
        "Another installation is running. Try again in a minute.".into()
    } else if e.contains("No space left") || e.contains("You don't have enough free space") {
        "There is not enough free space on the disk.".into()
    } else {
        e.lines()
            .rev()
            .find(|l| l.starts_with("E:"))
            .or_else(|| e.lines().last())
            .map(|l| l.trim_start_matches("E:").trim().to_string())
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| "The installation failed.".into())
    }
}

/// Which of `packages` are installed.
pub fn installed(packages: &[String]) -> HashSet<String> {
    if fake() {
        return fakesys::installed();
    }
    let out = Command::new("dpkg-query")
        .args(["-W", "-f=${Package}\t${db:Status-Abbrev}\n"])
        .args(packages)
        .stderr(Stdio::null())
        .output();
    let Ok(out) = out else {
        return HashSet::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let (p, st) = l.split_once('\t')?;
            st.starts_with("ii").then(|| p.to_string())
        })
        .collect()
}

/// Every catalog package's state, in one dpkg-query.
pub fn installed_catalog() -> HashSet<String> {
    let all: Vec<String> = crate::catalog::catalog()
        .apps
        .iter()
        .map(|a| a.packages[0].clone())
        .collect();
    installed(&all)
}

/// An update waiting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Update {
    pub package: String,
    pub from: Option<String>,
    pub to: String,
    pub security: bool,
}

/// `apt-get -s upgrade`'s "Inst" lines.
pub fn parse_simulation(text: &str) -> Vec<Update> {
    text.lines()
        .filter_map(|l| {
            let rest = l.strip_prefix("Inst ")?;
            let (package, rest) = rest.split_once(' ')?;
            let (from, rest) = match rest.strip_prefix('[') {
                Some(r) => {
                    let (v, r) = r.split_once(']')?;
                    (Some(v.to_string()), r.trim_start())
                }
                None => (None, rest),
            };
            let inner = rest.strip_prefix('(')?;
            let to = inner.split_whitespace().next()?.to_string();
            Some(Update {
                package: package.to_string(),
                from,
                to,
                security: inner.to_ascii_lowercase().contains("security"),
            })
        })
        .collect()
}

/// The updates waiting (from the package lists last read).
pub fn updates() -> Vec<Update> {
    if fake() {
        return fakesys::updates();
    }
    let out = Command::new("apt-get")
        .args([
            "-s",
            "-q",
            "-o",
            "Debug::NoLocking=1",
            "upgrade",
            "--with-new-pkgs",
        ])
        .env("LC_ALL", "C")
        .stderr(Stdio::null())
        .output();
    out.map(|o| parse_simulation(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

/// What the daily check found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub count: u32,
    pub security: u32,
    /// Seconds since 1970 when the lists were last read; 0 = never.
    pub checked: u64,
}

pub fn parse_state(text: &str) -> Status {
    let get = |k: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(k)?.strip_prefix('='))
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0)
    };
    Status {
        count: get("count") as u32,
        security: get("security") as u32,
        checked: get("checked"),
    }
}

pub fn status() -> Status {
    if fake() {
        let u = fakesys::updates();
        return Status {
            count: u.len() as u32,
            security: u.iter().filter(|u| u.security).count() as u32,
            checked: 1_700_000_000,
        };
    }
    let path = std::env::var("RETROOS_UPDATES_STATE").unwrap_or_else(|_| STATE.into());
    parse_state(&std::fs::read_to_string(path).unwrap_or_default())
}

/// The notice at the prompt and on the System Bar, if any.
pub fn notice(s: &Status) -> Option<String> {
    match s.count {
        0 => None,
        1 => Some("1 update available. Type UPDATE.".into()),
        n => Some(format!("{n} updates available. Type UPDATE.")),
    }
}

/// Are security updates installed by themselves? (On unless turned off.)
pub fn auto_security() -> bool {
    if fake() {
        return true;
    }
    !std::fs::read_to_string(CONF)
        .unwrap_or_default()
        .lines()
        .any(|l| l.trim() == "security=off")
}

/// Is Debian's non-free section among the sources?
pub fn nonfree() -> bool {
    if fake() {
        return false;
    }
    std::fs::read_to_string("/etc/apt/sources.list.d/debian.sources")
        .unwrap_or_default()
        .lines()
        .any(|l| l.starts_with("Components:") && l.split_whitespace().any(|c| c == "non-free"))
}

/// Turn automatic security updates / the non-free section on or off (the
/// set-updates helper; administrators without a password, like the other
/// settings).
pub fn set_setting(what: &str, on: bool) -> Result<(), String> {
    if fake() {
        return Ok(());
    }
    admin::run_helper("set-updates", &[what, if on { "on" } else { "off" }])
        .map_err(|e| e.to_string())
}

/// Packages in the whole index whose names contain `word` (the catalog's
/// "search everything"): (name, summary).
pub fn search_all(word: &str) -> Vec<(String, String)> {
    if fake() {
        return vec![("cowsay".into(), "configurable talking cow".into())];
    }
    let out = Command::new("apt-cache")
        .args(["search", "--names-only", word])
        .env("LC_ALL", "C")
        .stderr(Stdio::null())
        .output();
    let Ok(out) = out else { return Vec::new() };
    let mut v: Vec<(String, String)> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let (p, s) = l.split_once(" - ")?;
            Some((p.trim().to_string(), s.trim().to_string()))
        })
        .collect();
    v.sort();
    v.truncate(200);
    v
}

/// The pretend system: htop installed, three updates, password "retro".
/// What is installed is kept in a file so separate programs agree.
mod fakesys {
    use super::*;
    use std::time::Duration;

    fn file() -> std::path::PathBuf {
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        base.join("retro-software-test")
    }

    /// Like sudo, the password is remembered for five minutes, for every
    /// program (sudo: for the terminal).
    pub fn authorized() -> bool {
        std::fs::metadata(file().with_extension("auth"))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|d| d < Duration::from_secs(300))
    }

    pub fn authorize(p: &str) -> Result<(), AdminError> {
        if p == "retro" {
            let _ = std::fs::write(file().with_extension("auth"), "");
            Ok(())
        } else {
            Err(AdminError::WrongPassword)
        }
    }

    pub fn installed() -> HashSet<String> {
        match std::fs::read_to_string(file()) {
            Ok(s) => s.lines().map(String::from).collect(),
            Err(_) => ["htop".to_string()].into_iter().collect(),
        }
    }

    fn save(set: &HashSet<String>) {
        let mut v: Vec<&String> = set.iter().collect();
        v.sort();
        let text: String = v.iter().map(|p| format!("{p}\n")).collect();
        let _ = std::fs::write(file(), text);
    }

    fn updates_done() -> std::path::PathBuf {
        file().with_extension("updated")
    }

    pub fn updates() -> Vec<Update> {
        if updates_done().exists() {
            return Vec::new();
        }
        let u = |p: &str, to: &str, security| Update {
            package: p.into(),
            from: Some("1.0".into()),
            to: to.into(),
            security,
        };
        vec![
            u("firefox", "128.2", false),
            u("libssl3t64", "3.5.4-1~deb13u1", true),
            u("tzdata", "2026b-1", false),
        ]
    }

    pub fn run(op: Op, shared: Shared) {
        std::thread::spawn(move || {
            for i in 1..=10 {
                std::thread::sleep(Duration::from_millis(150));
                let text = match &op {
                    Op::Install(p) if i <= 5 => format!("Downloading {} of 5", i),
                    Op::Install(p) => format!("Installing {}", p[0]),
                    Op::Remove(p) => format!("Removing {}", p[0]),
                    Op::UpgradeAll => "Installing updates".into(),
                    Op::Check => "Reading the package lists".into(),
                };
                if let Ok(mut g) = shared.lock() {
                    g.0 = Progress {
                        percent: i * 10,
                        text,
                    };
                }
            }
            let mut set = installed();
            match &op {
                Op::Install(p) => set.extend(p.iter().cloned()),
                Op::Remove(p) => {
                    for x in p {
                        set.remove(x);
                    }
                }
                Op::UpgradeAll => {
                    let _ = std::fs::write(updates_done(), "");
                }
                Op::Check => {}
            }
            save(&set);
            if let Ok(mut g) = shared.lock() {
                g.1 = Some(Ok(()));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_running_elsewhere() {
        let job = "4242\ninstall retroarch libretro-nestopia\n";
        let prog = "step:Reading the package lists from the internet\n\
                    dlstatus:1:40:Retrieving file 2 of 5\nstep:Installing\n";
        let r = parse_running(job, prog, |p| p == 4242).unwrap();
        assert_eq!(r.what, "Installing retroarch, libretro-nestopia");
        assert_eq!(
            (r.progress.percent, r.progress.text.as_str()),
            (20, "Installing")
        );
        assert_eq!(
            parse_running(job, prog, |_| false),
            None,
            "its process is gone"
        );
        assert_eq!(parse_running("", "", |_| true), None);
        let r = parse_running("7\ncheck\n", "", |_| true).unwrap();
        assert_eq!(
            (r.what.as_str(), r.progress.percent),
            ("Checking for updates", 0)
        );
    }

    #[test]
    fn apt_status_lines() {
        let p = parse_status("dlstatus:3:40.0:Retrieving file 3 of 11").unwrap();
        assert_eq!((p.percent, p.text.as_str()), (20, "Downloading 3 of 11"));
        let p = parse_status("pmstatus:chromium:50:Preparing to configure chromium").unwrap();
        assert_eq!((p.percent, p.text.as_str()), (75, "Setting up chromium"));
        let p = parse_status("pmstatus:dpkg-exec:80:Running dpkg").unwrap();
        assert_eq!(p.text, "Finishing");
        let p = parse_status("pmstatus:cowsay:88.8889:Installed cowsay (amd64)").unwrap();
        assert_eq!((p.percent, p.text.as_str()), (94, "Installing cowsay"));
        let p = parse_status("pmstatus:vim:30:Removing vim").unwrap();
        assert_eq!(p.text, "Removing vim");
        assert!(parse_status("Reading package lists...").is_none());
    }

    #[test]
    fn simulated_upgrades() {
        let s = "NOTE: This is only a simulation!\nInst libssl3t64 [3.5.1-1] (3.5.4-1~deb13u1 Debian-Security:13/stable-security [arm64])\nInst tzdata [2025b-4] (2026a-0+deb13u1 Debian:13.2/stable [all])\nInst newdep (1.0 Debian:13.2/stable [arm64])\nConf tzdata (2026a-0+deb13u1 Debian:13.2/stable [all])\n";
        let u = parse_simulation(s);
        assert_eq!(u.len(), 3);
        assert!(u[0].security && !u[1].security);
        assert_eq!(u[0].from.as_deref(), Some("3.5.1-1"));
        assert_eq!(u[1].to, "2026a-0+deb13u1");
        assert_eq!(u[2].from, None);
    }

    #[test]
    fn state_and_notice() {
        let s = parse_state("count=3\nsecurity=1\nchecked=1762700000\n");
        assert_eq!((s.count, s.security, s.checked), (3, 1, 1_762_700_000));
        assert_eq!(notice(&s).unwrap(), "3 updates available. Type UPDATE.");
        assert_eq!(notice(&parse_state("")), None);
    }

    #[test]
    fn package_names_and_errors() {
        assert!(
            valid_package("libreoffice-writer") && valid_package("g++") && valid_package("0ad")
        );
        assert!(!valid_package("Foo") && !valid_package("-rf") && !valid_package("a;b"));
        assert_eq!(
            friendly_error("E: Unable to locate package nothing"),
            "That program is not in the package lists (no internet when they were read?)."
        );
        assert_eq!(
            friendly_error("W: x\nE: Sub-process /usr/bin/dpkg returned an error code (1)"),
            "Sub-process /usr/bin/dpkg returned an error code (1)"
        );
    }
}

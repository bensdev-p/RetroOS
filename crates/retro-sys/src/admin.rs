//! Running RetroOS's small root helpers (`/usr/libexec/retroos/*`).
//!
//! Settings that live in /etc are written by helpers that check their
//! arguments and change one thing. Members of the admin group may run them
//! through sudo without a password (sudoers.d/retroos, D-043); everyone else
//! is told that an administrator has to make the change.

use std::process::Command;

pub const LIBEXEC: &str = "/usr/libexec/retroos";

/// Why a helper did not do its job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminError {
    /// Not allowed without a password: the user is not an administrator.
    NotAdmin,
    /// The helper ran and refused or failed; its message.
    Failed(String),
}

impl std::fmt::Display for AdminError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdminError::NotAdmin => write!(f, "Only an administrator can change this setting."),
            AdminError::Failed(m) => write!(f, "{m}"),
        }
    }
}

fn is_root() -> bool {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() == 0 }
}

/// Run helper `name` (a file in [`LIBEXEC`]) with `args` as root.
pub fn run_helper(name: &str, args: &[&str]) -> Result<(), AdminError> {
    run_helper_input(name, args, "")
}

/// [`run_helper`] with `input` on the helper's standard input (secrets
/// never go on the command line, where `ps` shows them).
pub fn run_helper_input(name: &str, args: &[&str], input: &str) -> Result<(), AdminError> {
    use std::io::Write;
    use std::process::Stdio;
    let helper = format!("{LIBEXEC}/{name}");
    let mut cmd = if is_root() {
        Command::new(&helper)
    } else {
        let mut c = Command::new("sudo");
        c.args(["-n", "--", &helper]);
        c
    };
    let mut child = cmd
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AdminError::Failed(format!("Cannot run {name}: {e}")))?;
    if let Some(mut i) = child.stdin.take() {
        let _ = i.write_all(input.as_bytes());
    }
    let out = child
        .wait_with_output()
        .map_err(|e| AdminError::Failed(format!("Cannot run {name}: {e}")))?;
    if out.status.success() {
        return Ok(());
    }
    let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if msg.contains("password is required")
        || msg.contains("not allowed")
        || msg.contains("sudoers")
    {
        Err(AdminError::NotAdmin)
    } else if msg.is_empty() {
        Err(AdminError::Failed(format!("{name} failed.")))
    } else {
        Err(AdminError::Failed(
            msg.lines().last().unwrap_or("").to_string(),
        ))
    }
}

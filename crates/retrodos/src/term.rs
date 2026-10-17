//! Terminal plumbing: raw mode, window size, Ctrl+C.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};

static INTERRUPTED: AtomicBool = AtomicBool::new(false);
/// Like INTERRUPTED, but only cleared by the batch interpreter.
static BREAK: AtomicBool = AtomicBool::new(false);

extern "C" fn on_sigint(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::SeqCst);
    BREAK.store(true, Ordering::SeqCst);
}

/// Install the SIGINT handler used while built-in commands run.
pub fn install_signal_handlers() {
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = on_sigint as *const () as usize;
        libc::sigemptyset(&mut sa.sa_mask);
        sa.sa_flags = 0; // no SA_RESTART: blocking reads return EINTR so built-ins can stop
        libc::sigaction(libc::SIGINT, &sa, std::ptr::null_mut());
        libc::signal(libc::SIGQUIT, libc::SIG_IGN);
        libc::signal(libc::SIGTSTP, libc::SIG_IGN);
        libc::signal(libc::SIGTTOU, libc::SIG_IGN);
        libc::signal(libc::SIGTTIN, libc::SIG_IGN);
    }
}

/// Has Ctrl+C been pressed since the last [`clear_interrupt`]?
pub fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::SeqCst)
}

pub fn clear_interrupt() {
    INTERRUPTED.store(false, Ordering::SeqCst);
}

/// Was Ctrl+C pressed since the last call? (For "Terminate batch job".)
pub fn take_break() -> bool {
    BREAK.swap(false, Ordering::SeqCst)
}

pub fn is_tty(fd: i32) -> bool {
    unsafe { libc::isatty(fd) == 1 }
}

/// (columns, rows) of the controlling terminal; 80x25 if unknown.
pub fn size() -> (u16, u16) {
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(1, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_col > 0 {
            return (ws.ws_col, ws.ws_row);
        }
    }
    (80, 25)
}

/// RAII raw mode on stdin. Restores the previous settings on drop.
pub struct RawMode {
    orig: libc::termios,
}

impl RawMode {
    pub fn enable() -> io::Result<RawMode> {
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(0, &mut t) != 0 {
                return Err(io::Error::last_os_error());
            }
            let orig = t;
            t.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG | libc::IEXTEN);
            t.c_iflag &= !(libc::IXON | libc::ICRNL | libc::INLCR);
            t.c_cc[libc::VMIN] = 1;
            t.c_cc[libc::VTIME] = 0;
            libc::tcsetattr(0, libc::TCSADRAIN, &t);
            Ok(RawMode { orig })
        }
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        unsafe {
            libc::tcsetattr(0, libc::TCSADRAIN, &self.orig);
        }
    }
}

/// While a graphical program runs, keys typed at the console are not echoed
/// (no stray `^[[A`) and are thrown away afterwards. Ctrl+C still works.
pub struct Quiet {
    orig: Option<libc::termios>,
}

impl Quiet {
    pub fn enable() -> Quiet {
        unsafe {
            let mut t: libc::termios = std::mem::zeroed();
            if !is_tty(0) || libc::tcgetattr(0, &mut t) != 0 {
                return Quiet { orig: None };
            }
            let orig = t;
            t.c_lflag &= !(libc::ECHO | libc::ECHOCTL);
            libc::tcsetattr(0, libc::TCSANOW, &t);
            Quiet { orig: Some(orig) }
        }
    }
}

impl Drop for Quiet {
    fn drop(&mut self) {
        if let Some(o) = self.orig {
            unsafe {
                libc::tcflush(0, libc::TCIFLUSH);
                libc::tcsetattr(0, libc::TCSANOW, &o);
            }
        }
    }
}

/// Make sure the terminal is in sane cooked mode (after a child messed it up).
pub fn sane() {
    unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(0, &mut t) == 0 {
            t.c_lflag |= libc::ICANON | libc::ECHO | libc::ISIG | libc::IEXTEN | libc::ECHOE;
            t.c_iflag |= libc::ICRNL;
            t.c_oflag |= libc::OPOST | libc::ONLCR;
            libc::tcsetattr(0, libc::TCSADRAIN, &t);
        }
    }
}

/// Read one byte from stdin (raw mode). `None` on EOF or interruption.
/// Unbuffered on purpose: std's Stdin buffer would swallow the rest of an
/// escape sequence and defeat [`poll_input`].
pub fn read_byte() -> Option<u8> {
    let mut b = [0u8; 1];
    loop {
        let n = unsafe { libc::read(0, b.as_mut_ptr().cast(), 1) };
        match n {
            1 => return Some(b[0]),
            0 => return None,
            _ if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted => {
                if interrupted() {
                    return Some(3);
                }
            }
            _ => return None,
        }
    }
}

/// Wait up to `ms` for input; true if a byte is available.
pub fn poll_input(ms: i32) -> bool {
    let mut fds = libc::pollfd {
        fd: 0,
        events: libc::POLLIN,
        revents: 0,
    };
    unsafe { libc::poll(&mut fds, 1, ms) > 0 }
}

/// Ask a Yes/No style question and return the upper-cased key pressed.
/// Accepts only the characters in `choices`.
pub fn ask(prompt: &str, choices: &str) -> char {
    let mut out = io::stdout();
    let _ = write!(out, "{prompt}");
    let _ = out.flush();
    if !is_tty(0) {
        // Non-interactive: read a line.
        let mut s = String::new();
        let _ = io::stdin().read_line(&mut s);
        let c = s.trim().chars().next().unwrap_or('N').to_ascii_uppercase();
        let _ = writeln!(out, "{c}");
        return if choices.contains(c) {
            c
        } else {
            choices.chars().last().unwrap_or('N')
        };
    }
    let _raw = RawMode::enable();
    loop {
        let Some(b) = read_byte() else { return 'N' };
        if b == 3 {
            let _ = writeln!(out, "^C");
            return 'N';
        }
        let c = (b as char).to_ascii_uppercase();
        if choices.contains(c) {
            let _ = write!(out, "{c}\r\n");
            let _ = out.flush();
            return c;
        }
        let _ = write!(out, "\x07");
        let _ = out.flush();
    }
}

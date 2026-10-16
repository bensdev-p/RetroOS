//! Pseudo-terminal: spawn the shell and talk to it.

use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

pub struct Pty {
    pub master: File,
    pub pid: libc::pid_t,
}

fn cstr(s: &str) -> CString {
    CString::new(s).unwrap_or_else(|_| CString::new("?").unwrap())
}

impl Pty {
    /// Fork `argv` on a new pty of `cols`×`rows`, with `env` added.
    pub fn spawn(argv: &[String], cols: u16, rows: u16, env: &[(&str, String)]) -> io::Result<Pty> {
        let mut master: libc::c_int = -1;
        let mut slave: libc::c_int = -1;
        let ws = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: plain libc calls; fds are checked below.
        if unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null(),
                &ws,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        let master = unsafe { OwnedFd::from_raw_fd(master) };
        let slave = unsafe { OwnedFd::from_raw_fd(slave) };
        // Everything the child needs is prepared before fork (no allocation after it).
        let args: Vec<CString> = argv.iter().map(|a| cstr(a)).collect();
        let mut argp: Vec<*const libc::c_char> = args.iter().map(|a| a.as_ptr()).collect();
        argp.push(std::ptr::null());
        let mut envs: Vec<CString> = std::env::vars()
            .filter(|(k, _)| !env.iter().any(|(e, _)| e == k))
            .map(|(k, v)| cstr(&format!("{k}={v}")))
            .collect();
        envs.extend(env.iter().map(|(k, v)| cstr(&format!("{k}={v}"))));
        let mut envp: Vec<*const libc::c_char> = envs.iter().map(|a| a.as_ptr()).collect();
        envp.push(std::ptr::null());
        let fail = cstr("retro-console: cannot start the shell\r\n");

        let pid = unsafe { libc::fork() };
        if pid < 0 {
            return Err(io::Error::last_os_error());
        }
        if pid == 0 {
            // Child.
            unsafe {
                libc::setsid();
                libc::ioctl(slave.as_raw_fd(), libc::TIOCSCTTY, 0);
                for fd in 0..3 {
                    libc::dup2(slave.as_raw_fd(), fd);
                }
                for s in [
                    libc::SIGINT,
                    libc::SIGQUIT,
                    libc::SIGTSTP,
                    libc::SIGTTIN,
                    libc::SIGTTOU,
                    libc::SIGCHLD,
                    libc::SIGPIPE,
                ] {
                    libc::signal(s, libc::SIG_DFL);
                }
                let mut set: libc::sigset_t = std::mem::zeroed();
                libc::sigemptyset(&mut set);
                libc::sigprocmask(libc::SIG_SETMASK, &set, std::ptr::null_mut());
                libc::execvpe(argp[0], argp.as_ptr(), envp.as_ptr());
                libc::write(2, fail.as_ptr().cast(), fail.as_bytes().len());
                libc::_exit(127);
            }
        }
        drop(slave);
        let master = File::from(master);
        unsafe {
            let flags = libc::fcntl(master.as_raw_fd(), libc::F_GETFL);
            libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK);
            libc::fcntl(master.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC);
        }
        Ok(Pty { master, pid })
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        let ws = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        unsafe {
            libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ, &ws);
        }
    }

    /// Non-blocking check: has the child exited? Returns its status.
    pub fn try_wait(&self) -> Option<i32> {
        let mut status = 0;
        let r = unsafe { libc::waitpid(self.pid, &mut status, libc::WNOHANG) };
        (r == self.pid).then(|| {
            if libc::WIFEXITED(status) {
                libc::WEXITSTATUS(status)
            } else {
                128 + libc::WTERMSIG(status)
            }
        })
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        unsafe {
            libc::kill(self.pid, libc::SIGHUP);
            let mut st = 0;
            libc::waitpid(self.pid, &mut st, libc::WNOHANG);
        }
    }
}

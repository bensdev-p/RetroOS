//! Small wrappers over system information (statvfs, uname, /proc).

use std::ffi::CString;
use std::path::Path;

pub struct FsInfo {
    pub total: u64,
    pub free: u64,
    pub serial: String,
}

pub fn statvfs(p: &Path) -> Option<FsInfo> {
    let c = CString::new(p.as_os_str().as_encoded_bytes()).ok()?;
    unsafe {
        let mut s: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c.as_ptr(), &mut s) != 0 {
            return None;
        }
        let fsid = s.f_fsid as u64;
        let mix = (fsid ^ (fsid >> 32)) as u32;
        Some(FsInfo {
            total: s.f_blocks as u64 * s.f_frsize as u64,
            free: s.f_bavail as u64 * s.f_frsize as u64,
            serial: format!("{:04X}-{:04X}", mix >> 16, mix & 0xFFFF),
        })
    }
}

pub struct Uname {
    pub release: String,
    pub machine: String,
    pub nodename: String,
}

pub fn uname() -> Uname {
    unsafe {
        let mut u: libc::utsname = std::mem::zeroed();
        libc::uname(&mut u);
        let f = |a: &[libc::c_char]| {
            std::ffi::CStr::from_ptr(a.as_ptr())
                .to_string_lossy()
                .into_owned()
        };
        Uname {
            release: f(&u.release),
            machine: f(&u.machine),
            nodename: f(&u.nodename),
        }
    }
}

/// /proc/meminfo values in bytes.
pub fn meminfo() -> std::collections::HashMap<String, u64> {
    let mut m = std::collections::HashMap::new();
    if let Ok(s) = std::fs::read_to_string("/proc/meminfo") {
        for line in s.lines() {
            let mut it = line.split_whitespace();
            if let (Some(k), Some(v)) = (it.next(), it.next()) {
                if let Ok(v) = v.parse::<u64>() {
                    m.insert(k.trim_end_matches(':').to_string(), v * 1024);
                }
            }
        }
    }
    m
}

pub struct Proc {
    pub pid: u32,
    pub name: String,
    pub rss: u64,
}

/// Processes with their resident set size, largest first.
pub fn processes() -> Vec<Proc> {
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(4096) as u64;
    let mut v = Vec::new();
    if let Ok(rd) = std::fs::read_dir("/proc") {
        for e in rd.flatten() {
            let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            let base = e.path();
            let name = std::fs::read_to_string(base.join("comm"))
                .unwrap_or_default()
                .trim()
                .to_string();
            let rss = std::fs::read_to_string(base.join("statm"))
                .ok()
                .and_then(|s| {
                    s.split_whitespace()
                        .nth(1)
                        .and_then(|x| x.parse::<u64>().ok())
                })
                .unwrap_or(0)
                * page;
            if rss > 0 {
                v.push(Proc { pid, name, rss });
            }
        }
    }
    v.sort_by_key(|p| std::cmp::Reverse(p.rss));
    v
}

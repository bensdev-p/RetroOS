//! Shared pieces of the RetroOS login screen and first-boot Setup.

pub mod greetd;

use retro_tui::{Screen, BLACK, COBALT, TEAL};

/// Top and bottom bars in the Setup/BIOS style.
pub fn chrome(s: &mut Screen, title: &str, right: &str, help: &str) {
    s.clear(retro_tui::SILVER, COBALT);
    s.fill(0, 0, s.cols, 1, ' ', BLACK, TEAL);
    s.text(1, 0, title, BLACK, TEAL);
    let rw = right.chars().count() as u16;
    s.text(s.cols.saturating_sub(rw + 1), 0, right, BLACK, TEAL);
    let y = s.rows - 1;
    s.fill(0, y, s.cols, 1, ' ', BLACK, TEAL);
    s.text(1, y, help, BLACK, TEAL);
}

/// "10:42 AM" in local time.
pub fn clock() -> String {
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        let (h, ap) = match tm.tm_hour {
            0 => (12, "AM"),
            h @ 1..=11 => (h, "AM"),
            12 => (12, "PM"),
            h => (h - 12, "PM"),
        };
        format!("{h}:{:02} {ap}", tm.tm_min)
    }
}

/// Regular (uid ≥ 1000) user accounts, for the login screen hint.
pub fn local_users() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_to_string("/etc/passwd")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(':').collect();
            let uid: u32 = f.get(2)?.parse().ok()?;
            let shell = f.get(6).copied().unwrap_or("");
            ((1000..60000).contains(&uid)
                && !shell.ends_with("nologin")
                && !shell.ends_with("false"))
            .then(|| f[0].to_string())
        })
        .collect();
    v.sort();
    v
}

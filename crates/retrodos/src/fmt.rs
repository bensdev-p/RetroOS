//! DOS-flavoured number, date and time formatting.

use std::time::{SystemTime, UNIX_EPOCH};

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn mtime(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    }
}

/// 1234567 → "1,234,567".
pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub struct Tm {
    pub year: i32,
    pub mon: u32,
    pub mday: u32,
    pub hour: u32,
    pub min: u32,
    pub sec: u32,
    pub wday: u32,
}

pub fn local(t: i64) -> Tm {
    unsafe {
        let tt: libc::time_t = t as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&tt, &mut tm);
        Tm {
            year: tm.tm_year + 1900,
            mon: (tm.tm_mon + 1) as u32,
            mday: tm.tm_mday as u32,
            hour: tm.tm_hour as u32,
            min: tm.tm_min as u32,
            sec: tm.tm_sec as u32,
            wday: tm.tm_wday as u32,
        }
    }
}

const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// DIR listing date: "10-16-26".
pub fn date_short(t: i64) -> String {
    let tm = local(t);
    format!(
        "{:02}-{:02}-{:02}",
        tm.mon,
        tm.mday,
        tm.year.rem_euclid(100)
    )
}

/// DIR listing time: "10:42a" / " 3:07p".
pub fn time_short(t: i64) -> String {
    let tm = local(t);
    let (h, ap) = match tm.hour {
        0 => (12, 'a'),
        h @ 1..=11 => (h, 'a'),
        12 => (12, 'p'),
        h => (h - 12, 'p'),
    };
    format!("{h:>2}:{:02}{ap}", tm.min)
}

/// DATE command / %DATE%: "Fri 10-16-2026".
pub fn date_long(t: i64) -> String {
    let tm = local(t);
    format!(
        "{} {:02}-{:02}-{}",
        DAYS[tm.wday as usize % 7],
        tm.mon,
        tm.mday,
        tm.year
    )
}

/// TIME command / %TIME%: "10:42:07.00".
pub fn time_long(t: i64) -> String {
    let tm = local(t);
    format!("{:>2}:{:02}:{:02}.00", tm.hour, tm.min, tm.sec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(1234567), "1,234,567");
    }
}

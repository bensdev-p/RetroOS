//! The System Bar clock: local time, "10:42 AM".

use std::time::Duration;

fn local() -> libc::tm {
    // SAFETY: time() and localtime_r() only write to the values we pass.
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm
    }
}

/// "10:42 AM" (12-hour, as the Mac's and Windows 3.1's clocks).
pub fn format(hour: i32, min: i32) -> String {
    let h12 = match hour % 12 {
        0 => 12,
        h => h,
    };
    format!("{h12}:{min:02} {}", if hour < 12 { "AM" } else { "PM" })
}

pub fn now_text() -> String {
    let tm = local();
    format(tm.tm_hour, tm.tm_min)
}

/// Time until the minute changes.
pub fn until_next_minute() -> Duration {
    Duration::from_secs((60 - local().tm_sec.clamp(0, 59)) as u64)
}

#[cfg(test)]
mod tests {
    #[test]
    fn twelve_hour() {
        assert_eq!(super::format(0, 5), "12:05 AM");
        assert_eq!(super::format(10, 42), "10:42 AM");
        assert_eq!(super::format(12, 0), "12:00 PM");
        assert_eq!(super::format(23, 59), "11:59 PM");
    }
}

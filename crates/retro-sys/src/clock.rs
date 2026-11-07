//! Date, time and time zone through systemd-timedated (D-Bus), which the
//! admin group may use without a password (polkit rule, D-043).

use zbus::blocking::{Connection, Proxy};

const DEST: &str = "org.freedesktop.timedate1";
const PATH: &str = "/org/freedesktop/timedate1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clock {
    pub timezone: String,
    /// Set automatically from the network (timesyncd).
    pub ntp: bool,
    pub can_ntp: bool,
    pub synced: bool,
}

/// A local date and time, as people type it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Local {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

fn err(e: zbus::Error) -> String {
    let s = e.to_string();
    if s.contains("system_bus_socket") || s.contains("ServiceUnknown") {
        "The clock service is not running.".into()
    } else if s.contains("AccessDenied") || s.contains("NotAuthorized") || s.contains("interactive")
    {
        "Only an administrator can change the clock.".into()
    } else if s.contains("InvalidArgs") && s.contains("ime zone") {
        "Unknown time zone.".into()
    } else if s.contains("Automatic time synchronization is enabled") {
        "Turn off automatic time first.".into()
    } else {
        s
    }
}

fn proxy(conn: &Connection) -> Result<Proxy<'_>, String> {
    Proxy::new(conn, DEST, PATH, DEST).map_err(err)
}

fn conn() -> Result<Connection, String> {
    Connection::system().map_err(err)
}

/// The time zone from /etc/localtime when timedated cannot be asked.
fn timezone_from_files() -> String {
    std::fs::read_link("/etc/localtime")
        .ok()
        .and_then(|p| {
            let s = p.to_string_lossy().into_owned();
            s.split_once("zoneinfo/").map(|(_, z)| z.to_string())
        })
        .or_else(|| {
            std::fs::read_to_string("/etc/timezone")
                .ok()
                .map(|s| s.trim().to_string())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "UTC".into())
}

/// The clock's settings; without timedated, what the files say (and
/// automatic time unknown, shown as off and not changeable).
pub fn status() -> Result<Clock, String> {
    let c = conn()?;
    let p = proxy(&c)?;
    let get_b = |n: &str| p.get_property::<bool>(n).map_err(err);
    Ok(Clock {
        timezone: p.get_property::<String>("Timezone").map_err(err)?,
        ntp: get_b("NTP")?,
        can_ntp: get_b("CanNTP")?,
        synced: get_b("NTPSynchronized")?,
    })
}

/// [`status`], or the files' answer and the reason the service is missing.
pub fn status_or_files() -> (Clock, Option<String>) {
    match status() {
        Ok(c) => (c, None),
        Err(e) => (
            Clock {
                timezone: timezone_from_files(),
                ntp: false,
                can_ntp: false,
                synced: false,
            },
            Some(e),
        ),
    }
}

/// Every time zone name ("Europe/London"), sorted; UTC included.
pub fn timezones() -> Vec<String> {
    let listed = conn().and_then(|c| {
        let p = proxy(&c)?;
        p.call::<_, _, Vec<String>>("ListTimezones", &())
            .map_err(err)
    });
    let mut v = listed.unwrap_or_else(|_| zone_tab());
    if !v.iter().any(|z| z == "UTC") {
        v.push("UTC".into());
    }
    v.sort();
    v.dedup();
    v
}

/// Zone names from tzdata's zone1970.tab (third column).
fn zone_tab() -> Vec<String> {
    std::fs::read_to_string("/usr/share/zoneinfo/zone1970.tab")
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split('\t').nth(2).map(String::from))
        .collect()
}

fn call<B>(method: &str, body: &B) -> Result<(), String>
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    let c = conn()?;
    let p = proxy(&c)?;
    p.call::<_, _, ()>(method, body).map_err(err)
}

pub fn set_timezone(tz: &str) -> Result<(), String> {
    call("SetTimezone", &(tz, false))
}

pub fn set_ntp(on: bool) -> Result<(), String> {
    call("SetNTP", &(on, false))
}

/// Set the clock to a local date and time (automatic time must be off).
pub fn set_local(t: Local) -> Result<(), String> {
    let secs = to_epoch(t).ok_or("That date or time is not valid.")?;
    call("SetTime", &(secs * 1_000_000, false, false))
}

/// The local time now.
pub fn now() -> Local {
    // SAFETY: time(NULL) and localtime_r with valid pointers.
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        Local {
            year: tm.tm_year + 1900,
            month: (tm.tm_mon + 1) as u32,
            day: tm.tm_mday as u32,
            hour: tm.tm_hour as u32,
            minute: tm.tm_min as u32,
            second: tm.tm_sec as u32,
        }
    }
}

fn days_in(year: i32, month: u32) -> u32 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Seconds since 1970 for a local time (None if it is not a real date).
pub fn to_epoch(t: Local) -> Option<i64> {
    let ok = (1970..=2200).contains(&t.year)
        && (1..=12).contains(&t.month)
        && t.day >= 1
        && t.day <= days_in(t.year, t.month)
        && t.hour < 24
        && t.minute < 60
        && t.second < 60;
    if !ok {
        return None;
    }
    // SAFETY: mktime with a valid tm.
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        tm.tm_year = t.year - 1900;
        tm.tm_mon = t.month as i32 - 1;
        tm.tm_mday = t.day as i32;
        tm.tm_hour = t.hour as i32;
        tm.tm_min = t.minute as i32;
        tm.tm_sec = t.second as i32;
        tm.tm_isdst = -1;
        let s = libc::mktime(&mut tm);
        (s != -1).then_some(s as i64)
    }
}

/// "mm-dd-yyyy" (as DATE prints and reads it).
pub fn date_text(t: Local) -> String {
    format!("{:02}-{:02}-{:04}", t.month, t.day, t.year)
}

/// "hh:mm" (24-hour).
pub fn time_text(t: Local) -> String {
    format!("{:02}:{:02}", t.hour, t.minute)
}

/// Read "mm-dd-yyyy" (or with / or .) and "hh:mm[:ss]" back.
pub fn parse_local(date: &str, time: &str) -> Option<Local> {
    let d: Vec<&str> = date.trim().split(['-', '/', '.']).collect();
    let tm: Vec<&str> = time.trim().split(':').collect();
    let [m, dd, y] = d.as_slice() else {
        return None;
    };
    if tm.len() < 2 || tm.len() > 3 {
        return None;
    }
    let t = Local {
        year: y.trim().parse().ok()?,
        month: m.trim().parse().ok()?,
        day: dd.trim().parse().ok()?,
        hour: tm[0].trim().parse().ok()?,
        minute: tm[1].trim().parse().ok()?,
        second: tm.get(2).map_or(Some(0), |s| s.trim().parse().ok())?,
    };
    to_epoch(t).map(|_| t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_read_back() {
        let t = parse_local("02-29-2028", "23:59").unwrap();
        assert_eq!(
            (t.year, t.month, t.day, t.hour, t.minute),
            (2028, 2, 29, 23, 59)
        );
        assert_eq!(date_text(t), "02-29-2028");
        assert_eq!(time_text(t), "23:59");
        assert!(parse_local("02-29-2027", "10:00").is_none());
        assert!(parse_local("13-01-2027", "10:00").is_none());
        assert!(parse_local("12/31/2027", "24:00").is_none());
        assert!(parse_local("12.31.2027", "7:05:09").is_some());
    }
}

//! The Wastebasket: the freedesktop.org trash (~/.local/share/Trash), so
//! files thrown away here show up in every desktop's trash and back.

use std::path::{Path, PathBuf};

extern "C" {
    fn tzset();
}

/// Local time of `t` (seconds since the epoch): year, month, day, hour,
/// minute, second.
pub fn local_time(t: i64) -> (i32, u32, u32, u32, u32, u32) {
    // SAFETY: localtime_r writes into the tm we pass.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        let tt = t as libc::time_t;
        tzset();
        libc::localtime_r(&tt, &mut tm);
        tm
    };
    (
        tm.tm_year + 1900,
        (tm.tm_mon + 1) as u32,
        tm.tm_mday as u32,
        tm.tm_hour as u32,
        tm.tm_min as u32,
        tm.tm_sec as u32,
    )
}

/// Percent-encode a path for a .trashinfo file (RFC 2396, '/' kept).
pub fn encode_path(p: &Path) -> String {
    let mut out = String::new();
    for b in p.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn trash_dir(home: &Path) -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"))
        .join("Trash")
}

/// A name not yet used in the trash: "name", "name.2", "name.3"…
fn free_name(files: &Path, info: &Path, name: &str) -> String {
    (1..)
        .map(|n| {
            if n == 1 {
                name.to_string()
            } else {
                format!("{name}.{n}")
            }
        })
        .find(|n| !files.join(n).exists() && !info.join(format!("{n}.trashinfo")).exists())
        .unwrap_or_else(|| name.to_string())
}

/// Move `path` to the Wastebasket. Fails (and leaves the file) when the
/// file is on another file system than the trash (a USB stick): the caller
/// offers to delete it instead.
pub fn trash(path: &Path, home: &Path) -> std::io::Result<()> {
    let dir = trash_dir(home);
    let (files, info) = (dir.join("files"), dir.join("info"));
    std::fs::create_dir_all(&files)?;
    std::fs::create_dir_all(&info)?;
    let abs = std::fs::canonicalize(path)?;
    let base = abs
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let name = free_name(&files, &info, &base);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, mo, d, h, mi, s) = local_time(now);
    let info_file = info.join(format!("{name}.trashinfo"));
    std::fs::write(
        &info_file,
        format!(
            "[Trash Info]\nPath={}\nDeletionDate={y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}\n",
            encode_path(&abs)
        ),
    )?;
    if let Err(e) = std::fs::rename(&abs, files.join(&name)) {
        let _ = std::fs::remove_file(&info_file);
        return Err(e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_percent_encoded() {
        assert_eq!(
            encode_path(Path::new("/home/a b/ü.txt")),
            "/home/a%20b/%C3%BC.txt"
        );
    }

    #[test]
    fn trashing_moves_the_file_and_writes_its_info() {
        let home = std::env::temp_dir().join(format!("cabinet-trash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        // The test must not depend on the caller's XDG_DATA_HOME.
        if std::env::var_os("XDG_DATA_HOME").is_some() {
            return;
        }
        for _ in 0..2 {
            std::fs::write(home.join("old.txt"), "x").unwrap();
            trash(&home.join("old.txt"), &home).unwrap();
        }
        let t = home.join(".local/share/Trash");
        assert!(t.join("files/old.txt").exists() && t.join("files/old.txt.2").exists());
        let info = std::fs::read_to_string(t.join("info/old.txt.2.trashinfo")).unwrap();
        assert!(
            info.starts_with("[Trash Info]\nPath=/") && info.contains("old.txt\nDeletionDate=20")
        );
        assert!(!home.join("old.txt").exists());
        let _ = std::fs::remove_dir_all(home);
    }
}

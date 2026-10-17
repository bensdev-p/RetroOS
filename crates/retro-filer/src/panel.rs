//! One file panel: a directory listing with a cursor and marked files.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
    pub size: u64,
    /// Seconds since the epoch.
    pub mtime: i64,
    pub link: bool,
    pub exec: bool,
}

pub struct Panel {
    pub dir: PathBuf,
    pub entries: Vec<Entry>,
    pub cursor: usize,
    pub top: usize,
    pub marked: BTreeSet<String>,
    pub show_hidden: bool,
    pub error: Option<String>,
}

impl Panel {
    pub fn new(dir: &Path) -> Panel {
        let mut p = Panel {
            dir: dir.to_path_buf(),
            entries: Vec::new(),
            cursor: 0,
            top: 0,
            marked: BTreeSet::new(),
            show_hidden: false,
            error: None,
        };
        p.reload();
        p
    }

    /// Re-read the directory, keeping the cursor on the same name.
    pub fn reload(&mut self) {
        let keep = self.current().map(|e| e.name.clone());
        self.error = None;
        let mut v = Vec::new();
        if self.dir.parent().is_some() {
            v.push(Entry {
                name: "..".into(),
                dir: true,
                size: 0,
                mtime: 0,
                link: false,
                exec: false,
            });
        }
        match std::fs::read_dir(&self.dir) {
            Ok(rd) => {
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if name.starts_with('.') && !self.show_hidden {
                        continue;
                    }
                    let link = e.file_type().map(|t| t.is_symlink()).unwrap_or(false);
                    // Follow links for the kind and size.
                    let md = std::fs::metadata(e.path()).or_else(|_| e.metadata());
                    let Ok(md) = md else { continue };
                    use std::os::unix::fs::PermissionsExt;
                    v.push(Entry {
                        name,
                        dir: md.is_dir(),
                        size: md.len(),
                        mtime: md
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map_or(0, |d| d.as_secs() as i64),
                        link,
                        exec: md.is_file() && md.permissions().mode() & 0o111 != 0,
                    });
                }
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        // Directories first, then names without regard to case (as DIR /O).
        v[usize::from(self.dir.parent().is_some())..].sort_by(|a, b| {
            b.dir
                .cmp(&a.dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        self.entries = v;
        self.marked
            .retain(|m| self.entries.iter().any(|e| &e.name == m));
        self.cursor = keep
            .and_then(|k| self.entries.iter().position(|e| e.name == k))
            .unwrap_or(0)
            .min(self.entries.len().saturating_sub(1));
    }

    pub fn current(&self) -> Option<&Entry> {
        self.entries.get(self.cursor)
    }

    pub fn current_path(&self) -> Option<PathBuf> {
        self.current().map(|e| self.dir.join(&e.name))
    }

    /// Change directory; going up puts the cursor on the directory we left.
    pub fn cd(&mut self, to: &Path) -> Result<(), String> {
        let to = retro_dosfs::normalize(to);
        std::fs::read_dir(&to).map_err(|e| e.to_string())?;
        let from = self.dir.clone();
        self.dir = to;
        self.marked.clear();
        self.cursor = 0;
        self.top = 0;
        self.reload();
        if from.parent() == Some(self.dir.as_path()) {
            if let Some(n) = from.file_name().map(|n| n.to_string_lossy().into_owned()) {
                if let Some(i) = self.entries.iter().position(|e| e.name == n) {
                    self.cursor = i;
                }
            }
        }
        Ok(())
    }

    pub fn up(&mut self) -> Result<(), String> {
        match self.dir.parent() {
            Some(p) => {
                let p = p.to_path_buf();
                self.cd(&p)
            }
            None => Ok(()),
        }
    }

    pub fn toggle_mark(&mut self) {
        if let Some(e) = self.current() {
            if e.name != ".." {
                let n = e.name.clone();
                if !self.marked.remove(&n) {
                    self.marked.insert(n);
                }
            }
        }
        self.move_by(1);
    }

    /// Marked files, or the one under the cursor.
    pub fn selection(&self) -> Vec<PathBuf> {
        if self.marked.is_empty() {
            self.current()
                .filter(|e| e.name != "..")
                .map(|e| vec![self.dir.join(&e.name)])
                .unwrap_or_default()
        } else {
            self.marked.iter().map(|n| self.dir.join(n)).collect()
        }
    }

    pub fn move_by(&mut self, d: isize) {
        let n = self.entries.len() as isize;
        if n == 0 {
            return;
        }
        self.cursor = (self.cursor as isize + d).clamp(0, n - 1) as usize;
    }

    /// Jump to the next entry starting with `c` (quick search).
    pub fn jump(&mut self, c: char) {
        let c = c.to_lowercase().next().unwrap_or(c);
        let n = self.entries.len();
        for i in 1..=n {
            let j = (self.cursor + i) % n;
            if self.entries[j].name.to_lowercase().starts_with(c) {
                self.cursor = j;
                return;
            }
        }
    }

    pub fn scroll(&mut self, rows: usize) {
        if self.cursor < self.top {
            self.top = self.cursor;
        } else if rows > 0 && self.cursor >= self.top + rows {
            self.top = self.cursor + 1 - rows;
        }
    }

    pub fn marked_bytes(&self) -> u64 {
        self.entries
            .iter()
            .filter(|e| self.marked.contains(&e.name))
            .map(|e| e.size)
            .sum()
    }
}

// ---------------------------------------------------------------- file ops

/// Copy a file or directory tree into `dest_dir` (or to `dest` if it is not
/// a directory).
pub fn copy_into(src: &Path, dest: &Path) -> std::io::Result<()> {
    let target = if dest.is_dir() {
        dest.join(src.file_name().unwrap_or_default())
    } else {
        dest.to_path_buf()
    };
    if target == src {
        return Err(std::io::Error::other("File cannot be copied onto itself"));
    }
    if target.starts_with(src) {
        return Err(std::io::Error::other("Cannot copy a directory into itself"));
    }
    copy_tree(src, &target)
}

fn copy_tree(src: &Path, target: &Path) -> std::io::Result<()> {
    let md = std::fs::symlink_metadata(src)?;
    if md.file_type().is_symlink() {
        let to = std::fs::read_link(src)?;
        let _ = std::fs::remove_file(target);
        return std::os::unix::fs::symlink(to, target);
    }
    if md.is_dir() {
        std::fs::create_dir_all(target)?;
        for e in std::fs::read_dir(src)? {
            let e = e?;
            copy_tree(&e.path(), &target.join(e.file_name()))?;
        }
        return Ok(());
    }
    std::fs::copy(src, target).map(|_| ())
}

/// Move (rename, or copy then delete across file systems).
pub fn move_into(src: &Path, dest: &Path) -> std::io::Result<()> {
    let target = if dest.is_dir() {
        dest.join(src.file_name().unwrap_or_default())
    } else {
        dest.to_path_buf()
    };
    if target.starts_with(src) && target != src {
        return Err(std::io::Error::other("Cannot move a directory into itself"));
    }
    match std::fs::rename(src, &target) {
        Ok(()) => Ok(()),
        Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
            copy_tree(src, &target)?;
            delete(src)
        }
        Err(e) => Err(e),
    }
}

pub fn delete(p: &Path) -> std::io::Result<()> {
    let md = std::fs::symlink_metadata(p)?;
    if md.is_dir() {
        std::fs::remove_dir_all(p)
    } else {
        std::fs::remove_file(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("retro-filer-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn listing_order_marks_and_cd() {
        let d = tmp("list");
        std::fs::create_dir(d.join("games")).unwrap();
        std::fs::write(d.join("b.txt"), "bb").unwrap();
        std::fs::write(d.join("A.TXT"), "a").unwrap();
        std::fs::write(d.join(".hidden"), "").unwrap();
        let mut p = Panel::new(&d);
        let names: Vec<_> = p.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["..", "games", "A.TXT", "b.txt"]);
        p.cursor = 2;
        p.toggle_mark();
        p.toggle_mark();
        assert_eq!(p.marked.len(), 2);
        assert_eq!(p.marked_bytes(), 3);
        p.cd(&d.join("games")).unwrap();
        p.up().unwrap();
        assert_eq!(p.current().unwrap().name, "games");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn copy_move_delete_trees() {
        let d = tmp("ops");
        std::fs::create_dir_all(d.join("src/sub")).unwrap();
        std::fs::write(d.join("src/sub/f.txt"), "x").unwrap();
        std::fs::create_dir(d.join("dst")).unwrap();
        copy_into(&d.join("src"), &d.join("dst")).unwrap();
        assert_eq!(
            std::fs::read_to_string(d.join("dst/src/sub/f.txt")).unwrap(),
            "x"
        );
        assert!(copy_into(&d.join("src"), &d.join("src/sub")).is_err());
        move_into(&d.join("dst/src"), &d.join("moved")).unwrap();
        assert!(d.join("moved/sub/f.txt").exists() && !d.join("dst/src").exists());
        delete(&d.join("moved")).unwrap();
        assert!(!d.join("moved").exists());
        let _ = std::fs::remove_dir_all(d);
    }
}

//! retro-dosfs: the DOS view of a POSIX filesystem.
//!
//! * Drive letters: `C:` is the user's home, `Z:` is `/`, removable media get
//!   `D:` upward (sticky per mount name) and `A:` aliases the first removable
//!   drive. See docs/DESIGN.md §7.2.
//! * DOS path syntax (`C:\DOCS\FILE.TXT`, `\ROOT`, `..`, per-drive current
//!   directories) resolved **case-insensitively, case-preserving** against
//!   the real filesystem. Raw POSIX paths (`/usr/share`) are accepted too.
//! * DOS wildcards and Win95-style 8.3 aliases.

mod glob;
mod short;

pub use glob::{dos_match, has_wildcards};
pub use short::{is_valid_83, short_names, split_83};

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriveKind {
    Home,
    Root,
    Removable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drive {
    pub letter: char,
    pub root: PathBuf,
    pub label: String,
    pub kind: DriveKind,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PathError {
    InvalidDrive(char),
    PathNotFound(String),
    Ambiguous(String, Vec<String>),
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathError::InvalidDrive(_) => write!(f, "Invalid drive specification"),
            PathError::PathNotFound(_) => write!(f, "Path not found"),
            PathError::Ambiguous(n, c) => write!(f, "Ambiguous name {n}: {}", c.join(", ")),
        }
    }
}

impl std::error::Error for PathError {}

/// The drive table plus per-drive current directories (DOS semantics).
#[derive(Clone, Debug)]
pub struct Dos {
    drives: BTreeMap<char, Drive>,
    /// Current directory on each drive, as real path components below the drive root.
    cwd: BTreeMap<char, Vec<String>>,
    current: char,
    /// Show synthetic 8.3 aliases (SET LFN=OFF).
    pub lfn: bool,
}

impl Dos {
    /// `C:` = `home`, `Z:` = `/`, current drive C: at the home root.
    pub fn new(home: &Path) -> Dos {
        let mut d = Dos {
            drives: BTreeMap::new(),
            cwd: BTreeMap::new(),
            current: 'C',
            lfn: true,
        };
        d.add(Drive {
            letter: 'C',
            root: normalize(home),
            label: "HOME".into(),
            kind: DriveKind::Home,
        });
        d.add(Drive {
            letter: 'Z',
            root: PathBuf::from("/"),
            label: "SYSTEM".into(),
            kind: DriveKind::Root,
        });
        d
    }

    pub fn add(&mut self, drive: Drive) {
        self.cwd.entry(drive.letter).or_default();
        self.drives.insert(drive.letter, drive);
    }

    pub fn remove(&mut self, letter: char) {
        let letter = letter.to_ascii_uppercase();
        self.drives.remove(&letter);
        self.cwd.remove(&letter);
        if self.current == letter {
            self.current = 'C';
        }
    }

    pub fn drives(&self) -> impl Iterator<Item = &Drive> {
        self.drives.values()
    }

    pub fn drive(&self, letter: char) -> Option<&Drive> {
        let l = letter.to_ascii_uppercase();
        if l == 'A' {
            return self
                .drives
                .values()
                .find(|d| d.kind == DriveKind::Removable);
        }
        self.drives.get(&l)
    }

    pub fn current_drive(&self) -> char {
        self.current
    }

    /// Rescan removable media mounted under `/media/<user>` and `/run/media/<user>`.
    /// Letters are sticky per mount name for the lifetime of this table.
    pub fn refresh_removable(&mut self, user: &str) {
        let mut mounts: Vec<(String, PathBuf)> = Vec::new();
        for base in [format!("/media/{user}"), format!("/run/media/{user}")] {
            if let Ok(rd) = std::fs::read_dir(&base) {
                for e in rd.flatten() {
                    if e.path().is_dir() {
                        mounts.push((e.file_name().to_string_lossy().into_owned(), e.path()));
                    }
                }
            }
        }
        mounts.sort();
        // Drop vanished drives.
        let gone: Vec<char> = self
            .drives
            .values()
            .filter(|d| d.kind == DriveKind::Removable && !mounts.iter().any(|(_, p)| *p == d.root))
            .map(|d| d.letter)
            .collect();
        for l in gone {
            self.remove(l);
        }
        for (name, path) in mounts {
            if self.drives.values().any(|d| d.root == path) {
                continue;
            }
            if let Some(letter) = ('D'..='Y').find(|l| !self.drives.contains_key(l)) {
                self.add(Drive {
                    letter,
                    root: path,
                    label: name.to_uppercase(),
                    kind: DriveKind::Removable,
                });
            }
        }
    }

    /// Change the current drive (`D:` typed alone).
    pub fn set_drive(&mut self, letter: char) -> Result<(), PathError> {
        let d = self.drive(letter).ok_or(PathError::InvalidDrive(letter))?;
        let l = d.letter;
        if !d.root.is_dir() {
            return Err(PathError::InvalidDrive(letter));
        }
        self.current = l;
        Ok(())
    }

    /// The current directory as a POSIX path.
    pub fn cwd_path(&self) -> PathBuf {
        self.drive_cwd_path(self.current)
            .unwrap_or_else(|| PathBuf::from("/"))
    }

    fn drive_cwd_path(&self, letter: char) -> Option<PathBuf> {
        let d = self.drive(letter)?;
        let mut p = d.root.clone();
        for c in self.cwd.get(&d.letter).into_iter().flatten() {
            p.push(c);
        }
        Some(p)
    }

    /// `C:\DOCS` style display of the current directory.
    pub fn cwd_display(&self) -> String {
        self.display(&self.cwd_path())
    }

    /// Change directory. Accepts anything [`Dos::resolve`] accepts; a bare
    /// `D:path` changes drive D's directory without switching drives (DOS rule).
    pub fn chdir(&mut self, input: &str) -> Result<(), PathError> {
        let (drive_spec, _) = split_drive(input);
        let target = self.resolve(input)?;
        if !target.is_dir() {
            return Err(PathError::PathNotFound(input.into()));
        }
        let (letter, comps) = self.to_dos(&target);
        self.cwd.insert(letter, comps);
        // `CD D:\X` only changes D's directory; anything else (relative, `\X`,
        // or a POSIX path, which may land inside another drive) moves us there.
        if drive_spec.is_none() {
            self.current = letter;
        }
        Ok(())
    }

    /// Map a POSIX path to (drive letter, components below the drive root).
    /// The most specific drive wins; `Z:` matches everything.
    pub fn to_dos(&self, posix: &Path) -> (char, Vec<String>) {
        let p = normalize(posix);
        let mut best: Option<&Drive> = None;
        for d in self.drives.values() {
            if p.starts_with(&d.root)
                && best.is_none_or(|b| d.root.components().count() > b.root.components().count())
            {
                best = Some(d);
            }
        }
        let d = best.expect("Z: covers /");
        let rest = p.strip_prefix(&d.root).unwrap_or(Path::new(""));
        let comps = rest
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        (d.letter, comps)
    }

    /// POSIX → `C:\DOCS\FILE.TXT`.
    pub fn display(&self, posix: &Path) -> String {
        let (l, comps) = self.to_dos(posix);
        if self.lfn {
            format!("{l}:\\{}", comps.join("\\"))
        } else {
            // 8.3 aliases for every component.
            let mut dir = self.drive(l).map(|d| d.root.clone()).unwrap_or_default();
            let mut out = Vec::new();
            for c in comps {
                let siblings = list_names(&dir);
                let map = short_names(&siblings);
                out.push(map.get(&c).cloned().unwrap_or_else(|| c.to_uppercase()));
                dir.push(&c);
            }
            format!("{l}:\\{}", out.join("\\"))
        }
    }

    /// Resolve DOS or POSIX path syntax to a POSIX path.
    ///
    /// Existing components are matched case-insensitively (an exact match wins,
    /// otherwise a unique case-insensitive match, otherwise an 8.3 alias). The
    /// final component may not exist (for MD, COPY targets…); intermediate ones must.
    pub fn resolve(&self, input: &str) -> Result<PathBuf, PathError> {
        self.resolve_inner(input, false)
    }

    /// Like [`Dos::resolve`], but missing intermediate directories are kept
    /// as typed (for MD and other commands that create paths).
    pub fn resolve_new(&self, input: &str) -> Result<PathBuf, PathError> {
        self.resolve_inner(input, true)
    }

    fn resolve_inner(&self, input: &str, lenient: bool) -> Result<PathBuf, PathError> {
        let input = input.trim();
        if input.starts_with('/') {
            return Ok(normalize(Path::new(input)));
        }
        if let Some(rest) = input.strip_prefix('~') {
            if rest.is_empty() || rest.starts_with(['/', '\\']) {
                let home = self.drive('C').map(|d| d.root.clone()).unwrap_or_default();
                return self.walk(home.clone(), &home, rest, lenient);
            }
        }
        let (drive_spec, rest) = split_drive(input);
        let letter = match drive_spec {
            Some(l) => self.drive(l).ok_or(PathError::InvalidDrive(l))?.letter,
            None => self.current,
        };
        let root = self
            .drive(letter)
            .ok_or(PathError::InvalidDrive(letter))?
            .root
            .clone();
        let start = if rest.starts_with(['\\', '/']) {
            root.clone()
        } else {
            self.drive_cwd_path(letter).unwrap_or_else(|| root.clone())
        };
        self.walk(start, &root, rest, lenient)
    }

    fn walk(
        &self,
        start: PathBuf,
        root: &Path,
        rest: &str,
        lenient: bool,
    ) -> Result<PathBuf, PathError> {
        let parts: Vec<&str> = rest.split(['\\', '/']).filter(|s| !s.is_empty()).collect();
        let mut p = start;
        for (i, part) in parts.iter().enumerate() {
            let last = i + 1 == parts.len();
            match *part {
                "." => {}
                ".." => {
                    if p != root && p.starts_with(root) {
                        p.pop();
                    }
                }
                // DOS "..." = two levels up.
                s if s.len() > 2 && s.chars().all(|c| c == '.') => {
                    for _ in 0..s.len() - 1 {
                        if p != root && p.starts_with(root) {
                            p.pop();
                        }
                    }
                }
                name => match find_entry(&p, name)? {
                    Some(real) => p.push(real),
                    None if last || lenient || has_wildcards(name) => p.push(name),
                    None => return Err(PathError::PathNotFound(name.into())),
                },
            }
        }
        Ok(p)
    }

    /// Expand a DOS wildcard pattern into matching entries (sorted, case-insensitively).
    /// Non-wildcard input resolves to at most one existing path.
    pub fn expand(&self, input: &str) -> Result<Vec<PathBuf>, PathError> {
        let full = self.resolve(input)?;
        let name = full
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !has_wildcards(&name) {
            return Ok(if full.exists() || full.is_symlink() {
                vec![full]
            } else {
                vec![]
            });
        }
        let dir = full
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("/"));
        let names = list_names(&dir);
        let shorts = short_names(&names);
        let mut out: Vec<PathBuf> = names
            .iter()
            .filter(|n| {
                (!n.starts_with('.') || name.starts_with('.'))
                    && (dos_match(&name, n) || shorts.get(*n).is_some_and(|s| dos_match(&name, s)))
            })
            .map(|n| dir.join(n))
            .collect();
        out.sort_by_key(|p| p.to_string_lossy().to_lowercase());
        Ok(out)
    }
}

/// Split `D:rest` into (Some('D'), "rest").
pub fn split_drive(s: &str) -> (Option<char>, &str) {
    let mut it = s.chars();
    match (it.next(), it.next()) {
        (Some(c), Some(':')) if c.is_ascii_alphabetic() => (Some(c.to_ascii_uppercase()), &s[2..]),
        _ => (None, s),
    }
}

/// Lexical normalisation (no symlink resolution): removes `.`, folds `..`.
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::RootDir => out.push("/"),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(n) => out.push(n),
            Component::Prefix(_) => {}
        }
    }
    if out.as_os_str().is_empty() {
        out.push("/");
    }
    out
}

pub fn list_names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// Find `name` in `dir`: exact, then unique case-insensitive, then 8.3 alias.
pub fn find_entry(dir: &Path, name: &str) -> Result<Option<String>, PathError> {
    if dir.join(name).symlink_metadata().is_ok() {
        return Ok(Some(name.to_string()));
    }
    let names = list_names(dir);
    let lower = name.to_lowercase();
    let ci: Vec<&String> = names.iter().filter(|n| n.to_lowercase() == lower).collect();
    match ci.len() {
        1 => return Ok(Some(ci[0].clone())),
        n if n > 1 => {
            return Err(PathError::Ambiguous(
                name.into(),
                ci.into_iter().cloned().collect(),
            ));
        }
        _ => {}
    }
    if name.contains('~') {
        let shorts = short_names(&names);
        let upper = name.to_uppercase();
        if let Some((long, _)) = shorts.iter().find(|(_, s)| **s == upper) {
            return Ok(Some(long.clone()));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("retro-dosfs-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(p.join("home/Documents/Letters")).unwrap();
        fs::write(p.join("home/Documents/Report Final.txt"), "x").unwrap();
        fs::write(p.join("home/readme.TXT"), "x").unwrap();
        p
    }

    #[test]
    fn drives_and_display() {
        let t = tmp("disp");
        let dos = Dos::new(&t.join("home"));
        assert_eq!(dos.cwd_display(), "C:\\");
        assert_eq!(dos.display(&t.join("home/Documents")), "C:\\Documents");
        assert_eq!(dos.display(Path::new("/etc/hosts")), "Z:\\etc\\hosts");
        assert_eq!(dos.display(Path::new("/")), "Z:\\");
    }

    #[test]
    fn case_insensitive_resolution_and_cd() {
        let t = tmp("cd");
        let mut dos = Dos::new(&t.join("home"));
        dos.chdir("documents\\LETTERS").unwrap();
        assert_eq!(dos.cwd_display(), "C:\\Documents\\Letters");
        dos.chdir("..").unwrap();
        assert_eq!(dos.cwd_display(), "C:\\Documents");
        dos.chdir("\\").unwrap();
        assert_eq!(dos.cwd_display(), "C:\\");
        // Can't go above the drive root.
        dos.chdir("..\\..").unwrap();
        assert_eq!(dos.cwd_display(), "C:\\");
        assert_eq!(
            dos.resolve("README.txt").unwrap(),
            t.join("home/readme.TXT")
        );
        assert!(matches!(
            dos.chdir("nope\\x"),
            Err(PathError::PathNotFound(_))
        ));
    }

    #[test]
    fn per_drive_cwd_and_posix() {
        let t = tmp("drv");
        let mut dos = Dos::new(&t.join("home"));
        dos.chdir("Z:\\etc").unwrap(); // changes Z:'s cwd, not the current drive
        assert_eq!(dos.current_drive(), 'C');
        dos.set_drive('z').unwrap();
        assert_eq!(dos.cwd_display(), "Z:\\etc");
        dos.chdir("/usr").unwrap();
        assert_eq!(dos.cwd_display(), "Z:\\usr");
        assert_eq!(
            dos.resolve("C:readme.txt").unwrap(),
            t.join("home/readme.TXT")
        );
        assert_eq!(dos.set_drive('Q'), Err(PathError::InvalidDrive('Q')));
    }

    #[test]
    fn wildcards_and_aliases() {
        let t = tmp("wild");
        let dos = Dos::new(&t.join("home"));
        let m = dos.expand("*.TXT").unwrap();
        assert_eq!(m, vec![t.join("home/readme.TXT")]);
        let all = dos.expand("documents\\*.*").unwrap();
        assert_eq!(all.len(), 2);
        // 8.3 alias resolves.
        assert_eq!(
            dos.resolve("DOCUME~1\\REPORT~1.TXT").unwrap(),
            t.join("home/Documents/Report Final.txt")
        );
    }

    #[test]
    fn lfn_off_display() {
        let t = tmp("lfn");
        let mut dos = Dos::new(&t.join("home"));
        dos.lfn = false;
        assert_eq!(
            dos.display(&t.join("home/Documents/Report Final.txt")),
            "C:\\DOCUME~1\\REPORT~1.TXT"
        );
    }
}

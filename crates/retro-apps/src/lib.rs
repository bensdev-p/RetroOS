//! Finding installed applications and telling GUI programs from text ones
//! (DESIGN.md §7.4); the Desktop's Programs menu groups them by category.
//!
//! A program is a GUI program if a `.desktop` entry without `Terminal=true`
//! runs it, or if its ELF image links a GUI toolkit. GUI programs go to
//! Station's launch broker instead of running in the console's pty.

use std::path::{Path, PathBuf};

/// RetroOS names for applications whose program is called something else.
const ALIASES: &[(&str, &str)] = &[
    ("CHROME", "chromium"),
    ("BROWSER", "firefox"),
    ("VSCODE", "code"),
    ("ARCADE", "retroarch"),
    ("MEDIA", "mpv"),
];

/// Libraries whose presence in DT_NEEDED marks a GUI program.
const GUI_LIBS: &[&str] = &[
    "libwayland-client",
    "libX11",
    "libxcb.so",
    "libgtk-",
    "libgdk-",
    "libQt5Gui",
    "libQt6Gui",
    "libSDL2",
    "libSDL3",
    "libSDL-1",
    "libglfw",
    "libEGL",
    "libGL.so",
];

#[derive(Clone, Debug)]
pub struct DesktopApp {
    pub id: String,
    pub name: String,
    pub exec: Vec<String>,
    pub terminal: bool,
    /// freedesktop.org categories (`Network;WebBrowser;`).
    pub categories: Vec<String>,
    /// NoDisplay=true: runs by name, but is not listed in menus.
    pub no_display: bool,
}

fn data_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(h) = std::env::var_os("XDG_DATA_HOME") {
        v.push(PathBuf::from(h));
    } else if let Some(h) = std::env::var_os("HOME") {
        v.push(PathBuf::from(h).join(".local/share"));
    }
    let sys = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    v.extend(sys.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
    v
}

/// Every application entry, user entries shadowing system ones.
pub fn desktop_apps() -> Vec<DesktopApp> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for d in data_dirs() {
        let Ok(rd) = std::fs::read_dir(d.join("applications")) else {
            continue;
        };
        let mut files: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        files.sort();
        for p in files {
            if p.extension().is_none_or(|e| e != "desktop") {
                continue;
            }
            let id = p.file_stem().unwrap().to_string_lossy().into_owned();
            if !seen.insert(id.clone()) {
                continue;
            }
            if let Some(app) = std::fs::read_to_string(&p)
                .ok()
                .and_then(|t| parse_desktop(&id, &t))
            {
                out.push(app);
            }
        }
    }
    out
}

pub fn parse_desktop(id: &str, text: &str) -> Option<DesktopApp> {
    let mut in_entry = false;
    let (mut name, mut exec, mut terminal, mut hidden, mut app) = (None, None, false, false, false);
    let (mut categories, mut no_display) = (Vec::new(), false);
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k.trim() {
            "Name" => name = Some(v.trim().to_string()),
            "Exec" => exec = Some(v.trim().to_string()),
            "Terminal" => terminal = v.trim() == "true",
            "Hidden" => hidden = v.trim() == "true",
            "Type" => app = v.trim() == "Application",
            "Categories" => {
                categories = v
                    .split(';')
                    .map(str::trim)
                    .filter(|c| !c.is_empty())
                    .map(String::from)
                    .collect()
            }
            "NoDisplay" => no_display = v.trim() == "true",
            _ => {}
        }
    }
    if hidden || !app {
        return None;
    }
    let exec = split_exec(&exec?);
    if exec.is_empty() {
        return None;
    }
    Some(DesktopApp {
        id: id.to_string(),
        name: name.unwrap_or_else(|| id.to_string()),
        exec,
        terminal,
        categories,
        no_display,
    })
}

/// The Program Manager group an application belongs in (DESIGN.md §9.1),
/// from its freedesktop.org categories.
pub fn group_of(app: &DesktopApp) -> &'static str {
    let has = |c: &str| app.categories.iter().any(|x| x == c);
    if has("Game") {
        "Games"
    } else if has("Network") || has("WebBrowser") || has("Email") {
        "Internet"
    } else if has("AudioVideo") || has("Audio") || has("Video") {
        "Multimedia"
    } else if has("Graphics") {
        "Graphics"
    } else if has("Office") {
        "Office"
    } else if has("Development") {
        "Programming"
    } else if has("Settings") || has("System") {
        "System"
    } else {
        "Accessories"
    }
}

/// Split an Exec= line into argv, dropping field codes (%f, %U, …).
fn split_exec(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    let mut it = s.chars();
    while let Some(c) = it.next() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            '\\' if quoted => {
                if let Some(n) = it.next() {
                    cur.push(n);
                }
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                any = false;
            }
            _ => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out.into_iter()
        .filter(|a| !(a.len() == 2 && a.starts_with('%')))
        .map(|a| a.replace("%%", "%"))
        .collect()
}

fn base(s: &str) -> &str {
    s.rsplit('/').next().unwrap_or(s)
}

/// An application entry for a command name: by alias, entry id
/// (`firefox`, `org.gnome.Foo` → `FOO`), program name, or display name
/// without spaces.
pub fn find_app(cmd: &str) -> Option<DesktopApp> {
    let cmd = cmd.to_ascii_lowercase();
    let want = ALIASES
        .iter()
        .find(|(a, _)| a.eq_ignore_ascii_case(&cmd))
        .map(|(_, t)| t.to_string())
        .unwrap_or(cmd);
    let apps = desktop_apps();
    let matches = |a: &DesktopApp| {
        let id = a.id.to_ascii_lowercase();
        id == want
            || id.rsplit('.').next() == Some(want.as_str())
            || base(&a.exec[0]).eq_ignore_ascii_case(&want)
            || a.name.replace(' ', "").eq_ignore_ascii_case(&want)
    };
    apps.iter().find(|a| matches(a)).cloned()
}

/// Is the program at `path` a GUI program?
pub fn is_gui_program(path: &Path) -> bool {
    let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    for a in desktop_apps() {
        if a.terminal {
            continue;
        }
        let e = Path::new(&a.exec[0]);
        let hit = if e.is_absolute() {
            std::fs::canonicalize(e).map(|p| p == real).unwrap_or(false) || e == path
        } else {
            base(&a.exec[0]) == path.file_name().unwrap_or_default().to_string_lossy()
        };
        if hit {
            return true;
        }
    }
    elf_needed(&real)
        .map(|libs| {
            libs.iter()
                .any(|l| GUI_LIBS.iter().any(|g| l.starts_with(g)))
        })
        .unwrap_or(false)
}

/// DT_NEEDED entries of a 64-bit little-endian ELF file.
pub fn elf_needed(path: &Path) -> Option<Vec<String>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let mut h = [0u8; 64];
    f.read_exact(&mut h).ok()?;
    if &h[..4] != b"\x7fELF" || h[4] != 2 || h[5] != 1 {
        return None;
    }
    let u16_at = |b: &[u8], o: usize| u16::from_le_bytes([b[o], b[o + 1]]) as u64;
    let u64_at = |b: &[u8], o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    let (phoff, phentsize, phnum) = (u64_at(&h, 32), u16_at(&h, 54), u16_at(&h, 56));
    if phentsize < 56 || phnum > 128 {
        return None;
    }
    let mut ph = vec![0u8; (phentsize * phnum) as usize];
    f.seek(SeekFrom::Start(phoff)).ok()?;
    f.read_exact(&mut ph).ok()?;
    // (vaddr, offset, filesz) of PT_LOAD segments, and the PT_DYNAMIC segment.
    let mut loads = Vec::new();
    let mut dynamic = None;
    for i in 0..phnum as usize {
        let p = &ph[i * phentsize as usize..];
        let kind = u32::from_le_bytes(p[..4].try_into().unwrap());
        let (off, vaddr, filesz) = (u64_at(p, 8), u64_at(p, 16), u64_at(p, 32));
        match kind {
            1 => loads.push((vaddr, off, filesz)),
            2 => dynamic = Some((off, filesz)),
            _ => {}
        }
    }
    let (doff, dsize) = dynamic?;
    if dsize > 1 << 20 {
        return None;
    }
    let mut dynb = vec![0u8; dsize as usize];
    f.seek(SeekFrom::Start(doff)).ok()?;
    f.read_exact(&mut dynb).ok()?;
    let mut needed = Vec::new();
    let (mut strtab, mut strsz) = (None, 0u64);
    for e in dynb.chunks_exact(16) {
        let (tag, val) = (u64_at(e, 0), u64_at(e, 8));
        match tag {
            0 => break,
            1 => needed.push(val),
            5 => strtab = Some(val),
            10 => strsz = val,
            _ => {}
        }
    }
    let strtab = strtab?;
    let (va, off, _) = loads
        .iter()
        .find(|(va, _, sz)| strtab >= *va && strtab < va + sz)?;
    if strsz == 0 || strsz > 1 << 22 {
        return None;
    }
    let mut strs = vec![0u8; strsz as usize];
    f.seek(SeekFrom::Start(strtab - va + off)).ok()?;
    f.read_exact(&mut strs).ok()?;
    Some(
        needed
            .into_iter()
            .filter_map(|o| {
                let s = strs.get(o as usize..)?;
                let end = s.iter().position(|b| *b == 0)?;
                Some(String::from_utf8_lossy(&s[..end]).into_owned())
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_entries() {
        let a = parse_desktop(
            "firefox",
            "[Desktop Entry]\nName=Firefox\nExec=/usr/lib/firefox/firefox %u\nType=Application\n[Desktop Action new]\nExec=firefox --new-window\n",
        )
        .unwrap();
        assert_eq!(a.exec, ["/usr/lib/firefox/firefox"]);
        assert!(!a.terminal);
        assert_eq!(group_of(&a), "Accessories");
        let b = parse_desktop(
            "f",
            "[Desktop Entry]\nName=F\nExec=f\nType=Application\nCategories=Network;WebBrowser;\nNoDisplay=true\n",
        )
        .unwrap();
        assert_eq!(b.categories, ["Network", "WebBrowser"]);
        assert!(b.no_display);
        assert_eq!(group_of(&b), "Internet");
        assert!(parse_desktop("x", "[Desktop Entry]\nType=Link\nExec=x\n").is_none());
        assert_eq!(
            split_exec(r#""/opt/My App/run" --flag %F"#),
            ["/opt/My App/run", "--flag"]
        );
    }

    #[test]
    fn reads_dt_needed() {
        // Our own test binary links libc.
        let me = std::env::current_exe().unwrap();
        let libs = elf_needed(&me).unwrap();
        assert!(libs.iter().any(|l| l.starts_with("libc.so")), "{libs:?}");
        assert!(!is_gui_program(&me));
    }
}

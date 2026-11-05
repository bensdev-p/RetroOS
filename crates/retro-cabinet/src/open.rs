//! Which program opens a file: the type from its extension, then the first
//! application whose .desktop file lists that type; plain text (and small
//! files that read as UTF-8) opens in Jot when nothing else claims it.

use retro_apps::DesktopApp;
use std::path::Path;

/// The MIME type of a file name, from its extension.
pub fn mime_of(name: &str) -> Option<&'static str> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "txt" | "text" | "log" | "md" | "conf" | "cfg" | "ini" | "sh" | "bat" | "py" | "rs"
        | "c" | "h" | "cpp" | "toml" | "yaml" | "yml" | "csv" | "nfo" | "diz" => "text/plain",
        "json" => "application/json",
        "xml" => "application/xml",
        "html" | "htm" => "text/html",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "mp3" => "audio/mpeg",
        "ogg" | "oga" => "audio/ogg",
        "flac" => "audio/flac",
        "wav" => "audio/x-wav",
        "mp4" | "m4v" => "video/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        _ => return None,
    })
}

/// A short description for Get Info and the list ("Text document").
pub fn kind_of(name: &str, dir: bool) -> &'static str {
    if dir {
        return "Folder";
    }
    match mime_of(name) {
        Some(m) if m.starts_with("text/plain") => "Text document",
        Some("text/html") => "Web page",
        Some(m) if m.starts_with("image/") => "Picture",
        Some(m) if m.starts_with("audio/") => "Sound",
        Some(m) if m.starts_with("video/") => "Movie",
        Some("application/pdf") => "PDF document",
        Some(_) => "Document",
        None => "File",
    }
}

fn looks_like_text(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.len() <= 4 << 20)
        && std::fs::read(path).is_ok_and(|b| !b.contains(&0) && std::str::from_utf8(&b).is_ok())
}

/// The command line that opens `path`, from `apps`; None if nothing can.
pub fn command_for(path: &Path, apps: &[DesktopApp]) -> Option<(String, Vec<String>)> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let file = path.to_string_lossy().into_owned();
    if name.ends_with(".desktop") {
        let text = std::fs::read_to_string(path).ok()?;
        let app = retro_apps::parse_desktop("", &text)?;
        return Some((app.name, app.exec));
    }
    let mime = mime_of(&name);
    if let Some(m) = mime {
        if let Some(a) = apps.iter().find(|a| a.mime_types.iter().any(|x| x == m)) {
            let mut argv = a.exec.clone();
            argv.push(file);
            return Some((a.name.clone(), argv));
        }
    }
    let text = mime.is_some_and(|m| {
        m.starts_with("text/") || m == "application/json" || m == "application/xml"
    });
    (text || (mime.is_none() && looks_like_text(path)))
        .then(|| ("Jot".to_string(), vec!["jot".to_string(), file]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str, exec: &str, mime: &[&str]) -> DesktopApp {
        retro_apps::parse_desktop(
            name,
            &format!(
                "[Desktop Entry]\nType=Application\nName={name}\nExec={exec} %U\nMimeType={};\n",
                mime.join(";")
            ),
        )
        .unwrap()
    }

    #[test]
    fn types_and_kinds() {
        assert_eq!(mime_of("Photo.JPG"), Some("image/jpeg"));
        assert_eq!(mime_of("Makefile"), None);
        assert_eq!(kind_of("a.txt", false), "Text document");
        assert_eq!(kind_of("x", true), "Folder");
    }

    #[test]
    fn the_listed_program_opens_its_type_and_jot_the_rest_of_text() {
        let apps = [app("Firefox", "firefox", &["text/html", "image/png"])];
        let (n, argv) = command_for(Path::new("/h/page.html"), &apps).unwrap();
        assert_eq!(
            (n.as_str(), argv),
            ("Firefox", vec!["firefox".into(), "/h/page.html".into()])
        );
        let (n, _) = command_for(Path::new("/h/notes.txt"), &apps).unwrap();
        assert_eq!(n, "Jot");
        assert!(command_for(Path::new("/h/song.mp3"), &apps).is_none());
    }
}

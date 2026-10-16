//! Win95-style 8.3 short-name aliases (`PROGRA~1`), generated deterministically
//! from the sorted directory listing.

use std::collections::{HashMap, HashSet};

const SPECIAL: &str = "!#$%&'()-@^_`{}~";

fn valid_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || SPECIAL.contains(c)
}

/// Split a name into (base, extension) at the last dot (a leading dot doesn't count).
pub fn split_83(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i + 1..]),
        _ => (name, ""),
    }
}

/// Is `name` already a legal 8.3 name (ignoring case)?
pub fn is_valid_83(name: &str) -> bool {
    if name.starts_with('.') || name.matches('.').count() > 1 {
        return false;
    }
    let (b, e) = split_83(name);
    !b.is_empty() && b.len() <= 8 && e.len() <= 3 && b.chars().chain(e.chars()).all(valid_char)
}

fn clean(s: &str) -> String {
    s.chars()
        .filter(|c| *c != ' ' && *c != '.')
        .map(|c| {
            if valid_char(c) {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// Map every long name in a directory to its 8.3 alias.
pub fn short_names(names: &[String]) -> HashMap<String, String> {
    let mut sorted: Vec<&String> = names.iter().collect();
    sorted.sort();
    let mut used: HashSet<String> = HashSet::new();
    let mut out = HashMap::new();
    // Names that are already 8.3 keep their (upper-cased) name and reserve it first.
    for n in &sorted {
        if is_valid_83(n) {
            let u = n.to_ascii_uppercase();
            if used.insert(u.clone()) {
                out.insert((*n).clone(), u);
            }
        }
    }
    for n in sorted {
        if out.contains_key(n) {
            continue;
        }
        let stripped = n.trim_start_matches('.');
        let (b, e) = split_83(stripped);
        let base = clean(b);
        let ext: String = clean(e).chars().take(3).collect();
        let base = if base.is_empty() {
            "_".to_string()
        } else {
            base
        };
        for k in 1u32.. {
            let tail = format!("~{k}");
            let keep = 8usize.saturating_sub(tail.len()).min(6);
            let stem: String = base.chars().take(keep).collect();
            let cand = if ext.is_empty() {
                format!("{stem}{tail}")
            } else {
                format!("{stem}{tail}.{ext}")
            };
            if used.insert(cand.clone()) {
                out.insert(n.clone(), cand);
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases() {
        let names: Vec<String> = [
            "Program Files",
            "Program Data",
            "README.TXT",
            "notes.txt",
            "archive.tar.gz",
            ".bashrc",
            "a+b.c",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let m = short_names(&names);
        assert_eq!(m["Program Data"], "PROGRA~1");
        assert_eq!(m["Program Files"], "PROGRA~2");
        assert_eq!(m["README.TXT"], "README.TXT");
        assert_eq!(m["notes.txt"], "NOTES.TXT");
        assert_eq!(m["archive.tar.gz"], "ARCHIV~1.GZ");
        assert_eq!(m[".bashrc"], "BASHRC~1");
        assert_eq!(m["a+b.c"], "A_B~1.C");
    }
}

//! DOS wildcards: `*` and `?`, case-insensitive. `*.*` matches every name
//! (including names without an extension) and `*.` matches names without one.

pub fn has_wildcards(s: &str) -> bool {
    s.contains(['*', '?'])
}

pub fn dos_match(pattern: &str, name: &str) -> bool {
    let p = pattern.to_lowercase();
    let n = name.to_lowercase();
    if p == "*.*" || p == "*" {
        return true;
    }
    if let Some(base) = p.strip_suffix('.') {
        // "FOO*." — names with no extension.
        if !base.is_empty() && !base.ends_with('.') {
            return !n.contains('.') && wild(base.as_bytes(), n.as_bytes());
        }
    }
    if let Some(base) = p.strip_suffix(".*") {
        // "README.*" also matches "README".
        if !n.contains('.') && wild(base.as_bytes(), n.as_bytes()) {
            return true;
        }
    }
    wild(p.as_bytes(), n.as_bytes())
}

fn wild(p: &[u8], n: &[u8]) -> bool {
    // Iterative glob with backtracking on the last '*'.
    let (mut pi, mut ni) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == b'?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == b'*' {
            star = Some(pi);
            mark = ni;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::dos_match;

    #[test]
    fn matches() {
        assert!(dos_match("*.*", "Makefile"));
        assert!(dos_match("*.TXT", "notes.txt"));
        assert!(!dos_match("*.TXT", "notes.txt.bak"));
        assert!(dos_match("*.", "Makefile"));
        assert!(!dos_match("*.", "a.out"));
        assert!(dos_match("REPORT?.DOC", "report1.doc"));
        assert!(!dos_match("REPORT?.DOC", "report12.doc"));
        assert!(dos_match("README.*", "README"));
        assert!(dos_match("a*b*c", "aXXbYYc"));
        assert!(!dos_match("a*b*c", "aXXbYY"));
    }
}

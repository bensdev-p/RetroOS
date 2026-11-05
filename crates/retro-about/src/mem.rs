//! Memory figures from /proc: the system's (meminfo) and each program's
//! (the proportional set size of its process and all its children, so a
//! browser's content processes count towards it).

use std::collections::HashMap;

/// `/proc/meminfo` as kB values ("MemTotal" → 8048000).
pub fn parse_meminfo(text: &str) -> HashMap<String, u64> {
    text.lines()
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            let kb = v.split_whitespace().next()?.parse().ok()?;
            Some((k.trim().to_string(), kb))
        })
        .collect()
}

pub fn meminfo() -> HashMap<String, u64> {
    std::fs::read_to_string("/proc/meminfo")
        .map(|t| parse_meminfo(&t))
        .unwrap_or_default()
}

/// The "Pss:" line of a smaps_rollup file, in kB.
pub fn parse_pss(text: &str) -> Option<u64> {
    text.lines()
        .find_map(|l| l.strip_prefix("Pss:"))
        .and_then(|v| v.split_whitespace().next()?.parse().ok())
}

/// Parent pid from a /proc/<pid>/stat line (the field after the
/// parenthesised command, which may itself contain spaces and parentheses).
pub fn parse_ppid(stat: &str) -> Option<u32> {
    let after = &stat[stat.rfind(')')? + 1..];
    after.split_whitespace().nth(1)?.parse().ok()
}

/// Every process as (pid, parent pid).
pub fn process_tree() -> Vec<(u32, u32)> {
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    dir.filter_map(|e| {
        let pid: u32 = e.ok()?.file_name().to_str()?.parse().ok()?;
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        Some((pid, parse_ppid(&stat)?))
    })
    .collect()
}

/// `root` and all its descendants in `tree`.
pub fn descendants(tree: &[(u32, u32)], root: u32) -> Vec<u32> {
    let mut out = vec![root];
    let mut i = 0;
    while i < out.len() {
        let p = out[i];
        out.extend(tree.iter().filter(|(_, pp)| *pp == p).map(|(c, _)| *c));
        i += 1;
    }
    out
}

/// PSS of a process tree in kB (processes we may not read count as 0).
pub fn tree_pss(tree: &[(u32, u32)], root: u32) -> u64 {
    descendants(tree, root)
        .iter()
        .filter_map(|p| std::fs::read_to_string(format!("/proc/{p}/smaps_rollup")).ok())
        .filter_map(|t| parse_pss(&t))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meminfo_values() {
        let m = parse_meminfo(
            "MemTotal:        8048000 kB\nMemAvailable:    7012345 kB\nHugePages_Total:       0\n",
        );
        assert_eq!(m["MemTotal"], 8048000);
        assert_eq!(m["MemAvailable"], 7012345);
        assert_eq!(m["HugePages_Total"], 0);
    }

    #[test]
    fn pss_and_ppid() {
        assert_eq!(
            parse_pss("Rss:  2000 kB\nPss:  1234 kB\nPss_Anon: 10 kB\n"),
            Some(1234)
        );
        assert_eq!(parse_ppid("42 (Web Content (x)) S 7 42 42 0"), Some(7));
    }

    #[test]
    fn children_are_found() {
        let tree = [(1, 0), (10, 1), (11, 10), (12, 11), (20, 1)];
        let mut d = descendants(&tree, 10);
        d.sort();
        assert_eq!(d, vec![10, 11, 12]);
    }
}

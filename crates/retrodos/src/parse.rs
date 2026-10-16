//! Command-line parsing: pipelines, redirection, quoting, DOS switches.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Redir {
    In(String),
    Out(String),
    Append(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stage {
    /// The command word as typed (quotes removed).
    pub cmd: String,
    /// Everything after the command word, verbatim (minus redirections), with
    /// the single delimiter after the command removed.
    pub rest: String,
    pub redirs: Vec<Redir>,
}

/// How a chained command depends on the previous one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cond {
    Always,
    /// `&&`: only if the previous command succeeded.
    IfOk,
    /// `||`: only if the previous command failed.
    IfFail,
}

/// Split `a & b && c || d` (outside quotes) into conditional commands.
pub fn split_chain(line: &str) -> Vec<(Cond, String)> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut cond = Cond::Always;
    let mut quoted = false;
    let mut it = line.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '"' => {
                quoted = !quoted;
                cur.push(c);
            }
            '&' if !quoted => {
                let next = if it.peek() == Some(&'&') {
                    it.next();
                    Cond::IfOk
                } else {
                    Cond::Always
                };
                out.push((cond, std::mem::take(&mut cur)));
                cond = next;
            }
            '|' if !quoted && it.peek() == Some(&'|') => {
                it.next();
                out.push((cond, std::mem::take(&mut cur)));
                cond = Cond::IfFail;
            }
            c => cur.push(c),
        }
    }
    out.push((cond, cur));
    out.retain(|(_, s)| !s.trim().is_empty());
    out
}

/// Split a line into pipeline stages. Quotes protect `|`, `<` and `>`.
pub fn parse_line(line: &str) -> Vec<Stage> {
    let mut stages = Vec::new();
    let mut text = String::new();
    let mut redirs = Vec::new();
    let mut chars = line.chars().peekable();
    let mut quoted = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                text.push(c);
            }
            '|' if !quoted => {
                stages.push(make_stage(&text, std::mem::take(&mut redirs)));
                text.clear();
            }
            '>' | '<' if !quoted => {
                let append = c == '>' && chars.peek() == Some(&'>');
                if append {
                    chars.next();
                }
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
                let mut target = String::new();
                let mut q = false;
                while let Some(&n) = chars.peek() {
                    if n == '"' {
                        q = !q;
                        chars.next();
                        continue;
                    }
                    if !q && (n.is_whitespace() || n == '|' || n == '<' || n == '>') {
                        break;
                    }
                    target.push(n);
                    chars.next();
                }
                redirs.push(match (c, append) {
                    ('<', _) => Redir::In(target),
                    (_, true) => Redir::Append(target),
                    _ => Redir::Out(target),
                });
            }
            _ => text.push(c),
        }
    }
    stages.push(make_stage(&text, redirs));
    stages.retain(|s| !s.cmd.is_empty() || !s.redirs.is_empty());
    stages
}

fn make_stage(text: &str, redirs: Vec<Redir>) -> Stage {
    let t = text.trim_start_matches([' ', '\t', '@']);
    let t = t.trim_end_matches([' ', '\t', '\r', '\n']);
    let (cmd, rest) = first_word(t);
    Stage {
        cmd,
        rest: rest.to_string(),
        redirs,
    }
}

/// Split off the first word (honouring quotes). Returns (word without quotes, rest).
pub fn first_word(t: &str) -> (String, &str) {
    let mut word = String::new();
    let mut quoted = false;
    for (i, c) in t.char_indices() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                return (word, &t[i + c.len_utf8()..]);
            }
            c => word.push(c),
        }
    }
    (word, "")
}

/// Split argument text into words, removing quotes.
pub fn words(rest: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in rest.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                any = false;
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Arguments of a built-in: DOS switches separated from positional words.
#[derive(Clone, Debug, Default)]
pub struct Args {
    pub raw: String,
    pub words: Vec<String>,
    /// Upper-cased switch bodies without the slash: "W", "A:H", "-Y", "?".
    pub switches: Vec<String>,
    /// The arguments were glued to the command name ("CD..", "ECHO.").
    pub glued: bool,
}

impl Args {
    pub fn new(rest: &str) -> Args {
        let mut a = Args {
            raw: rest.to_string(),
            ..Default::default()
        };
        for w in words(rest) {
            // Glued switches: "/W/P", "*.TXT/W".
            let mut parts = split_glued(&w);
            if let Some(first) = parts.first() {
                if !first.starts_with('/') {
                    a.words.push(parts.remove(0));
                }
            }
            for p in parts {
                if is_switch(&p) {
                    a.switches.push(p[1..].to_ascii_uppercase());
                } else {
                    a.words.push(p);
                }
            }
        }
        a
    }

    pub fn has(&self, sw: &str) -> bool {
        self.switches
            .iter()
            .any(|s| s == sw || s.split(':').next() == Some(sw))
    }

    /// Value of "/X:VAL" (or "/XVAL" for single-letter switches).
    pub fn value(&self, sw: &str) -> Option<String> {
        self.switches.iter().find_map(|s| {
            s.strip_prefix(sw)
                .map(|v| v.trim_start_matches(':').to_string())
        })
    }

    pub fn help(&self) -> bool {
        self.has("?")
    }
}

/// "/A:H/O:N" → ["/A:H", "/O:N"]; "*.TXT/W" → ["*.TXT", "/W"]; POSIX paths stay whole.
fn split_glued(w: &str) -> Vec<String> {
    if looks_like_posix_path(w) {
        return vec![w.to_string()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in w.chars() {
        if c == '/' && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn looks_like_posix_path(w: &str) -> bool {
    // Contains a slash after position 0 with path-like neighbours: "/usr/bin", "a/b/c", "./x".
    if w.starts_with("./") || w.starts_with("../") || w.starts_with("~/") {
        return true;
    }
    if w.starts_with('/') {
        let first = w[1..].split('/').next().unwrap_or("");
        return w[1..].contains('/') || (first.len() > 1 && std::path::Path::new(w).exists());
    }
    false
}

/// Is "/X" a switch (as opposed to a POSIX path such as "/usr")?
pub fn is_switch(w: &str) -> bool {
    let Some(body) = w.strip_prefix('/') else {
        return false;
    };
    if body.is_empty() || body.len() > 8 || body.contains('/') {
        return false;
    }
    if body.len() > 1 && std::path::Path::new(w).exists() {
        return false; // "/tmp", "/etc" are paths
    }
    let mut it = body.chars();
    let first = it.next().unwrap();
    (first == '?' || first == '-' || first == '+' || first.is_ascii_alphabetic())
        && body
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '-' | '?' | '+'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_and_redirection() {
        let s = parse_line("dir /w | sort > out.txt");
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].cmd, "dir");
        assert_eq!(s[0].rest, "/w");
        assert_eq!(s[1].cmd, "sort");
        assert_eq!(s[1].redirs, vec![Redir::Out("out.txt".into())]);
        let s = parse_line("echo \"a | b\" >> \"my log.txt\"");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].rest, "\"a | b\"");
        assert_eq!(s[0].redirs, vec![Redir::Append("my log.txt".into())]);
        let s = parse_line("@echo off");
        assert_eq!(s[0].cmd, "echo");
    }

    #[test]
    fn chains() {
        let c = split_chain("cd x && dir || echo \"a && b\" & ver");
        assert_eq!(c.len(), 4);
        assert_eq!(c[1].0, Cond::IfOk);
        assert_eq!(c[2], (Cond::IfFail, " echo \"a && b\" ".to_string()));
        assert_eq!(c[3].0, Cond::Always);
    }

    #[test]
    fn switches() {
        let a = Args::new("*.TXT/W /a:h /O:-D \"My File\"");
        assert_eq!(a.words, vec!["*.TXT", "My File"]);
        assert_eq!(a.switches, vec!["W", "A:H", "O:-D"]);
        assert!(a.has("A"));
        assert_eq!(a.value("O").as_deref(), Some("-D"));
        let a = Args::new("/usr/share /?");
        assert_eq!(a.words, vec!["/usr/share"]);
        assert!(a.help());
        let a = Args::new("/etc");
        assert_eq!(a.words, vec!["/etc"]);
    }
}

//! People's accounts (Control Panel → Users): read from /etc/passwd and
//! /etc/group, changed by the `user-admin` helper after the administrator
//! has typed their password (DESIGN.md §10.6, D-046).

use crate::admin;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    pub name: String,
    /// The full name ("" when none).
    pub full: String,
    pub uid: u32,
    pub admin: bool,
}

impl User {
    /// "Jo Bloggs (jo)" or "jo".
    pub fn label(&self) -> String {
        if self.full.is_empty() {
            self.name.clone()
        } else {
            format!("{} ({})", self.full, self.name)
        }
    }
}

/// Accounts with UIDs 1000–59999 that can log on (people, not services),
/// by name.
pub fn list() -> Vec<User> {
    parse(
        &std::fs::read_to_string("/etc/passwd").unwrap_or_default(),
        &std::fs::read_to_string("/etc/group").unwrap_or_default(),
    )
}

pub fn parse(passwd: &str, group: &str) -> Vec<User> {
    let admins: Vec<&str> = group
        .lines()
        .find(|l| l.split(':').next() == Some("admin"))
        .and_then(|l| l.split(':').nth(3))
        .map(|m| m.split(',').filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    let mut v: Vec<User> = passwd
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(':').collect();
            let uid: u32 = f.get(2)?.parse().ok()?;
            // People only: services (and build accounts) cannot log on.
            let shell = f.get(6).copied().unwrap_or("");
            if !(1000..60000).contains(&uid)
                || shell.ends_with("nologin")
                || shell.ends_with("false")
            {
                return None;
            }
            let name = f[0].to_string();
            Some(User {
                admin: admins.contains(&name.as_str()),
                full: f
                    .get(4)
                    .unwrap_or(&"")
                    .split(',')
                    .next()
                    .unwrap_or("")
                    .to_string(),
                name,
                uid,
            })
        })
        .collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

/// The user running this program.
pub fn me() -> String {
    std::env::var("USER").unwrap_or_default()
}

/// A new user name: lower-case letters, digits, `-` and `_`, starting with a
/// letter or `_`, at most 32 characters, not taken.
pub fn check_new_name(name: &str, existing: &[User]) -> Result<(), String> {
    let ok_start = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c == '_');
    let ok_rest = name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if !ok_start || !ok_rest || name.len() > 32 {
        return Err("A user name is lower-case letters and digits, starting with a letter (like jo or sam2).".into());
    }
    if existing.iter().any(|u| u.name == name) || name == "root" {
        return Err(format!("There is already a user called {name}."));
    }
    Ok(())
}

fn run(args: &[&str], admin_password: &str, input: &str) -> Result<(), String> {
    admin::run_helper_authorized("user-admin", args, admin_password, input)
        .map_err(|e| e.to_string())
}

pub fn add(name: &str, full: &str, password: &str, admin_password: &str) -> Result<(), String> {
    run(&["add", name, full], admin_password, password)
}

pub fn set_password(name: &str, password: &str, admin_password: &str) -> Result<(), String> {
    run(&["password", name], admin_password, password)
}

pub fn set_admin(name: &str, on: bool, admin_password: &str) -> Result<(), String> {
    run(
        &["admin", name, if on { "yes" } else { "no" }],
        admin_password,
        "",
    )
}

/// Remove an account; its home folder stays.
pub fn remove(name: &str, admin_password: &str) -> Result<(), String> {
    run(&["remove", name], admin_password, "")
}

/// Why a new password will not do (None: fine).
pub fn password_problem(pw: &str, again: &str) -> Option<&'static str> {
    if pw.is_empty() {
        Some("Type a password.")
    } else if pw != again {
        Some("The two passwords are not the same.")
    } else if pw.contains(['\n', '\r']) {
        Some("A password cannot contain line breaks.")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn people_and_administrators() {
        let passwd = "root:x:0:0:root:/root:/bin/bash\nben:x:1000:1000:Ben,,,:/home/ben:/bin/bash\njo:x:1001:1001::/home/jo:/bin/bash\nnixbld1:x:30001:30000:Nix:/var/empty:/sbin/nologin\nnobody:x:65534:65534::/:/usr/sbin/nologin\n";
        let group = "sudo:x:27:ben\nadmin:x:990:ben\n";
        let u = parse(passwd, group);
        assert_eq!(u.len(), 2);
        assert!(u[0].admin && !u[1].admin);
        assert_eq!(u[0].label(), "Ben (ben)");
        assert_eq!(u[1].label(), "jo");
        assert!(check_new_name("sam2", &u).is_ok());
        assert!(check_new_name("jo", &u).is_err());
        assert!(check_new_name("2sam", &u).is_err());
        assert!(check_new_name("Sam", &u).is_err());
        assert_eq!(
            password_problem("a", "b"),
            Some("The two passwords are not the same.")
        );
        assert_eq!(password_problem("abc", "abc"), None);
    }
}

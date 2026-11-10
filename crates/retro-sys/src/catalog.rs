//! The Software Library's catalog (DESIGN.md §10.5, D-055): the curated
//! programs in `catalog/*.toml`, built into the program so the text and
//! Desktop libraries always agree.

use std::sync::OnceLock;

/// A program in the catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    pub name: String,
    pub category: &'static str,
    /// Debian packages; the first is "the" package.
    pub packages: Vec<String>,
    /// More names for INSTALL and RetroDOS (upper case).
    pub alias: Vec<String>,
    /// The command that starts it.
    pub run: String,
    pub blurb: String,
    /// Shown once it is installed.
    pub note: Option<String>,
}

/// A category: its name and its icon (retro_gfx::iconset).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Category {
    pub name: &'static str,
    pub icon: &'static str,
}

const FILES: &[&str] = &[
    include_str!("../../../catalog/internet.toml"),
    include_str!("../../../catalog/office.toml"),
    include_str!("../../../catalog/graphics.toml"),
    include_str!("../../../catalog/sound-video.toml"),
    include_str!("../../../catalog/games.toml"),
    include_str!("../../../catalog/emulators.toml"),
    include_str!("../../../catalog/programming.toml"),
    include_str!("../../../catalog/utilities.toml"),
    include_str!("../../../catalog/education.toml"),
];

pub struct Catalog {
    pub categories: Vec<Category>,
    pub apps: Vec<App>,
}

fn leak(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

/// Parse one catalog file.
pub fn parse(text: &str) -> Result<(Category, Vec<App>), String> {
    let doc: toml_edit::Document<String> = text.parse().map_err(|e| format!("{e}"))?;
    let s = |t: &toml_edit::Item, k: &str| t.get(k).and_then(|v| v.as_str()).map(String::from);
    let root = doc.as_item();
    let category = leak(&s(root, "category").ok_or("no category")?);
    let icon = leak(&s(root, "icon").unwrap_or_default());
    let mut apps = Vec::new();
    if let Some(list) = root.get("app").and_then(|a| a.as_array_of_tables()) {
        for t in list.iter() {
            let item = toml_edit::Item::Table(t.clone());
            let strings = |k: &str| -> Vec<String> {
                item.get(k)
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let name = s(&item, "name").ok_or("an app without a name")?;
            let packages = strings("packages");
            if packages.is_empty() {
                return Err(format!("{name}: no packages"));
            }
            apps.push(App {
                category,
                packages,
                alias: strings("alias")
                    .into_iter()
                    .map(|a| a.to_ascii_uppercase())
                    .collect(),
                run: s(&item, "run").unwrap_or_default(),
                blurb: s(&item, "blurb").unwrap_or_default(),
                note: s(&item, "note"),
                name,
            });
        }
    }
    Ok((
        Category {
            name: category,
            icon,
        },
        apps,
    ))
}

/// The whole catalog.
pub fn catalog() -> &'static Catalog {
    static C: OnceLock<Catalog> = OnceLock::new();
    C.get_or_init(|| {
        let mut c = Catalog {
            categories: Vec::new(),
            apps: Vec::new(),
        };
        for f in FILES {
            let (cat, apps) = parse(f).expect("catalog/*.toml are valid (unit test)");
            c.categories.push(cat);
            c.apps.extend(apps);
        }
        c
    })
}

impl Catalog {
    pub fn in_category(&self, cat: &str) -> Vec<&App> {
        self.apps.iter().filter(|a| a.category == cat).collect()
    }

    /// A program by what someone typed: its name (spaces optional), an
    /// alias, or its first package; any case.
    pub fn find(&self, typed: &str) -> Option<&App> {
        let t = typed.trim().to_ascii_lowercase();
        let squash = |s: &str| s.to_ascii_lowercase().replace([' ', '-'], "");
        self.apps.iter().find(|a| {
            a.name.to_ascii_lowercase() == t
                || squash(&a.name) == squash(&t)
                || a.alias.iter().any(|x| x.eq_ignore_ascii_case(&t))
                || a.packages[0] == t
        })
    }

    /// Programs whose name, alias, blurb or package mentions `word`.
    pub fn search(&self, word: &str) -> Vec<&App> {
        let w = word.trim().to_ascii_lowercase();
        if w.is_empty() {
            return self.apps.iter().collect();
        }
        self.apps
            .iter()
            .filter(|a| {
                a.name.to_ascii_lowercase().contains(&w)
                    || a.blurb.to_ascii_lowercase().contains(&w)
                    || a.alias.iter().any(|x| x.to_ascii_lowercase().contains(&w))
                    || a.packages.iter().any(|p| p.contains(&w))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_parses() {
        let c = catalog();
        assert_eq!(c.categories.len(), 9);
        assert!(c.apps.len() >= 60, "{} programs", c.apps.len());
        for a in &c.apps {
            assert!(!a.blurb.is_empty() && !a.run.is_empty(), "{}", a.name);
            assert!(
                a.blurb.len() <= 72,
                "{}: blurb too long for LIBRARY",
                a.name
            );
        }
        // Names and aliases are unique, so INSTALL <name> is unambiguous.
        let mut seen = std::collections::HashSet::new();
        for a in &c.apps {
            for n in std::iter::once(a.name.to_ascii_uppercase()).chain(a.alias.clone()) {
                assert!(seen.insert(n.clone()), "{n} is used twice");
            }
        }
    }

    #[test]
    fn finding_programs() {
        let c = catalog();
        assert_eq!(c.find("chrome").unwrap().name, "Chromium");
        assert_eq!(c.find("vscode").unwrap().packages[0], "code");
        assert_eq!(
            c.find("libreofficewriter").unwrap().name,
            "LibreOffice Writer"
        );
        assert_eq!(c.find("dosbox-x").unwrap().name, "DOSBox-X");
        assert!(c.find("nonsense").is_none());
        assert!(c.search("pixel").iter().any(|a| a.name == "GrafX2"));
    }
}

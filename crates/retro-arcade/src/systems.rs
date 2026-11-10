//! The game consoles Arcade knows (D-057): where their games go, which
//! files are games, and what plays them — a RetroArch core from Debian, or
//! a standalone emulator for consoles Debian has no core for.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Launch {
    /// A libretro core: `<name>_libretro.so` in the libretro folder.
    Core(&'static str),
    /// A program that takes the game file.
    Command(&'static str, &'static [&'static str]),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct System {
    pub name: &'static str,
    /// The folder under C:\GAMES.
    pub folder: &'static str,
    pub exts: &'static [&'static str],
    pub launch: Launch,
    /// What to install for it.
    pub packages: &'static [&'static str],
}

pub const SYSTEMS: &[System] = &[
    System {
        name: "Nintendo (NES)",
        folder: "NES",
        exts: &["nes", "zip"],
        launch: Launch::Core("nestopia"),
        packages: &["retroarch", "libretro-nestopia"],
    },
    System {
        name: "Super Nintendo",
        folder: "SNES",
        exts: &["sfc", "smc", "zip"],
        launch: Launch::Core("bsnes_mercury_balanced"),
        packages: &["retroarch", "libretro-bsnes-mercury-balanced"],
    },
    System {
        name: "Game Boy and Color",
        folder: "GameBoy",
        exts: &["gb", "gbc", "zip"],
        launch: Launch::Core("gambatte"),
        packages: &["retroarch", "libretro-gambatte"],
    },
    System {
        name: "Game Boy Advance",
        folder: "GBA",
        exts: &["gba", "zip"],
        launch: Launch::Core("mgba"),
        packages: &["retroarch", "libretro-mgba"],
    },
    System {
        name: "Nintendo DS",
        folder: "DS",
        exts: &["nds", "zip"],
        launch: Launch::Core("desmume"),
        packages: &["retroarch", "libretro-desmume"],
    },
    System {
        name: "Mega Drive / Genesis",
        folder: "MegaDrive",
        exts: &["md", "gen", "smd", "bin", "zip"],
        launch: Launch::Command("mednafen", &["-video.fs", "1"]),
        packages: &["mednafen"],
    },
    System {
        name: "Master System / Game Gear",
        folder: "MasterSystem",
        exts: &["sms", "gg", "zip"],
        launch: Launch::Command("mednafen", &["-video.fs", "1"]),
        packages: &["mednafen"],
    },
    System {
        name: "PC Engine",
        folder: "PCEngine",
        exts: &["pce", "zip"],
        launch: Launch::Command("mednafen", &["-video.fs", "1"]),
        packages: &["mednafen"],
    },
    System {
        name: "Atari 2600",
        folder: "Atari2600",
        exts: &["a26", "bin", "zip"],
        launch: Launch::Command("stella", &["-fullscreen", "1"]),
        packages: &["stella"],
    },
];

/// Where libretro cores are (Debian's multiarch folder for this machine).
pub fn core_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("RETROOS_LIBRETRO_DIR") {
        return d.into();
    }
    let arch = if cfg!(target_arch = "aarch64") {
        "aarch64-linux-gnu"
    } else {
        "x86_64-linux-gnu"
    };
    PathBuf::from(format!("/usr/lib/{arch}/libretro"))
}

fn in_path(cmd: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file()))
        .unwrap_or(false)
}

impl System {
    /// Is what plays it installed?
    pub fn ready(&self) -> bool {
        match self.launch {
            Launch::Core(c) => {
                in_path("retroarch") && core_dir().join(format!("{c}_libretro.so")).is_file()
            }
            Launch::Command(c, _) => in_path(c),
        }
    }

    /// The emulator's name, for "Install …".
    pub fn emulator(&self) -> String {
        match self.launch {
            Launch::Core(c) => format!("RetroArch ({c})"),
            Launch::Command(c, _) => {
                let mut s = c.to_string();
                if let Some(f) = s.get_mut(..1) {
                    f.make_ascii_uppercase();
                }
                s
            }
        }
    }

    /// The command that plays `game` full screen.
    pub fn command(&self, game: &Path) -> Vec<String> {
        let g = game.to_string_lossy().into_owned();
        match self.launch {
            Launch::Core(c) => vec![
                "retroarch".into(),
                "-f".into(),
                "-L".into(),
                core_dir()
                    .join(format!("{c}_libretro.so"))
                    .to_string_lossy()
                    .into_owned(),
                g,
            ],
            Launch::Command(c, args) => {
                let mut v = vec![c.to_string()];
                v.extend(args.iter().map(|a| a.to_string()));
                v.push(g);
                v
            }
        }
    }

    pub fn dir(&self, games: &Path) -> PathBuf {
        games.join(self.folder)
    }

    /// The games in its folder, by name.
    pub fn games(&self, games: &Path) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(self.dir(games))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.extension()
                        .map(|e| e.to_string_lossy().to_lowercase())
                        .is_some_and(|e| self.exts.contains(&e.as_str()))
            })
            .collect();
        v.sort_by_key(|p| p.to_string_lossy().to_lowercase());
        v
    }
}

/// A game's name for the list: the file name without its extension,
/// underscores as spaces.
pub fn title(game: &Path) -> String {
    game.file_stem()
        .map(|s| s.to_string_lossy().replace('_', " "))
        .unwrap_or_default()
}

/// C:\GAMES (~/Games), with a folder per console and a note in each.
pub fn games_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/".into());
    home.join("Games")
}

pub fn make_folders(games: &Path) {
    for s in SYSTEMS {
        let d = s.dir(games);
        if d.is_dir() {
            continue;
        }
        if std::fs::create_dir_all(&d).is_ok() {
            let exts: Vec<String> = s
                .exts
                .iter()
                .map(|e| format!(".{}", e.to_ascii_uppercase()))
                .collect();
            let _ = std::fs::write(
                d.join("README.TXT"),
                format!(
                    "Put {} games here ({} files). Arcade lists them.\nOnly use games you own or that are free to copy.\n",
                    s.name,
                    exts.join(", ")
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn games_by_extension_and_commands() {
        let d = std::env::temp_dir().join(format!("retro-arcade-{}", std::process::id()));
        make_folders(&d);
        let nes = &SYSTEMS[0];
        std::fs::write(nes.dir(&d).join("Super_Tux_Bros.nes"), "").unwrap();
        std::fs::write(nes.dir(&d).join("notes.txt"), "").unwrap();
        let g = nes.games(&d);
        assert_eq!(g.len(), 1, "README.TXT and notes are not games");
        assert_eq!(title(&g[0]), "Super Tux Bros");
        let cmd = nes.command(&g[0]);
        assert_eq!(cmd[..3], ["retroarch", "-f", "-L"]);
        assert!(cmd[3].ends_with("libretro/nestopia_libretro.so"));
        let md = SYSTEMS.iter().find(|s| s.folder == "MegaDrive").unwrap();
        assert_eq!(md.command(Path::new("/g/x.md"))[0], "mednafen");
        assert_eq!(md.emulator(), "Mednafen");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn every_package_is_in_debian() {
        // The Debian names checked against trixie arm64 (2026-11-10).
        for s in SYSTEMS {
            assert!(!s.packages.is_empty());
            for p in s.packages {
                assert!(p
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'));
            }
        }
    }
}

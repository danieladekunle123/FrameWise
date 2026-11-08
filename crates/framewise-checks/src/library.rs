//! Installed-game detection for profiles: Steam, Epic Games, Battle.net and the Xbox app (Game
//! Pass / Microsoft Store). Read-only; nothing is launched or contacted.

use std::path::{Path, PathBuf};

use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};
use winreg::RegKey;

use crate::profiles::{parse_gaming_root, vdf_values, Game, GameSource};

/// Every game found, sorted by name. Each library is read independently, so one broken library
/// doesn't hide the others.
pub fn detect_games() -> Vec<Game> {
    let mut games = Vec::new();
    games.extend(steam());
    games.extend(epic());
    games.extend(battle_net());
    games.extend(xbox());
    games.extend(roblox());
    games.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    games.dedup_by(|a, b| a.id == b.id);
    games
}

fn steam() -> Vec<Game> {
    let mut out = Vec::new();
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(path) = hkcu.open_subkey(r"Software\Valve\Steam").and_then(|k| k.get_value::<String, _>("SteamPath")) else {
        return out;
    };
    let steam = PathBuf::from(path.replace('/', "\\"));
    let mut libraries = vec![steam.clone()];
    for vdf in [steam.join(r"steamapps\libraryfolders.vdf"), steam.join(r"config\libraryfolders.vdf")] {
        if let Ok(c) = std::fs::read_to_string(&vdf) {
            libraries.extend(vdf_values(&c, "path").into_iter().map(PathBuf::from));
        }
    }
    libraries.sort();
    libraries.dedup_by(|a, b| a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy()));
    for lib in libraries {
        let apps = lib.join("steamapps");
        let Ok(rd) = std::fs::read_dir(&apps) else { continue };
        for e in rd.flatten() {
            let f = e.file_name().to_string_lossy().to_string();
            if !(f.starts_with("appmanifest_") && f.ends_with(".acf")) {
                continue;
            }
            let Ok(c) = std::fs::read_to_string(e.path()) else { continue };
            let first = |k: &str| vdf_values(&c, k).into_iter().next().unwrap_or_default();
            let (appid, name, dir) = (first("appid"), first("name"), first("installdir"));
            // Tools and redistributables aren't games.
            if appid.is_empty() || name.is_empty() || dir.is_empty() || name.contains("Redistributables") || name.starts_with("Proton ") || name.contains("Steamworks") {
                continue;
            }
            let install = apps.join("common").join(&dir);
            if !install.is_dir() {
                continue;
            }
            out.push(Game { id: format!("steam:{appid}"), name, source: GameSource::Steam, install_dir: Some(path_string(&install)), exe: None });
        }
    }
    out
}

fn epic() -> Vec<Game> {
    let mut out = Vec::new();
    let Some(pd) = std::env::var_os("ProgramData") else { return out };
    let Ok(rd) = std::fs::read_dir(PathBuf::from(pd).join(r"Epic\EpicGamesLauncher\Data\Manifests")) else { return out };
    for e in rd.flatten() {
        if e.path().extension().and_then(|x| x.to_str()) != Some("item") {
            continue;
        }
        let Ok(c) = std::fs::read_to_string(e.path()) else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&c) else { continue };
        let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
        let (name, dir, app, exe) = (s("DisplayName"), s("InstallLocation"), s("AppName"), s("LaunchExecutable"));
        let is_game = v["AppCategories"].as_array().map(|a| a.iter().any(|x| x.as_str() == Some("games"))).unwrap_or(true);
        if name.is_empty() || dir.is_empty() || app.is_empty() || !is_game || !Path::new(&dir).is_dir() {
            continue;
        }
        let exe = (!exe.is_empty()).then(|| path_string(&Path::new(&dir).join(&exe)));
        out.push(Game { id: format!("epic:{app}"), name, source: GameSource::Epic, install_dir: Some(dir), exe });
    }
    out
}

/// Battle.net games register in Windows' installed-programs list with Blizzard as publisher.
fn battle_net() -> Vec<Game> {
    let mut out = Vec::new();
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    for view in [KEY_WOW64_32KEY, KEY_WOW64_64KEY] {
        let Ok(root) = hklm.open_subkey_with_flags(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall", KEY_READ | view) else { continue };
        for key in root.enum_keys().flatten() {
            let Ok(k) = root.open_subkey(&key) else { continue };
            let get = |n: &str| k.get_value::<String, _>(n).unwrap_or_default();
            let (publisher, name, dir) = (get("Publisher"), get("DisplayName"), get("InstallLocation"));
            if !publisher.to_lowercase().contains("blizzard") || name.is_empty() || dir.is_empty() || name.eq_ignore_ascii_case("Battle.net") {
                continue;
            }
            let dir = dir.trim_matches('"').to_string();
            if !Path::new(&dir).is_dir() {
                continue;
            }
            let folder = Path::new(&dir).file_name().map(|f| f.to_string_lossy().to_lowercase()).unwrap_or_default();
            out.push(Game { id: format!("bnet:{folder}"), name, source: GameSource::BattleNet, install_dir: Some(dir), exe: None });
        }
    }
    out
}

/// Xbox app games live in a folder per drive, named in that drive's `.GamingRoot` file
/// (`XboxGames` by default). Each game has a `Content` folder with its executable.
fn xbox() -> Vec<Game> {
    let mut out = Vec::new();
    for letter in b'C'..=b'Z' {
        let root = format!("{}:\\", letter as char);
        let Ok(bytes) = std::fs::read(format!("{root}.GamingRoot")) else { continue };
        for folder in parse_gaming_root(&bytes) {
            let Ok(rd) = std::fs::read_dir(Path::new(&root).join(&folder)) else { continue };
            for e in rd.flatten() {
                let game_dir = e.path();
                let content = game_dir.join("Content");
                if !content.is_dir() {
                    continue;
                }
                let name = e.file_name().to_string_lossy().to_string();
                out.push(Game {
                    id: format!("xbox:{}", name.to_lowercase()),
                    name: name.replace(['-', '_'], " "),
                    source: GameSource::Xbox,
                    install_dir: Some(path_string(&content)),
                    exe: None,
                });
            }
        }
    }
    out
}

/// Roblox installs per user into Versions\version-<hash>, a folder that changes with every update,
/// so the profile matches the whole Versions folder (Studio and the crash handler are excluded in
/// profiles::matches).
fn roblox() -> Vec<Game> {
    let Some(local) = std::env::var_os("LOCALAPPDATA") else { return Vec::new() };
    let versions = PathBuf::from(local).join(r"Roblox\Versions");
    let has_player = std::fs::read_dir(&versions)
        .map(|rd| rd.flatten().any(|e| e.path().join("RobloxPlayerBeta.exe").is_file()))
        .unwrap_or(false);
    if !has_player {
        return Vec::new();
    }
    vec![Game { id: "roblox:player".into(), name: "Roblox".into(), source: GameSource::Roblox, install_dir: Some(path_string(&versions)), exe: None }]
}

fn path_string(p: &Path) -> String {
    p.to_string_lossy().to_string()
}

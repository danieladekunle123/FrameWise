//! Game profiles (phase 4): what a profile changes, which processes belong to a game, which apps a
//! profile may pause or close, and the state saved so every change can be put back, even after a
//! crash. Pure code; the watcher in the app core does the Windows work.
//!
//! A profile never touches the game itself or anything Windows needs, and never changes security
//! settings. Every action is temporary: it's reverted when the game exits, or when the watcher next
//! starts if it crashed or the PC restarted mid-game.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum GameSource {
    Steam,
    Epic,
    BattleNet,
    Xbox,
    Roblox,
    Manual,
}

impl GameSource {
    pub fn label(self) -> &'static str {
        match self {
            GameSource::Steam => "Steam",
            GameSource::Epic => "Epic Games",
            GameSource::BattleNet => "Battle.net",
            GameSource::Xbox => "Xbox app",
            GameSource::Roblox => "Roblox",
            GameSource::Manual => "Added by you",
        }
    }
}

/// A game FrameWise knows about. `id` is stable across scans (`steam:730`, `epic:<AppName>`,
/// `bnet:<folder>`, `xbox:<folder>`, `manual:<exe path>`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Game {
    pub id: String,
    pub name: String,
    pub source: GameSource,
    /// Install folder: any process started from inside it counts as the game (launchers often start
    /// a different exe than the one in the shortcut).
    pub install_dir: Option<String>,
    /// The main executable, when the library says (Epic, manual).
    pub exe: Option<String>,
}

/// Executables inside game folders that aren't the game: crash reporters, installers, anti-cheat
/// services. They neither start a profile nor keep one running.
const NOT_THE_GAME: &[&str] = &[
    "unitycrashhandler64.exe",
    "robloxcrashhandler.exe",
    "robloxstudiobeta.exe",
    "robloxstudiolauncherbeta.exe",
    "unitycrashhandler32.exe",
    "crashreportclient.exe",
    "crashpad_handler.exe",
    "easyanticheat.exe",
    "easyanticheat_setup.exe",
    "easyanticheat_eos_setup.exe",
    "beservice.exe",
    "beservice_x64.exe",
    "dxsetup.exe",
    "vc_redist.x64.exe",
    "vc_redist.x86.exe",
    "vcredist_x64.exe",
    "vcredist_x86.exe",
    "uninstall.exe",
    "unins000.exe",
];

fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// Whether a running process (full image path) belongs to this game.
pub fn matches(game: &Game, exe_path: &str) -> bool {
    let p = exe_path.replace('/', "\\").to_lowercase();
    if NOT_THE_GAME.contains(&file_name(&p)) {
        return false;
    }
    if let Some(exe) = &game.exe {
        if p == exe.replace('/', "\\").to_lowercase() {
            return true;
        }
    }
    if let Some(dir) = &game.install_dir {
        let mut d = dir.replace('/', "\\").to_lowercase();
        if d.len() < 4 {
            return false; // never a drive root
        }
        if !d.ends_with('\\') {
            d.push('\\');
        }
        return p.starts_with(&d);
    }
    false
}

/// The changes a profile makes while its game runs.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct ProfileActions {
    /// Power plan GUID to switch to (None = leave as is).
    #[serde(default)]
    pub power_plan: Option<String>,
    /// Executable names (lower case) to pause: suspended, so they use no CPU, and resumed after.
    #[serde(default)]
    pub pause: Vec<String>,
    /// Executable names (lower case) to ask to close, and reopen afterwards.
    #[serde(default)]
    pub close: Vec<String>,
}

impl ProfileActions {
    pub fn is_empty(&self) -> bool {
        self.power_plan.is_none() && self.pause.is_empty() && self.close.is_empty()
    }

    /// Drops anything a profile must never touch and normalises names.
    pub fn sanitised(&self) -> ProfileActions {
        let clean = |v: &[String]| {
            let mut out: Vec<String> = v.iter().map(|s| s.trim().to_lowercase()).filter(|s| may_touch(s)).collect();
            out.sort();
            out.dedup();
            out
        };
        let close = clean(&self.close);
        // An app is either paused or closed, not both.
        let pause = clean(&self.pause).into_iter().filter(|p| !close.contains(p)).collect();
        ProfileActions {
            power_plan: self.power_plan.clone().filter(|g| crate::tweak::is_known_scheme_or_guid(g)),
            pause,
            close,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Profile {
    pub game_id: String,
    pub enabled: bool,
    pub actions: ProfileActions,
}

/// Processes a profile must never pause or close: Windows itself, security software, anti-cheat,
/// FrameWise, and shared components other apps rely on.
const NEVER_TOUCH: &[&str] = &[
    "system",
    "registry",
    "smss.exe",
    "csrss.exe",
    "wininit.exe",
    "winlogon.exe",
    "services.exe",
    "lsass.exe",
    "lsaiso.exe",
    "svchost.exe",
    "dwm.exe",
    "explorer.exe",
    "sihost.exe",
    "fontdrvhost.exe",
    "ctfmon.exe",
    "audiodg.exe",
    "conhost.exe",
    "dllhost.exe",
    "taskhostw.exe",
    "runtimebroker.exe",
    "searchhost.exe",
    "searchindexer.exe",
    "startmenuexperiencehost.exe",
    "shellexperiencehost.exe",
    "textinputhost.exe",
    "applicationframehost.exe",
    "securityhealthsystray.exe",
    "securityhealthservice.exe",
    "msmpeng.exe",
    "nissrv.exe",
    "smartscreen.exe",
    "framewise.exe",
    "msedgewebview2.exe",
    "presentmon.exe",
    "vgc.exe",
    "vgtray.exe",
    "easyanticheat.exe",
    "beservice.exe",
    "beservice_x64.exe",
    "faceit.exe",
    "faceitservice.exe",
    "gameguard.exe",
    "nvcontainer.exe",
    "nvdisplay.container.exe",
    "rtkauduservice64.exe",
];

/// Whether a profile may pause or close this executable (file name, any case).
pub fn may_touch(exe_name: &str) -> bool {
    let n = file_name(exe_name).to_lowercase();
    !n.is_empty()
        && n.ends_with(".exe")
        && !NEVER_TOUCH.contains(&n.as_str())
        && !n.starts_with("easyanticheat")
        && !n.starts_with("battleye")
        && !n.contains("anticheat")
}

/// A paused process, identified by PID *and* start time so a reused PID is never resumed by mistake.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PausedProcess {
    pub pid: u32,
    pub exe: String,
    /// Process creation time as a FILETIME.
    pub created: u64,
}

/// A process closed by a profile, to reopen when the game exits.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ClosedApp {
    pub exe_path: String,
    pub command_line: String,
}

/// Everything a session changed, saved *before* changing it.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct SavedState {
    pub game_name: String,
    /// The power plan before the profile switched it, and the one it switched to.
    pub power_plan_before: Option<String>,
    pub power_plan_set: Option<String>,
    pub paused: Vec<PausedProcess>,
    pub closed: Vec<ClosedApp>,
    /// Apps the profile asked to close that kept running (minimised to the tray, for example).
    #[serde(default)]
    pub not_closed: Vec<String>,
}

/// Whether to put the power plan back: only if it's still the one the profile set. If the user
/// changed it during the game, their choice stays.
pub fn should_restore_plan(state: &SavedState, current: Option<&str>) -> bool {
    match (&state.power_plan_before, &state.power_plan_set, current) {
        (Some(before), Some(set), Some(now)) => now.eq_ignore_ascii_case(set) && !now.eq_ignore_ascii_case(before),
        _ => false,
    }
}

// ---------------------------------------------------------------------------------------------
// Library file formats

/// Values for a key in a Valve KeyValues (VDF/ACF) file, unescaped.
pub fn vdf_values(content: &str, key: &str) -> Vec<String> {
    content
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.trim().split('"').collect();
            // ["", key, "\t\t", value, ""]
            if parts.len() >= 5 && parts[1].eq_ignore_ascii_case(key) {
                Some(parts[3].replace("\\\\", "\\"))
            } else {
                None
            }
        })
        .collect()
}

/// The Xbox app's games folder from a drive's `.GamingRoot` file: the 4-byte "RGBX" signature, a
/// 4-byte count, then UTF-16 folder paths relative to the drive root, NUL-separated.
pub fn parse_gaming_root(bytes: &[u8]) -> Vec<String> {
    if bytes.len() < 10 || &bytes[..4] != b"RGBX" {
        return Vec::new();
    }
    let words: Vec<u16> = bytes[8..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    words
        .split(|&w| w == 0)
        .map(String::from_utf16_lossy)
        .map(|s| s.trim().trim_start_matches('\\').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(dir: Option<&str>, exe: Option<&str>) -> Game {
        Game {
            id: "g".into(),
            name: "G".into(),
            source: GameSource::Steam,
            install_dir: dir.map(String::from),
            exe: exe.map(String::from),
        }
    }

    #[test]
    fn matches_by_folder_or_exe() {
        let g = game(Some(r"D:\SteamLibrary\steamapps\common\Cyberpunk 2077"), None);
        assert!(matches(&g, r"D:\SteamLibrary\steamapps\common\Cyberpunk 2077\bin\x64\Cyberpunk2077.exe"));
        assert!(matches(&g, r"d:/steamlibrary/steamapps/common/cyberpunk 2077/REDprelauncher.exe"));
        // A sibling folder with the same prefix isn't the game.
        assert!(!matches(&g, r"D:\SteamLibrary\steamapps\common\Cyberpunk 2077 Mods\tool.exe"));
        // Crash reporters and anti-cheat inside the folder don't count.
        assert!(!matches(&g, r"D:\SteamLibrary\steamapps\common\Cyberpunk 2077\bin\x64\CrashReportClient.exe"));
        let m = game(None, Some(r"C:\Games\Thing\thing.exe"));
        assert!(matches(&m, r"c:\games\thing\THING.EXE"));
        assert!(!matches(&m, r"C:\Games\Thing\other.exe"));
        // A drive root is never an install folder.
        assert!(!matches(&game(Some("C:\\"), None), r"C:\Windows\explorer.exe"));
    }

    #[test]
    fn never_touches_windows_security_or_anticheat() {
        for n in ["explorer.exe", "SVCHOST.EXE", "MsMpEng.exe", "vgc.exe", "EasyAntiCheat_EOS.exe", "BattlEye_x64.exe", "FrameWise.exe", "something"] {
            assert!(!may_touch(n), "{n}");
        }
        for n in ["Discord.exe", "steamwebhelper.exe", r"C:\Program Files\OBS\obs64.exe", "lghub.exe"] {
            assert!(may_touch(n), "{n}");
        }
        let a = ProfileActions {
            power_plan: Some("8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c".into()),
            pause: vec!["Discord.exe".into(), "explorer.exe".into(), "lghub.exe".into()],
            close: vec!["LGHUB.exe".into(), "svchost.exe".into()],
        }
        .sanitised();
        assert_eq!(a.pause, vec!["discord.exe"]);
        assert_eq!(a.close, vec!["lghub.exe"]);
        assert!(a.power_plan.is_some());
        assert!(ProfileActions { power_plan: Some("not a guid".into()), ..Default::default() }.sanitised().power_plan.is_none());
    }

    #[test]
    fn plan_restore_respects_the_users_own_change() {
        let s = SavedState { power_plan_before: Some("balanced".into()), power_plan_set: Some("high".into()), ..Default::default() };
        assert!(should_restore_plan(&s, Some("HIGH")));
        assert!(!should_restore_plan(&s, Some("ultimate")), "user switched during the game: keep theirs");
        assert!(!should_restore_plan(&s, Some("balanced")), "already back");
        assert!(!should_restore_plan(&SavedState::default(), Some("high")));
    }

    #[test]
    fn gaming_root_file() {
        let mut b = b"RGBX".to_vec();
        b.extend(1u32.to_le_bytes());
        for w in "\\XboxGames".encode_utf16().chain(std::iter::once(0)) {
            b.extend(w.to_le_bytes());
        }
        assert_eq!(parse_gaming_root(&b), vec!["XboxGames"]);
        assert!(parse_gaming_root(b"nope").is_empty());
    }

    #[test]
    fn acf_values() {
        let acf = "\"AppState\"\n{\n\t\"appid\"\t\t\"730\"\n\t\"name\"\t\t\"Counter-Strike 2\"\n\t\"installdir\"\t\t\"Counter-Strike Global Offensive\"\n}";
        assert_eq!(vdf_values(acf, "appid"), vec!["730"]);
        assert_eq!(vdf_values(acf, "installdir"), vec!["Counter-Strike Global Offensive"]);
    }
}

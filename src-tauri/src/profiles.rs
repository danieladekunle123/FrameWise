//! Game profile commands for the interface (phase 4). The watcher (`watcher.rs`) applies the
//! profiles; these commands only read and save them and start, pause or stop the watcher.

use framewise_checks::evaluate::KNOWN_OVERLAYS;
use framewise_checks::profiles::{self, Game, GameSource, Profile, ProfileActions};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::watcher::{self, WatcherStatus};
use crate::{db, with_db, AppState, CmdResult};

#[derive(Serialize)]
pub struct GameRow {
    pub game: Game,
    pub source_label: &'static str,
    pub profile: Option<Profile>,
}

#[derive(Serialize)]
pub struct GamesList {
    pub games: Vec<GameRow>,
    /// Power plans on this PC: (GUID, name).
    pub plans: Vec<(String, String)>,
    pub current_plan: Option<String>,
}

#[tauri::command]
pub async fn games_list(state: State<'_, AppState>) -> CmdResult<GamesList> {
    let detected = tauri::async_runtime::spawn_blocking(framewise_checks::library::detect_games).await.map_err(|e| e.to_string())?;
    let manual = with_db(&state, |c| db::manual_games(c))?;
    let saved = with_db(&state, |c| db::profiles(c))?;
    let mut games: Vec<Game> = detected;
    games.extend(manual);
    // Profiles whose game is no longer detected (uninstalled) still show, so they can be removed.
    for (g, _) in &saved {
        if !games.iter().any(|x| x.id == g.id) {
            games.push(g.clone());
        }
    }
    let rows = games
        .into_iter()
        .map(|g| {
            let profile = saved.iter().find(|(x, _)| x.id == g.id).map(|(_, p)| p.clone());
            GameRow { source_label: g.source.label(), game: g, profile }
        })
        .collect();
    let current_plan = match framewise_checks::system::read(&framewise_checks::tweak::Tweak::PowerScheme) {
        Ok(framewise_checks::tweak::TweakValue::Text(s)) => Some(s),
        _ => None,
    };
    Ok(GamesList { games: rows, plans: framewise_checks::system::power_plans(), current_plan })
}

#[tauri::command]
pub fn game_add(state: State<'_, AppState>, path: String) -> CmdResult<Game> {
    let p = std::path::Path::new(path.trim().trim_matches('"'));
    if !p.is_file() || !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")) {
        return Err("Pick the game's .exe file.".into());
    }
    let full = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let full = full.to_string_lossy().trim_start_matches(r"\\?\").to_string();
    if !profiles::may_touch(&full) {
        return Err("That's part of Windows or security software, not a game.".into());
    }
    let name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Game".into());
    let g = Game { id: format!("manual:{}", full.to_lowercase()), name, source: GameSource::Manual, install_dir: None, exe: Some(full) };
    with_db(&state, |c| db::add_manual_game(c, &g))?;
    Ok(g)
}

#[tauri::command]
pub fn game_remove(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    with_db(&state, |c| db::remove_manual_game(c, &id))?;
    watcher::reload();
    Ok(())
}

#[tauri::command]
pub fn profile_save(state: State<'_, AppState>, game: Game, enabled: bool, actions: ProfileActions) -> CmdResult<Profile> {
    let p = Profile { game_id: game.id.clone(), enabled, actions: actions.sanitised() };
    with_db(&state, |c| db::save_profile(c, &game, &p))?;
    watcher::reload();
    Ok(p)
}

#[derive(Serialize)]
pub struct RunningApp {
    /// Lower-case exe name, as profiles store it.
    pub exe: String,
    pub path: Option<String>,
    /// A friendly name for known overlays and launchers.
    pub label: Option<String>,
    pub known_overlay: bool,
}

/// Apps running now that a profile may pause or close, known overlays and launchers first.
#[tauri::command]
pub async fn running_apps() -> CmdResult<Vec<RunningApp>> {
    tauri::async_runtime::spawn_blocking(|| {
        let me = crate::procctl::session_of(std::process::id());
        let mut out: Vec<RunningApp> = Vec::new();
        for (pid, exe) in crate::procctl::list() {
            let name = exe.to_lowercase();
            if !profiles::may_touch(&name) || out.iter().any(|a| a.exe == name) || crate::procctl::session_of(pid) != me {
                continue;
            }
            let Some(path) = crate::procctl::image_path(pid) else { continue };
            // Windows' own folder holds system processes, never "apps" to pause.
            if path.to_lowercase().starts_with(r"c:\windows\") {
                continue;
            }
            let known = KNOWN_OVERLAYS.iter().find(|(e, _)| *e == name);
            out.push(RunningApp { exe: name, path: Some(path), label: known.map(|(_, l)| l.to_string()), known_overlay: known.is_some() });
        }
        out.sort_by(|a, b| b.known_overlay.cmp(&a.known_overlay).then(a.exe.cmp(&b.exe)));
        out
    })
    .await
    .map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct WatcherInfo {
    /// Profiles are on (the watcher should run and start with Windows).
    pub enabled: bool,
    pub running: bool,
    pub autostart: bool,
    pub paused: bool,
    pub status: Option<WatcherStatus>,
    pub budget_ram_mb: f64,
    pub budget_cpu_pct: f64,
}

#[tauri::command]
pub fn watcher_status(state: State<'_, AppState>) -> CmdResult<WatcherInfo> {
    let enabled = with_db(&state, |c| db::setting(c, watcher::SETTING_ENABLED))?.as_deref() == Some("1");
    let paused = with_db(&state, |c| db::setting(c, watcher::SETTING_PAUSED))?.as_deref() == Some("1");
    let status = with_db(&state, |c| db::setting(c, watcher::SETTING_STATUS))?.and_then(|s| serde_json::from_str::<WatcherStatus>(&s).ok());
    let running = watcher::is_running();
    Ok(WatcherInfo {
        enabled,
        running,
        autostart: watcher::autostart(),
        paused,
        status: status.filter(|_| running),
        budget_ram_mb: watcher::BUDGET_RAM_MB,
        budget_cpu_pct: watcher::BUDGET_CPU_PCT,
    })
}

/// Turns profiles on (start the watcher now and with Windows) or off (put everything back, stop).
#[tauri::command]
pub fn watcher_enable(state: State<'_, AppState>, on: bool) -> CmdResult<WatcherInfo> {
    with_db(&state, |c| db::set_setting(c, watcher::SETTING_ENABLED, if on { "1" } else { "0" }))?;
    watcher::set_autostart(on)?;
    if on {
        watcher::start()?;
        // Give it a moment to take its lock so the status below is accurate.
        for _ in 0..20 {
            if watcher::is_running() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    } else {
        watcher::quit();
        for _ in 0..50 {
            if !watcher::is_running() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    watcher_status(state)
}

#[tauri::command]
pub fn watcher_pause(state: State<'_, AppState>, paused: bool) -> CmdResult<()> {
    with_db(&state, |c| db::set_setting(c, watcher::SETTING_PAUSED, if paused { "1" } else { "0" }))?;
    watcher::reload();
    Ok(())
}

#[derive(Serialize, Deserialize)]
pub struct SessionView {
    pub id: i64,
    pub game_id: String,
    pub game_name: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub restored: bool,
    pub power_plan: Option<String>,
    pub paused: Vec<String>,
    pub closed: Vec<String>,
    pub not_closed: Vec<String>,
}

#[tauri::command]
pub fn profile_sessions(state: State<'_, AppState>, limit: Option<u32>) -> CmdResult<Vec<SessionView>> {
    let rows = with_db(&state, |c| db::sessions(c, limit.unwrap_or(20).min(200)))?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let file = |p: &str| p.rsplit(['\\', '/']).next().unwrap_or(p).to_string();
            let mut paused: Vec<String> = r.state.paused.iter().map(|p| p.exe.clone()).collect();
            paused.dedup();
            SessionView {
                id: r.id,
                game_id: r.game_id,
                game_name: r.state.game_name.clone(),
                started_at: r.started_at,
                ended_at: r.ended_at,
                restored: r.restored,
                power_plan: r.state.power_plan_set.as_deref().map(|g| framewise_checks::tweak::scheme_name(g).to_string()),
                paused,
                closed: r.state.closed.iter().map(|c| file(&c.exe_path)).collect(),
                not_closed: r.state.not_closed.clone(),
            }
        })
        .collect())
}

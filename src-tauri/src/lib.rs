//! FrameWise app core: commands for the interface, local storage and the self-footprint meter.
//!
//! The core never runs as admin. Changes go through the pipeline in [`changes`]; machine-wide
//! ones are made by the elevated helper ([`elevate`]), which is this same executable started with
//! `--helper` and talks to the core over a user-only named pipe ([`pipe`]).

mod capture;
mod changes;
mod crashlog;
mod db;
mod elevate;
mod footprint;
mod live;
mod memory;
mod pipe;
mod procctl;
mod profiles;
mod standby;
mod watcher;

pub use elevate::helper_main;
pub use watcher::{stop_watcher, uninstall_cleanup, watcher_main};
#[cfg(debug_assertions)]
pub use elevate::selftest as helper_selftest;
#[cfg(debug_assertions)]
pub use live::capture_selftest;

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use framewise_checks::{evaluate_all, evaluate_one, health_score, myths::Myth, Finding, ScanResult};
use rusqlite::Connection;
use serde::Serialize;
use tauri::{Manager, State};

use footprint::{FootprintMeter, FootprintReport, LiveStats};

pub(crate) struct AppState {
    db: Mutex<Option<Connection>>,
    footprint: FootprintMeter,
    /// Set while a scan runs, so double-clicking Scan doesn't start two at once.
    scanning: AtomicBool,
    /// Set while a change, undo or cleanup runs: one at a time.
    task_running: AtomicBool,
    /// This session's restore point (made before the first change).
    restore_point: Mutex<Option<i64>>,
    /// Refresh-rate changes waiting for "Keep"; undone automatically otherwise.
    pending_confirm: Mutex<HashSet<i64>>,
    /// Live monitor and benchmark recording (phase 3).
    live: live::Live,
}

pub(crate) type CmdResult<T> = Result<T, String>;

pub(crate) fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub(crate) fn with_db<T>(state: &AppState, f: impl FnOnce(&mut Connection) -> rusqlite::Result<T>) -> CmdResult<T> {
    let mut guard = state.db.lock().map_err(|_| "database lock poisoned".to_string())?;
    let conn = guard.as_mut().ok_or_else(|| "The local database couldn't be opened.".to_string())?;
    f(conn).map_err(|e| format!("Database error: {e}"))
}

#[tauri::command]
async fn run_scan(state: State<'_, AppState>) -> CmdResult<ScanResult> {
    if state.scanning.swap(true, Ordering::SeqCst) {
        return Err("A scan is already running.".into());
    }
    struct Done<'a>(&'a AtomicBool);
    impl Drop for Done<'_> {
        fn drop(&mut self) {
            self.0.store(false, Ordering::SeqCst);
        }
    }
    let _done = Done(&state.scanning);
    let started_at = now_secs();
    let start = Instant::now();
    let snapshot = tauri::async_runtime::spawn_blocking(framewise_checks::collect::collect)
        .await
        .map_err(|e| format!("Scan failed: {e}"))?;
    let findings = evaluate_all(&snapshot);
    let mut scan = ScanResult {
        id: 0,
        started_at,
        duration_ms: start.elapsed().as_millis() as u64,
        score: health_score(&findings),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        findings,
        collection_errors: snapshot.collection_errors,
    };
    // A scan is still useful if it can't be saved, so storage errors don't fail it.
    if let Ok(id) = with_db(&state, |c| db::save_scan(c, &scan)) {
        scan.id = id;
    }
    Ok(scan)
}

#[tauri::command]
fn last_scan(state: State<'_, AppState>) -> CmdResult<Option<ScanResult>> {
    with_db(&state, |c| db::last_scan(c))
}

#[derive(Serialize)]
struct RecheckResult {
    finding: Finding,
    score: u8,
}

/// Re-runs the given checks against a fresh snapshot and stores them in the last scan. Other
/// checks keep their last-scanned result; the score is recomputed from the stored findings.
/// Returns `(findings, score)`; the score is `None` when there's no stored scan.
pub(crate) fn recheck_checks(state: &AppState, check_ids: &[String]) -> (Vec<Finding>, Option<u8>) {
    if check_ids.is_empty() {
        return (Vec::new(), None);
    }
    let snapshot = framewise_checks::collect::collect();
    let findings: Vec<Finding> = check_ids.iter().filter_map(|id| evaluate_one(&snapshot, id)).collect();
    let Ok(Some(mut last)) = with_db(state, |c| db::last_scan(c)) else {
        return (findings, None);
    };
    for f in &findings {
        if let Some(slot) = last.findings.iter_mut().find(|x| x.check_id == f.check_id) {
            *slot = f.clone();
        }
    }
    let score = health_score(&last.findings);
    for f in &findings {
        let _ = with_db(state, |c| db::update_finding(c, last.id, f, score));
    }
    (findings, Some(score))
}

#[tauri::command]
async fn recheck(app: tauri::AppHandle, check_id: String) -> CmdResult<RecheckResult> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let (mut findings, score) = recheck_checks(&state, &[check_id.clone()]);
        let finding = findings.pop().ok_or_else(|| format!("Unknown check: {check_id}"))?;
        let score = score.unwrap_or_else(|| health_score(&[finding.clone()]));
        Ok(RecheckResult { finding, score })
    })
    .await
    .map_err(|e| format!("Recheck failed: {e}"))?
}

#[tauri::command]
fn score_history(state: State<'_, AppState>) -> CmdResult<Vec<db::ScoreHistoryPoint>> {
    with_db(&state, |c| db::score_history(c, 20))
}

#[tauri::command]
fn footprint(state: State<'_, AppState>) -> Option<FootprintReport> {
    state.footprint.report()
}

#[tauri::command]
fn live_stats(state: State<'_, AppState>) -> LiveStats {
    state.footprint.live()
}

#[tauri::command]
fn myths() -> Vec<Myth> {
    framewise_checks::myths::myths()
}

#[derive(Serialize)]
struct AppInfo {
    version: String,
    data_dir: String,
    db_ok: bool,
    /// Running from the portable zip: no watcher, no installer updates.
    portable: bool,
}

/// The portable build is the release exe with a `framewise.portable` file next to it.
pub(crate) fn is_portable() -> bool {
    std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("framewise.portable").is_file())).unwrap_or(false)
}

/// Before an update installs: stop the watcher so its exe can be replaced. The app starts it again
/// after the update if profiles are on.
#[tauri::command]
fn prepare_update() {
    watcher::stop_watcher();
}

#[tauri::command]
fn app_info(state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        data_dir: db::data_dir().to_string_lossy().to_string(),
        db_ok: state.db.lock().map(|g| g.is_some()).unwrap_or(false),
        portable: is_portable(),
    }
}

/// The Windows accent colour as `#RRGGBB`, for the optional "Use Windows accent colour" setting.
#[tauri::command]
fn system_accent() -> Option<String> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let abgr: u32 = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\DWM")
        .ok()?
        .get_value("AccentColor")
        .ok()?;
    let (r, g, b) = (abgr & 0xFF, (abgr >> 8) & 0xFF, (abgr >> 16) & 0xFF);
    Some(format!("#{r:02X}{g:02X}{b:02X}"))
}

/// Opens a Windows settings page or a web link. Only these schemes are allowed, so the
/// interface can't be used to launch arbitrary programs.
#[tauri::command]
fn open_external(uri: String) -> CmdResult<()> {
    let allowed = ["ms-settings:", "windowsdefender://", "https://"];
    if !allowed.iter().any(|p| uri.starts_with(p)) || uri.chars().any(|c| c.is_control() || c == '"') {
        return Err("That link type isn't allowed.".into());
    }
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let op = HSTRING::from("open");
    let target = HSTRING::from(uri.as_str());
    // SAFETY: both strings outlive the call; no parent window or parameters.
    let result = unsafe { ShellExecuteW(HWND::default(), &op, &target, PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    if result.0 as isize <= 32 {
        return Err("Windows couldn't open that page.".into());
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    crashlog::install("app");
    let conn = db::open().ok();
    let meter = FootprintMeter::default();

    tauri::Builder::default()
        .manage(AppState {
            db: Mutex::new(conn),
            footprint: meter.clone(),
            scanning: AtomicBool::new(false),
            task_running: AtomicBool::new(false),
            restore_point: Mutex::new(None),
            pending_confirm: Mutex::new(HashSet::new()),
            live: live::Live::default(),
        })
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(move |app| {
            if let Some(window) = app.get_webview_window("main") {
                memory::watch(&window);
            }
            // Benchmark hotkey: works while a game has focus. If another app already owns the
            // shortcut, registration fails quietly and the button still works.
            {
                use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
                let _ = app.handle().plugin(
                    tauri_plugin_global_shortcut::Builder::new()
                        .with_handler(|app, _shortcut, event| {
                            if event.state() == ShortcutState::Pressed {
                                let app = app.clone();
                                // Off the UI thread: starting capture may wait on a UAC prompt.
                                std::thread::spawn(move || {
                                    let live = app.state::<AppState>().live.clone();
                                    live.hotkey(&app);
                                });
                            }
                        })
                        .build(),
                );
                let _ = app.global_shortcut().register(live::HOTKEY);
            }
            // Profiles are on but the watcher isn't running (after an update, or it was killed):
            // start it again. It also starts with Windows; this covers the time until then.
            if !is_portable() && watcher::autostart() && !watcher::is_running() {
                let on = app.state::<AppState>().db.lock().ok().and_then(|g| g.as_ref().and_then(|c| db::setting(c, watcher::SETTING_ENABLED).ok().flatten()));
                if on.as_deref() == Some("1") {
                    let _ = watcher::start();
                }
            }
            // The meter writes to its own connection so it never waits on a scan.
            let persist_conn = Mutex::new(db::open().ok());
            meter.start(move |at, ram, cpu| {
                if let Ok(guard) = persist_conn.lock() {
                    if let Some(c) = guard.as_ref() {
                        let _ = db::insert_footprint(c, "ui", at, ram, cpu);
                        let _ = db::prune(c, at);
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            run_scan,
            last_scan,
            recheck,
            score_history,
            footprint,
            live_stats,
            myths,
            app_info,
            prepare_update,
            crashlog::crash_reports,
            crashlog::crash_dismiss,
            crashlog::crash_record,
            crashlog::crash_issue,
            crashlog::watcher_budget_issue,
            standby::standby_status,
            standby::standby_purge,
            system_accent,
            open_external,
            changes::apply_fix,
            changes::fix_all_safe,
            changes::undo_changes,
            changes::undo_all,
            changes::confirm_changes,
            changes::change_log,
            changes::startup_entries,
            changes::cleanup_scan,
            changes::cleanup_run,
            changes::export_report,
            live::monitor_status,
            live::monitor_start,
            live::monitor_stop,
            live::monitor_pin,
            live::monitor_allow_without_prompt,
            live::overlay_set,
            live::benchmark_start,
            live::benchmark_cancel,
            live::benchmarks_list,
            live::benchmark_detail,
            live::benchmark_rename,
            live::benchmark_delete,
            live::benchmark_compare,
            profiles::games_list,
            profiles::game_add,
            profiles::game_remove,
            profiles::profile_save,
            profiles::running_apps,
            profiles::watcher_status,
            profiles::watcher_enable,
            profiles::watcher_pause,
            profiles::profile_sessions
        ])
        .run(tauri::generate_context!())
        .expect("error while running FrameWise");
}

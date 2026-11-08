//! The change pipeline: every change goes check → snapshot → apply → verify → log, and is rolled
//! back if a step fails. Undo writes the logged old value back through the same path.
//!
//! Per-user settings are written here, without admin rights. Machine-wide ones go through the
//! elevated helper, started at most once per task, so a task shows at most one UAC prompt.

use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::time::Duration;

use framewise_checks::cleanup::{self, CategoryInfo, CategoryScan, CleanCategory, CleanResult};
use framewise_checks::fixes::{self, PlanInput, Planned, SAFE_FIXES};
use framewise_checks::snapshot::StartupEntry;
use framewise_checks::tweak::{validate_elevated, HelperRequest, Tweak, TweakValue};
use framewise_checks::{system, Finding, Status};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::db::{self, RestorePointRow, StoredChange};
use crate::elevate::{ApiError, Elevated};
use crate::{now_secs, with_db, AppState};

/// Seconds a new refresh rate is kept without confirmation before it's undone automatically,
/// in case the monitor shows a black screen (like Windows' own "Keep display settings?").
pub const CONFIRM_SECONDS: u64 = 15;

type Res<T> = Result<T, ApiError>;

fn db_err(e: String) -> ApiError {
    ApiError::new("db_failed", e)
}

// ---------------------------------------------------------------------------------------------
// Rows for the interface

#[derive(Serialize, Clone, Debug)]
pub struct ChangeRow {
    pub id: i64,
    pub check_id: String,
    pub title: String,
    pub old_label: Option<String>,
    pub new_label: Option<String>,
    pub applied_at: u64,
    pub source: String,
    pub reverted_at: Option<u64>,
    pub needs_restart: bool,
    /// Changed (or undone) since the PC last started, and it only takes effect after a restart.
    pub restart_pending: bool,
    pub needs_admin: bool,
    pub undoable: bool,
    pub detail: Option<String>,
}

fn boot_time() -> u64 {
    now_secs().saturating_sub(system::uptime_secs())
}

fn parse_tweak(c: &StoredChange) -> Option<Tweak> {
    c.target.as_deref().and_then(|t| serde_json::from_str(t).ok())
}

fn parse_value(v: &Option<String>) -> Option<TweakValue> {
    v.as_deref().and_then(|v| serde_json::from_str(v).ok())
}

pub fn to_row(c: &StoredChange, boot: u64) -> ChangeRow {
    let tweak = parse_tweak(c);
    let label = |v: &Option<String>| tweak.as_ref().zip(parse_value(v)).map(|(t, v)| t.describe(&v));
    let last = c.reverted_at.unwrap_or(c.applied_at);
    ChangeRow {
        id: c.id,
        check_id: c.check_id.clone(),
        title: c.title.clone(),
        old_label: label(&c.old_value),
        new_label: label(&c.new_value),
        applied_at: c.applied_at,
        source: c.source.clone(),
        reverted_at: c.reverted_at,
        needs_restart: c.needs_restart,
        restart_pending: c.needs_restart && last >= boot,
        needs_admin: tweak.as_ref().is_some_and(|t| t.needs_admin()),
        undoable: tweak.is_some() && c.reverted_at.is_none(),
        detail: c.detail.clone(),
    }
}

// ---------------------------------------------------------------------------------------------
// Task: one user action, at most one UAC prompt

struct Task<'a> {
    app: &'a AppHandle,
    state: &'a AppState,
    hwnd: isize,
    helper: Option<Elevated>,
    /// Once Windows refuses permission, don't ask again within the same task.
    refused: Option<ApiError>,
}

impl<'a> Task<'a> {
    fn begin(app: &'a AppHandle, state: &'a AppState) -> Res<Task<'a>> {
        if state.task_running.swap(true, Ordering::SeqCst) {
            return Err(ApiError::new("busy", "FrameWise is already making a change. Wait for it to finish."));
        }
        let hwnd = app.get_webview_window("main").and_then(|w| w.hwnd().ok()).map(|h| h.0 as isize).unwrap_or(0);
        Ok(Task { app, state, hwnd, helper: None, refused: None })
    }

    fn progress(&self, msg: &str) {
        let _ = self.app.emit("task-progress", msg);
    }

    fn helper(&mut self) -> Res<&mut Elevated> {
        if let Some(e) = &self.refused {
            return Err(e.clone());
        }
        if self.helper.is_none() {
            self.progress("Waiting for permission in the Windows prompt…");
            match Elevated::start(self.hwnd) {
                Ok(h) => self.helper = Some(h),
                Err(e) => {
                    self.refused = Some(e.clone());
                    return Err(e);
                }
            }
        }
        Ok(self.helper.as_mut().expect("just started"))
    }

    /// Before the first change in a session, a Windows restore point.
    fn ensure_restore_point(&mut self, wanted: bool) -> Res<Option<RestorePointRow>> {
        if !wanted || self.state.restore_point.lock().map(|g| g.is_some()).unwrap_or(false) {
            return Ok(None);
        }
        self.progress("Creating a Windows restore point. This can take a minute…");
        let req = HelperRequest::CreateRestorePoint { description: "FrameWise: before changes".into() };
        let data = self.helper()?.call(&req, Duration::from_secs(600))?;
        let info: system::RestorePointInfo =
            serde_json::from_value(data).map_err(|_| ApiError::new("restore_point_failed", "Unexpected restore point reply."))?;
        let row = with_db(self.state, |c| db::insert_restore_point(c, now_secs(), info.sequence, &info.description, info.reused))
            .map_err(db_err)?;
        if let Ok(mut g) = self.state.restore_point.lock() {
            *g = Some(row.id);
        }
        Ok(Some(row))
    }

    fn read(&self, t: &Tweak) -> Res<TweakValue> {
        system::read(t).map_err(|e| ApiError::new("read_failed", e))
    }

    fn write(&mut self, t: &Tweak, v: &TweakValue) -> Res<()> {
        if t.needs_admin() {
            validate_elevated(t, v).map_err(|e| ApiError::new("refused", e))?;
            let req = HelperRequest::Write { tweak: t.clone(), value: v.clone() };
            self.helper()?.call(&req, Duration::from_secs(60)).map(|_| ())
        } else {
            system::write(t, v).map_err(|e| ApiError::new("write_failed", e))
        }
    }

    /// Re-reads until the value matches (power and display changes can take a moment to show).
    fn verify(&self, t: &Tweak, expected: &TweakValue) -> bool {
        for _ in 0..8 {
            if self.read(t).is_ok_and(|v| same(t, &v, expected)) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        false
    }

    /// Applies one planned change. `Ok(None)` when it was already set.
    fn apply(&mut self, p: &Planned, source: &str) -> Res<Option<ChangeRow>> {
        let title = p.tweak.title();
        self.progress(&format!("Changing {title}…"));
        // 1. Check
        let old = self.read(&p.tweak)?;
        if same(&p.tweak, &old, &p.value) {
            return Ok(None);
        }
        // 3. Apply (2, the restore point, is done once per session before the first apply)
        self.write(&p.tweak, &p.value)?;
        // 4. Verify, rolling back if it didn't stick
        if !self.verify(&p.tweak, &p.value) {
            let _ = self.write(&p.tweak, &old);
            return Err(ApiError::new("verify_failed", format!("{title} didn't change as expected, so FrameWise put it back.")));
        }
        // 5. Log. A change that can't be logged can't be undone, so it's rolled back too.
        let stored = StoredChange {
            check_id: p.check_id.clone(),
            applied_at: now_secs(),
            old_value: serde_json::to_string(&old).ok(),
            new_value: serde_json::to_string(&p.value).ok(),
            source: source.into(),
            target: serde_json::to_string(&p.tweak).ok(),
            title: title.clone(),
            needs_restart: p.tweak.needs_restart(),
            restore_point_id: self.state.restore_point.lock().ok().and_then(|g| *g),
            ..Default::default()
        };
        match with_db(self.state, |c| db::insert_change(c, &stored)) {
            Ok(id) => Ok(Some(to_row(&StoredChange { id, ..stored }, boot_time()))),
            Err(e) => {
                let _ = self.write(&p.tweak, &old);
                Err(ApiError::new("db_failed", format!("Couldn't record the change, so FrameWise put it back. {e}")))
            }
        }
    }

    fn undo(&mut self, c: &StoredChange, force: bool) -> Res<()> {
        let (Some(t), Some(old), Some(new)) = (parse_tweak(c), parse_value(&c.old_value), parse_value(&c.new_value)) else {
            return Err(ApiError::new("not_undoable", format!("{} can't be undone from FrameWise.", c.title)));
        };
        self.progress(&format!("Undoing {}…", c.title));
        let cur = self.read(&t)?;
        if !same(&t, &cur, &old) {
            if !force && !same(&t, &cur, &new) {
                return Err(ApiError::new(
                    "changed_since",
                    format!(
                        "{} was changed outside FrameWise since (it's now {}). Undo would set it back to {}.",
                        c.title,
                        t.describe(&cur),
                        t.describe(&old)
                    ),
                ));
            }
            self.write(&t, &old)?;
            if !self.verify(&t, &old) {
                return Err(ApiError::new("verify_failed", format!("{} didn't go back as expected.", c.title)));
            }
        }
        with_db(self.state, |db| db::mark_reverted(db, c.id, now_secs())).map_err(db_err)?;
        if let Ok(mut p) = self.state.pending_confirm.lock() {
            p.remove(&c.id);
        }
        Ok(())
    }
}

impl Drop for Task<'_> {
    fn drop(&mut self) {
        // Ends the helper (it exits on Done) before allowing the next task.
        self.helper.take();
        self.state.task_running.store(false, Ordering::SeqCst);
        let _ = self.app.emit("task-progress", "");
    }
}

fn same(t: &Tweak, a: &TweakValue, b: &TweakValue) -> bool {
    match (t, a, b) {
        (Tweak::PowerScheme | Tweak::PowerMode, TweakValue::Text(x), TweakValue::Text(y)) => x.eq_ignore_ascii_case(y),
        _ => a == b,
    }
}

/// Runs blocking work off the async runtime with access to the app state.
async fn blocking<T: Send + 'static>(app: AppHandle, f: impl FnOnce(&AppHandle, &AppState) -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        f(&app, &state)
    })
    .await
    .map_err(|e| ApiError::new("error", e.to_string()))?
}

// ---------------------------------------------------------------------------------------------
// Fix commands

fn default_true() -> bool {
    true
}

#[derive(Deserialize, Default)]
pub struct FixOptions {
    /// Create a restore point before the first change in this session (Settings, on by default).
    #[serde(default = "default_true")]
    pub restore_point: bool,
    #[serde(default)]
    pub input: PlanInput,
}

#[derive(Serialize)]
pub struct FixError {
    pub check_id: String,
    pub code: String,
    pub message: String,
}

#[derive(Serialize, Default)]
pub struct FixOutcome {
    pub changes: Vec<ChangeRow>,
    /// Titles of settings that were already as recommended.
    pub already_set: Vec<String>,
    pub errors: Vec<FixError>,
    pub restore_point: Option<RestorePointRow>,
    /// Rechecked findings and the new score.
    pub findings: Vec<Finding>,
    pub score: Option<u8>,
    /// Changes that are undone automatically unless confirmed within `confirm_seconds`.
    pub confirm_ids: Vec<i64>,
    pub confirm_seconds: u64,
}

fn run_plans(task: &mut Task, plans: Vec<Planned>, source: &str, restore_point: bool) -> Res<FixOutcome> {
    let mut out = FixOutcome::default();
    // Only what actually needs changing; a restore point isn't made for nothing.
    let mut needed = Vec::new();
    for p in plans {
        match task.read(&p.tweak) {
            Ok(v) if same(&p.tweak, &v, &p.value) => out.already_set.push(p.tweak.title()),
            Ok(_) => needed.push(p),
            Err(e) => out.errors.push(FixError { check_id: p.check_id.clone(), code: e.code, message: e.message }),
        }
    }
    if needed.is_empty() {
        return Ok(out);
    }
    // A restore point failure stops the task before anything changes; the interface can offer
    // to continue without one.
    out.restore_point = task.ensure_restore_point(restore_point)?;
    for p in &needed {
        match task.apply(p, source) {
            Ok(Some(row)) => out.changes.push(row),
            Ok(None) => out.already_set.push(p.tweak.title()),
            Err(e) => out.errors.push(FixError { check_id: p.check_id.clone(), code: e.code, message: e.message }),
        }
    }
    Ok(out)
}

fn finish(app: &AppHandle, state: &AppState, mut out: FixOutcome, check_ids: &[String]) -> FixOutcome {
    let (findings, score) = crate::recheck_checks(state, check_ids);
    out.findings = findings;
    out.score = score;
    let refresh: Vec<i64> = out.changes.iter().filter(|c| c.check_id == "refresh_rate").map(|c| c.id).collect();
    if !refresh.is_empty() {
        if let Ok(mut p) = state.pending_confirm.lock() {
            p.extend(refresh.iter().copied());
        }
        out.confirm_ids = refresh.clone();
        out.confirm_seconds = CONFIRM_SECONDS;
        let app = app.clone();
        std::thread::spawn(move || auto_revert(app, refresh));
    }
    out
}

/// Undoes unconfirmed refresh-rate changes after [`CONFIRM_SECONDS`].
fn auto_revert(app: AppHandle, ids: Vec<i64>) {
    std::thread::sleep(Duration::from_secs(CONFIRM_SECONDS));
    let state = app.state::<AppState>();
    let still: Vec<i64> = match state.pending_confirm.lock() {
        Ok(mut p) => ids.into_iter().filter(|i| p.remove(i)).collect(),
        Err(_) => return,
    };
    if still.is_empty() {
        return;
    }
    // Wait for any running task, then undo.
    for _ in 0..120 {
        match undo_inner(&app, &state, &still, true) {
            Err(e) if e.code == "busy" => std::thread::sleep(Duration::from_millis(500)),
            r => {
                let _ = app.emit("changes-auto-reverted", r.map(|o| o.reverted).unwrap_or_default());
                return;
            }
        }
    }
}

#[tauri::command]
pub async fn apply_fix(app: AppHandle, check_id: String, options: FixOptions) -> Res<FixOutcome> {
    blocking(app, move |app, state| {
        let out = {
            let mut task = Task::begin(app, state)?;
            task.progress("Checking current settings…");
            let plans = fixes::plan(&check_id, &options.input).map_err(|e| ApiError::new("plan_failed", e))?;
            run_plans(&mut task, plans, "manual", options.restore_point)?
        };
        Ok(finish(app, state, out, &[check_id]))
    })
    .await
}

#[tauri::command]
pub async fn fix_all_safe(app: AppHandle, options: FixOptions) -> Res<FixOutcome> {
    blocking(app, move |app, state| {
        let last = with_db(state, |c| db::last_scan(c)).ok().flatten().ok_or_else(|| ApiError::new("no_scan", "Run a scan first."))?;
        let ids: Vec<String> = last
            .findings
            .iter()
            .filter(|f| f.status == Status::Issue && SAFE_FIXES.contains(&f.check_id.as_str()))
            .map(|f| f.check_id.clone())
            .collect();
        let out = {
            let mut task = Task::begin(app, state)?;
            task.progress("Checking current settings…");
            let mut plans = Vec::new();
            let mut errors = Vec::new();
            for id in &ids {
                match fixes::plan(id, &PlanInput::default()) {
                    Ok(p) => plans.extend(p),
                    Err(e) => errors.push(FixError { check_id: id.clone(), code: "plan_failed".into(), message: e }),
                }
            }
            let mut out = run_plans(&mut task, plans, "fix_all", options.restore_point)?;
            out.errors.extend(errors);
            out
        };
        Ok(finish(app, state, out, &ids))
    })
    .await
}

#[derive(Serialize, Default)]
pub struct UndoOutcome {
    pub reverted: Vec<i64>,
    pub errors: Vec<(i64, ApiError)>,
    pub findings: Vec<Finding>,
    pub score: Option<u8>,
}

fn undo_inner(app: &AppHandle, state: &AppState, ids: &[i64], force: bool) -> Res<UndoOutcome> {
    let mut stored = with_db(state, |c| db::changes_by_id(c, ids)).map_err(db_err)?;
    // Newest first, so stacked changes to one setting unwind in order.
    stored.sort_by(|a, b| b.id.cmp(&a.id));
    let mut out = UndoOutcome::default();
    {
        let mut task = Task::begin(app, state)?;
        for c in stored.iter().filter(|c| c.reverted_at.is_none()) {
            match task.undo(c, force) {
                Ok(()) => out.reverted.push(c.id),
                Err(e) => out.errors.push((c.id, e)),
            }
        }
    }
    let checks: HashSet<String> = stored.iter().map(|c| c.check_id.clone()).collect();
    let checks: Vec<String> = checks.into_iter().collect();
    let (findings, score) = crate::recheck_checks(state, &checks);
    out.findings = findings;
    out.score = score;
    Ok(out)
}

#[tauri::command]
pub async fn undo_changes(app: AppHandle, ids: Vec<i64>, force: bool) -> Res<UndoOutcome> {
    blocking(app, move |app, state| undo_inner(app, state, &ids, force)).await
}

#[tauri::command]
pub async fn undo_all(app: AppHandle, force: bool) -> Res<UndoOutcome> {
    blocking(app, move |app, state| {
        let ids: Vec<i64> = with_db(state, |c| db::active_changes(c)).map_err(db_err)?.iter().map(|c| c.id).collect();
        undo_inner(app, state, &ids, force)
    })
    .await
}

/// Keeps a refresh-rate change (stops the automatic undo).
#[tauri::command]
pub fn confirm_changes(state: State<'_, AppState>, ids: Vec<i64>) {
    if let Ok(mut p) = state.pending_confirm.lock() {
        for id in ids {
            p.remove(&id);
        }
    }
}

#[derive(Serialize)]
pub struct ChangeLog {
    pub changes: Vec<ChangeRow>,
    pub restore_points: Vec<RestorePointRow>,
    /// True when System Restore points are made this session already.
    pub restore_point_this_session: bool,
}

#[tauri::command]
pub fn change_log(state: State<'_, AppState>) -> Res<ChangeLog> {
    let boot = boot_time();
    let changes = with_db(&state, |c| db::changes(c, 500)).map_err(db_err)?.iter().map(|c| to_row(c, boot)).collect();
    let restore_points = with_db(&state, |c| db::restore_points(c, 20)).map_err(db_err)?;
    Ok(ChangeLog {
        changes,
        restore_points,
        restore_point_this_session: state.restore_point.lock().map(|g| g.is_some()).unwrap_or(false),
    })
}

#[tauri::command]
pub fn startup_entries() -> Vec<StartupEntry> {
    let mut v = framewise_checks::collect::startup_entries();
    v.sort_by(|a, b| b.enabled.cmp(&a.enabled).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    v
}

// ---------------------------------------------------------------------------------------------
// Cleanup

#[derive(Serialize)]
pub struct CleanupCategory {
    #[serde(flatten)]
    pub info: CategoryInfo,
    pub scan: CategoryScan,
}

#[tauri::command]
pub async fn cleanup_scan(app: AppHandle, elevated: bool) -> Res<Vec<CleanupCategory>> {
    blocking(app, move |app, state| {
        let mut scans: Vec<CategoryScan> = std::thread::scope(|s| {
            let hs: Vec<_> = cleanup::ALL_CATEGORIES.iter().map(|c| s.spawn(move || cleanup::scan(*c))).collect();
            hs.into_iter().map(|h| h.join().unwrap_or_default()).collect()
        });
        if elevated {
            let wanted: Vec<CleanCategory> = cleanup::ALL_CATEGORIES
                .iter()
                .copied()
                .filter(|c| c.info().needs_admin && *c != CleanCategory::DriverPackages)
                .collect();
            let mut task = Task::begin(app, state)?;
            let data = task.helper()?.call(&HelperRequest::ScanCleanup { categories: wanted }, Duration::from_secs(600))?;
            task.progress("Measuring…");
            let admin: Vec<CategoryScan> = serde_json::from_value(data).unwrap_or_default();
            for a in admin {
                if let Some(slot) = scans.iter_mut().find(|s| s.id == a.id) {
                    *slot = a;
                }
            }
        }
        Ok(cleanup::ALL_CATEGORIES.iter().zip(scans).map(|(c, scan)| CleanupCategory { info: c.info(), scan }).collect())
    })
    .await
}

#[derive(Serialize, Default)]
pub struct CleanupOutcome {
    pub results: Vec<CleanResult>,
    pub freed_bytes: u64,
    pub recycled_bytes: u64,
}

fn size_text(b: u64) -> String {
    let mb = b as f64 / 1_048_576.0;
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else {
        format!("{mb:.0} MB")
    }
}

#[tauri::command]
pub async fn cleanup_run(app: AppHandle, categories: Vec<CleanCategory>, permanent: bool) -> Res<CleanupOutcome> {
    blocking(app, move |app, state| {
        let mut out = CleanupOutcome::default();
        let mut task = Task::begin(app, state)?;
        // Per-user categories first, so a refused UAC prompt still leaves those done.
        let mut ordered = categories.clone();
        ordered.sort_by_key(|c| c.info().needs_admin);
        ordered.dedup();
        for cat in ordered {
            let info = cat.info();
            task.progress(&format!("Cleaning {}…", info.title.to_lowercase()));
            let r: CleanResult = if cat == CleanCategory::DriverPackages {
                let mut r = CleanResult { id: Some(cat), ..Default::default() };
                match cleanup::driver_packages() {
                    Ok(pkgs) => {
                        for p in pkgs.iter().filter(|p| cleanup::removable(p)) {
                            let req = HelperRequest::RemoveDriverPackage { inf: p.inf.clone() };
                            match task.helper().and_then(|h| h.call(&req, Duration::from_secs(300))) {
                                Ok(v) => {
                                    r.removed_items += 1;
                                    r.freed_bytes += v.as_u64().unwrap_or(p.bytes);
                                }
                                Err(e) => {
                                    r.skipped_items += 1;
                                    r.error = Some(e.message);
                                }
                            }
                        }
                    }
                    Err(e) => r.error = Some(e),
                }
                r
            } else if info.needs_admin {
                match task.helper().and_then(|h| h.call(&HelperRequest::Clean { category: cat }, Duration::from_secs(1800))) {
                    Ok(v) => serde_json::from_value(v).unwrap_or_default(),
                    Err(e) => CleanResult { id: Some(cat), error: Some(e.message), ..Default::default() },
                }
            } else {
                cleanup::clean(cat, !permanent)
            };
            if r.recycled {
                out.recycled_bytes += r.freed_bytes;
            } else {
                out.freed_bytes += r.freed_bytes;
            }
            if r.removed_items > 0 || r.freed_bytes > 0 {
                let verb = if r.recycled { "Moved to the Recycle Bin" } else { "Deleted" };
                let mut detail = format!("{verb}: {} ({} items)", size_text(r.freed_bytes), r.removed_items);
                if r.skipped_items > 0 {
                    detail.push_str(&format!(", {} in use and left alone", r.skipped_items));
                }
                let stored = StoredChange {
                    check_id: "cleanup".into(),
                    applied_at: now_secs(),
                    source: "cleanup".into(),
                    title: format!("Cleanup · {}", info.title),
                    detail: Some(detail),
                    ..Default::default()
                };
                let _ = with_db(state, |c| db::insert_change(c, &stored));
            }
            out.results.push(r);
        }
        Ok(out)
    })
    .await
}

// ---------------------------------------------------------------------------------------------
// Report

fn local_time(unix: u64) -> String {
    use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    let ft = (unix + 11_644_473_600) * 10_000_000;
    let ft = FILETIME { dwLowDateTime: ft as u32, dwHighDateTime: (ft >> 32) as u32 };
    let (mut utc, mut local) = (SYSTEMTIME::default(), SYSTEMTIME::default());
    // SAFETY: plain conversions on stack values.
    unsafe {
        if FileTimeToSystemTime(&ft, &mut utc).is_err() || SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).is_err() {
            return format!("{unix}");
        }
    }
    format!("{}-{:02}-{:02} {:02}:{:02}", local.wYear, local.wMonth, local.wDay, local.wHour, local.wMinute)
}

fn downloads_dir() -> std::path::PathBuf {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_Downloads, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    // SAFETY: the returned string is freed with CoTaskMemFree.
    unsafe {
        if let Ok(p) = SHGetKnownFolderPath(&FOLDERID_Downloads, KF_FLAG_DEFAULT, None) {
            let s = p.to_string().unwrap_or_default();
            CoTaskMemFree(Some(p.0 as *const _));
            if !s.is_empty() {
                return s.into();
            }
        }
    }
    std::env::var_os("USERPROFILE").map(|p| std::path::PathBuf::from(p).join("Downloads")).unwrap_or_else(std::env::temp_dir)
}

/// Writes a plain-text report of the last scan and every change to the Downloads folder and
/// shows it in File Explorer.
#[tauri::command]
pub fn export_report(state: State<'_, AppState>) -> Res<String> {
    let mut t = String::new();
    let now = now_secs();
    t.push_str(&format!("FrameWise report\r\nCreated {} · FrameWise {}\r\n\r\n", local_time(now), env!("CARGO_PKG_VERSION")));
    if let Ok(Some(scan)) = with_db(&state, |c| db::last_scan(c)) {
        t.push_str(&format!("Health score {} (scan of {})\r\n\r\n", scan.score, local_time(scan.started_at)));
        for (label, st) in [("Needs attention", Status::Issue), ("Check manually", Status::Unknown), ("Good", Status::Ok)] {
            let items: Vec<&Finding> = scan.findings.iter().filter(|f| f.status == st).collect();
            if items.is_empty() {
                continue;
            }
            t.push_str(&format!("{label}\r\n"));
            for f in items {
                t.push_str(&format!("  - {}: {}\r\n", f.title, f.summary));
            }
            t.push_str("\r\n");
        }
    }
    let boot = boot_time();
    let changes = with_db(&state, |c| db::changes(c, 1000)).map_err(db_err)?;
    t.push_str("Changes made by FrameWise\r\n");
    if changes.is_empty() {
        t.push_str("  None\r\n");
    }
    for c in &changes {
        let r = to_row(c, boot);
        let what = match (&r.old_label, &r.new_label) {
            (Some(o), Some(n)) => format!("{o} → {n}"),
            _ => r.detail.clone().unwrap_or_default(),
        };
        let undone = r.reverted_at.map(|at| format!(" · undone {}", local_time(at))).unwrap_or_default();
        t.push_str(&format!("  - {} · {}: {} ({}){}\r\n", local_time(r.applied_at), r.title, what, r.source.replace('_', " "), undone));
    }
    let rps = with_db(&state, |c| db::restore_points(c, 50)).map_err(db_err)?;
    if !rps.is_empty() {
        t.push_str("\r\nRestore points\r\n");
        for rp in rps {
            let reused = if rp.reused { " (Windows reused one made in the previous 24 hours)" } else { "" };
            t.push_str(&format!("  - {} · #{} {}{}\r\n", local_time(rp.created_at), rp.sequence.unwrap_or(0), rp.description, reused));
        }
    }
    let dir = downloads_dir();
    let name = format!("FrameWise report {}.txt", local_time(now).replace(':', "-"));
    let path = dir.join(name);
    std::fs::write(&path, t.as_bytes()).map_err(|e| ApiError::new("write_failed", format!("Couldn't save the report: {e}")))?;
    reveal(&path);
    Ok(path.to_string_lossy().to_string())
}

fn reveal(path: &std::path::Path) {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let params = HSTRING::from(format!("/select,\"{}\"", path.display()));
    // SAFETY: strings outlive the call.
    unsafe {
        ShellExecuteW(HWND::default(), &HSTRING::from("open"), &HSTRING::from("explorer.exe"), &params, PCWSTR::null(), SW_SHOWNORMAL);
    }
}

//! Local SQLite storage in `%LOCALAPPDATA%\FrameWise\framewise.db`.
//!
//! The full schema from the design document is created up front so later phases only add rows.

use std::path::PathBuf;

use framewise_checks::{Finding, ScanResult};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

pub fn data_dir() -> PathBuf {
    // Tests (and anyone who wants a separate profile) can point FrameWise at another folder.
    if let Some(d) = std::env::var_os("FRAMEWISE_DATA_DIR").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("FrameWise")
}

const SCHEMA_V1: &str = r#"
CREATE TABLE IF NOT EXISTS scans (
    id INTEGER PRIMARY KEY,
    started_at INTEGER NOT NULL,
    duration_ms INTEGER NOT NULL,
    score INTEGER NOT NULL,
    app_version TEXT NOT NULL,
    collection_errors TEXT NOT NULL DEFAULT '[]'
);
CREATE TABLE IF NOT EXISTS findings (
    scan_id INTEGER NOT NULL REFERENCES scans(id) ON DELETE CASCADE,
    check_id TEXT NOT NULL,
    status TEXT NOT NULL,
    detected_value TEXT,
    expected_value TEXT,
    impact TEXT NOT NULL,
    payload TEXT NOT NULL,
    PRIMARY KEY (scan_id, check_id)
);
CREATE TABLE IF NOT EXISTS changes (
    id INTEGER PRIMARY KEY,
    check_id TEXT NOT NULL,
    applied_at INTEGER NOT NULL,
    old_value TEXT,
    new_value TEXT,
    source TEXT NOT NULL,
    reverted_at INTEGER
);
CREATE TABLE IF NOT EXISTS restore_points (
    id INTEGER PRIMARY KEY,
    created_at INTEGER NOT NULL,
    windows_sequence_number INTEGER
);
CREATE TABLE IF NOT EXISTS profiles (
    game_id TEXT PRIMARY KEY,
    exe_path TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 0,
    actions TEXT NOT NULL DEFAULT '[]'
);
CREATE TABLE IF NOT EXISTS profile_sessions (
    id INTEGER PRIMARY KEY,
    game_id TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    ended_at INTEGER,
    saved_state TEXT NOT NULL,
    restored INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS benchmarks (
    id INTEGER PRIMARY KEY,
    game_id TEXT,
    label TEXT,
    recorded_at INTEGER NOT NULL,
    avg_fps REAL,
    low_1 REAL,
    low_01 REAL,
    frametime_file TEXT
);
CREATE TABLE IF NOT EXISTS footprint_samples (
    process TEXT NOT NULL,
    sampled_at INTEGER NOT NULL,
    ram_mb REAL NOT NULL,
    cpu_pct REAL NOT NULL
);
CREATE INDEX IF NOT EXISTS footprint_samples_time ON footprint_samples(sampled_at);
"#;

pub fn open() -> rusqlite::Result<Connection> {
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let conn = Connection::open(dir.join("framewise.db"))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&conn)?;
    Ok(conn)
}

/// Phase 2: the undo log needs the setting itself (`target`, a typed tweak as JSON), a title for
/// the History screen, and whether it needs a restart. Cleanup runs are logged with a `detail`.
const SCHEMA_V2: &str = r#"
ALTER TABLE changes ADD COLUMN target TEXT;
ALTER TABLE changes ADD COLUMN title TEXT NOT NULL DEFAULT '';
ALTER TABLE changes ADD COLUMN needs_restart INTEGER NOT NULL DEFAULT 0;
ALTER TABLE changes ADD COLUMN restore_point_id INTEGER;
ALTER TABLE changes ADD COLUMN detail TEXT;
ALTER TABLE restore_points ADD COLUMN description TEXT NOT NULL DEFAULT '';
ALTER TABLE restore_points ADD COLUMN reused INTEGER NOT NULL DEFAULT 0;
CREATE INDEX IF NOT EXISTS changes_active ON changes(reverted_at);
"#;

pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version < 1 {
        conn.execute_batch(SCHEMA_V1)?;
        conn.pragma_update(None, "user_version", 1)?;
    }
    if version < 2 {
        conn.execute_batch(&format!("BEGIN; {SCHEMA_V2} PRAGMA user_version = 2; COMMIT;"))?;
    }
    if version < 3 {
        conn.execute_batch(&format!("BEGIN; {SCHEMA_V3} PRAGMA user_version = 3; COMMIT;"))?;
    }
    if version < 4 {
        conn.execute_batch(&format!("BEGIN; {SCHEMA_V4} PRAGMA user_version = 4; COMMIT;"))?;
    }
    Ok(())
}

/// Phase 3: benchmark runs keep the measured app, length and full statistics.
const SCHEMA_V3: &str = r#"
ALTER TABLE benchmarks ADD COLUMN app TEXT NOT NULL DEFAULT '';
ALTER TABLE benchmarks ADD COLUMN duration_s REAL NOT NULL DEFAULT 0;
ALTER TABLE benchmarks ADD COLUMN frames INTEGER NOT NULL DEFAULT 0;
ALTER TABLE benchmarks ADD COLUMN stats TEXT NOT NULL DEFAULT '{}';
CREATE INDEX IF NOT EXISTS benchmarks_recorded ON benchmarks (recorded_at);
"#;

/// Phase 4: games added by hand, small key/value settings (profiles on/off, watcher status), and
/// the game each profile belongs to, so the watcher never has to scan the libraries.
const SCHEMA_V4: &str = r#"
CREATE TABLE IF NOT EXISTS manual_games (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    exe_path TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
ALTER TABLE profiles ADD COLUMN game TEXT NOT NULL DEFAULT '{}';
CREATE INDEX IF NOT EXISTS profile_sessions_open ON profile_sessions (restored);
"#;

pub fn setting(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    use rusqlite::OptionalExtension;
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional()
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", [key, value])?;
    Ok(())
}

pub fn manual_games(conn: &Connection) -> rusqlite::Result<Vec<framewise_checks::profiles::Game>> {
    use framewise_checks::profiles::{Game, GameSource};
    let mut st = conn.prepare("SELECT id, name, exe_path FROM manual_games ORDER BY name")?;
    let rows = st
        .query_map([], |r| {
            Ok(Game { id: r.get(0)?, name: r.get(1)?, source: GameSource::Manual, install_dir: None, exe: Some(r.get(2)?) })
        })?
        .collect();
    rows
}

pub fn add_manual_game(conn: &Connection, g: &framewise_checks::profiles::Game) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO manual_games (id, name, exe_path) VALUES (?1, ?2, ?3) ON CONFLICT(id) DO UPDATE SET name = excluded.name",
        rusqlite::params![g.id, g.name, g.exe.clone().unwrap_or_default()],
    )?;
    Ok(())
}

pub fn remove_manual_game(conn: &Connection, id: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM manual_games WHERE id = ?1", [id])?;
    conn.execute("DELETE FROM profiles WHERE game_id = ?1", [id])?;
    Ok(())
}

/// Profiles with the game each belongs to.
pub fn profiles(conn: &Connection) -> rusqlite::Result<Vec<(framewise_checks::profiles::Game, framewise_checks::profiles::Profile)>> {
    use framewise_checks::profiles::{Game, Profile, ProfileActions};
    let mut st = conn.prepare("SELECT game_id, enabled, actions, game FROM profiles")?;
    let rows = st
        .query_map([], |r| {
            let id: String = r.get(0)?;
            let actions: String = r.get(2)?;
            let game: String = r.get(3)?;
            Ok((id, r.get::<_, i64>(1)? != 0, actions, game))
        })?
        .filter_map(|r| r.ok())
        .filter_map(|(id, enabled, actions, game)| {
            let game: Game = serde_json::from_str(&game).ok()?;
            let actions: ProfileActions = serde_json::from_str(&actions).unwrap_or_default();
            Some((game, Profile { game_id: id, enabled, actions }))
        })
        .collect();
    Ok(rows)
}

pub fn save_profile(conn: &Connection, game: &framewise_checks::profiles::Game, p: &framewise_checks::profiles::Profile) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO profiles (game_id, exe_path, enabled, actions, game) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(game_id) DO UPDATE SET exe_path = excluded.exe_path, enabled = excluded.enabled, actions = excluded.actions, game = excluded.game",
        rusqlite::params![
            p.game_id,
            game.exe.clone().or(game.install_dir.clone()).unwrap_or_default(),
            p.enabled as i64,
            serde_json::to_string(&p.actions).unwrap_or_else(|_| "{}".into()),
            serde_json::to_string(game).unwrap_or_else(|_| "{}".into())
        ],
    )?;
    Ok(())
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct SessionRow {
    pub id: i64,
    pub game_id: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub restored: bool,
    pub state: framewise_checks::profiles::SavedState,
}

pub fn insert_session(conn: &Connection, game_id: &str, at: u64, state: &framewise_checks::profiles::SavedState) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO profile_sessions (game_id, started_at, saved_state, restored) VALUES (?1, ?2, ?3, 0)",
        rusqlite::params![game_id, at as i64, serde_json::to_string(state).unwrap_or_default()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_session_state(conn: &Connection, id: i64, state: &framewise_checks::profiles::SavedState) -> rusqlite::Result<()> {
    conn.execute("UPDATE profile_sessions SET saved_state = ?2 WHERE id = ?1", rusqlite::params![id, serde_json::to_string(state).unwrap_or_default()])?;
    Ok(())
}

pub fn finish_session(conn: &Connection, id: i64, at: u64, state: &framewise_checks::profiles::SavedState) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE profile_sessions SET ended_at = ?2, restored = 1, saved_state = ?3 WHERE id = ?1",
        rusqlite::params![id, at as i64, serde_json::to_string(state).unwrap_or_default()],
    )?;
    Ok(())
}

fn session_from_row(r: &rusqlite::Row) -> rusqlite::Result<SessionRow> {
    let state: String = r.get(5)?;
    Ok(SessionRow {
        id: r.get(0)?,
        game_id: r.get(1)?,
        started_at: r.get(2)?,
        ended_at: r.get(3)?,
        restored: r.get::<_, i64>(4)? != 0,
        state: serde_json::from_str(&state).unwrap_or_default(),
    })
}

/// Sessions that were never restored (the watcher crashed, or the PC restarted mid-game).
pub fn open_sessions(conn: &Connection) -> rusqlite::Result<Vec<SessionRow>> {
    let mut st = conn.prepare("SELECT id, game_id, started_at, ended_at, restored, saved_state FROM profile_sessions WHERE restored = 0 ORDER BY id")?;
    let rows = st.query_map([], session_from_row)?.collect();
    rows
}

pub fn sessions(conn: &Connection, limit: u32) -> rusqlite::Result<Vec<SessionRow>> {
    let mut st = conn.prepare("SELECT id, game_id, started_at, ended_at, restored, saved_state FROM profile_sessions ORDER BY id DESC LIMIT ?1")?;
    let rows = st.query_map([limit], session_from_row)?.collect();
    rows
}

/// Raw frame-time files: the newest this many are kept, older runs keep their statistics only.
pub const KEEP_FRAMETIME_FILES: usize = 50;

pub fn benchmarks_dir() -> PathBuf {
    data_dir().join("benchmarks")
}

#[derive(serde::Serialize, Clone, Debug)]
pub struct BenchmarkRow {
    pub id: i64,
    pub app: String,
    pub label: String,
    pub recorded_at: i64,
    pub duration_s: f64,
    pub frames: i64,
    pub avg_fps: f64,
    pub low_1: f64,
    pub low_01: f64,
    pub stats: framewise_checks::frames::FrameStats,
    pub has_frames: bool,
}

fn benchmark_from_row(r: &rusqlite::Row) -> rusqlite::Result<BenchmarkRow> {
    let stats: String = r.get(9)?;
    let file: Option<String> = r.get(10)?;
    Ok(BenchmarkRow {
        id: r.get(0)?,
        app: r.get(1)?,
        label: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
        recorded_at: r.get(3)?,
        duration_s: r.get(4)?,
        frames: r.get(5)?,
        avg_fps: r.get::<_, Option<f64>>(6)?.unwrap_or(0.0),
        low_1: r.get::<_, Option<f64>>(7)?.unwrap_or(0.0),
        low_01: r.get::<_, Option<f64>>(8)?.unwrap_or(0.0),
        stats: serde_json::from_str(&stats).unwrap_or_default(),
        has_frames: file.is_some_and(|f| benchmarks_dir().join(f).is_file()),
    })
}

const BENCH_COLS: &str = "id, app, label, recorded_at, duration_s, frames, avg_fps, low_1, low_01, stats, frametime_file";

pub fn insert_benchmark(
    conn: &Connection,
    app: &str,
    label: &str,
    at: u64,
    s: &framewise_checks::frames::FrameStats,
    file: Option<&str>,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO benchmarks (app, label, recorded_at, duration_s, frames, avg_fps, low_1, low_01, stats, frametime_file)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            app,
            label,
            at as i64,
            s.duration_s,
            s.frames as i64,
            s.avg_fps,
            s.low_1,
            s.low_01,
            serde_json::to_string(s).unwrap_or_else(|_| "{}".into()),
            file
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn benchmarks(conn: &Connection) -> rusqlite::Result<Vec<BenchmarkRow>> {
    let mut st = conn.prepare(&format!("SELECT {BENCH_COLS} FROM benchmarks ORDER BY recorded_at DESC, id DESC"))?;
    let rows = st.query_map([], benchmark_from_row)?.collect();
    rows
}

pub fn benchmarks_by_id(conn: &Connection, ids: &[i64]) -> rusqlite::Result<Vec<BenchmarkRow>> {
    let mut out = Vec::new();
    let mut st = conn.prepare(&format!("SELECT {BENCH_COLS} FROM benchmarks WHERE id = ?1"))?;
    for id in ids {
        if let Some(r) = st.query_map([id], benchmark_from_row)?.next() {
            out.push(r?);
        }
    }
    Ok(out)
}

pub fn benchmark_file(conn: &Connection, id: i64) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT frametime_file FROM benchmarks WHERE id = ?1", [id], |r| r.get(0))
}

pub fn rename_benchmark(conn: &Connection, id: i64, label: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE benchmarks SET label = ?2 WHERE id = ?1", rusqlite::params![id, label])?;
    Ok(())
}

pub fn delete_benchmark(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    if let Ok(Some(f)) = benchmark_file(conn, id) {
        let _ = std::fs::remove_file(benchmarks_dir().join(f));
    }
    conn.execute("DELETE FROM benchmarks WHERE id = ?1", [id])?;
    Ok(())
}

/// Keeps the newest `KEEP_FRAMETIME_FILES` frame-time files; older runs keep their statistics.
pub fn prune_benchmark_files(conn: &Connection) -> rusqlite::Result<()> {
    let mut st = conn.prepare("SELECT id, frametime_file FROM benchmarks WHERE frametime_file IS NOT NULL ORDER BY recorded_at DESC, id DESC")?;
    let old: Vec<(i64, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.filter_map(|r| r.ok()).skip(KEEP_FRAMETIME_FILES).collect();
    for (id, f) in old {
        let _ = std::fs::remove_file(benchmarks_dir().join(&f));
        conn.execute("UPDATE benchmarks SET frametime_file = NULL WHERE id = ?1", [id])?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Undo log

#[derive(Clone, Debug, Default)]
pub struct StoredChange {
    pub id: i64,
    pub check_id: String,
    pub applied_at: u64,
    pub old_value: Option<String>,
    pub new_value: Option<String>,
    pub source: String,
    pub reverted_at: Option<u64>,
    pub target: Option<String>,
    pub title: String,
    pub needs_restart: bool,
    pub restore_point_id: Option<i64>,
    pub detail: Option<String>,
}

const CHANGE_COLS: &str =
    "id, check_id, applied_at, old_value, new_value, source, reverted_at, target, title, needs_restart, restore_point_id, detail";

fn change_row(r: &rusqlite::Row) -> rusqlite::Result<StoredChange> {
    Ok(StoredChange {
        id: r.get(0)?,
        check_id: r.get(1)?,
        applied_at: r.get::<_, i64>(2)? as u64,
        old_value: r.get(3)?,
        new_value: r.get(4)?,
        source: r.get(5)?,
        reverted_at: r.get::<_, Option<i64>>(6)?.map(|v| v as u64),
        target: r.get(7)?,
        title: r.get(8)?,
        needs_restart: r.get::<_, i64>(9)? != 0,
        restore_point_id: r.get(10)?,
        detail: r.get(11)?,
    })
}

pub fn insert_change(conn: &Connection, c: &StoredChange) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO changes (check_id, applied_at, old_value, new_value, source, reverted_at, target, title, needs_restart, restore_point_id, detail)
         VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7, ?8, ?9, ?10)",
        params![
            c.check_id,
            c.applied_at as i64,
            c.old_value,
            c.new_value,
            c.source,
            c.target,
            c.title,
            c.needs_restart as i64,
            c.restore_point_id,
            c.detail
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn changes(conn: &Connection, limit: u32) -> rusqlite::Result<Vec<StoredChange>> {
    let mut stmt = conn.prepare(&format!("SELECT {CHANGE_COLS} FROM changes ORDER BY id DESC LIMIT ?1"))?;
    let rows = stmt.query_map([limit], change_row)?.filter_map(Result::ok).collect();
    Ok(rows)
}

pub fn changes_by_id(conn: &Connection, ids: &[i64]) -> rusqlite::Result<Vec<StoredChange>> {
    let mut stmt = conn.prepare(&format!("SELECT {CHANGE_COLS} FROM changes WHERE id = ?1"))?;
    let mut out = Vec::new();
    for id in ids {
        if let Ok(c) = stmt.query_row([id], change_row) {
            out.push(c);
        }
    }
    Ok(out)
}

/// Changes that can still be undone, newest first.
pub fn active_changes(conn: &Connection) -> rusqlite::Result<Vec<StoredChange>> {
    let mut stmt =
        conn.prepare(&format!("SELECT {CHANGE_COLS} FROM changes WHERE reverted_at IS NULL AND target IS NOT NULL ORDER BY id DESC"))?;
    let rows = stmt.query_map([], change_row)?.filter_map(Result::ok).collect();
    Ok(rows)
}

pub fn mark_reverted(conn: &Connection, id: i64, at: u64) -> rusqlite::Result<()> {
    conn.execute("UPDATE changes SET reverted_at = ?1 WHERE id = ?2", params![at as i64, id])?;
    Ok(())
}

#[derive(Serialize, Clone, Debug)]
pub struct RestorePointRow {
    pub id: i64,
    pub created_at: u64,
    pub sequence: Option<i64>,
    pub description: String,
    pub reused: bool,
}

pub fn insert_restore_point(conn: &Connection, at: u64, seq: i64, description: &str, reused: bool) -> rusqlite::Result<RestorePointRow> {
    conn.execute(
        "INSERT INTO restore_points (created_at, windows_sequence_number, description, reused) VALUES (?1, ?2, ?3, ?4)",
        params![at as i64, seq, description, reused as i64],
    )?;
    Ok(RestorePointRow { id: conn.last_insert_rowid(), created_at: at, sequence: Some(seq), description: description.into(), reused })
}

pub fn restore_points(conn: &Connection, limit: u32) -> rusqlite::Result<Vec<RestorePointRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, created_at, windows_sequence_number, description, reused FROM restore_points ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map([limit], |r| {
            Ok(RestorePointRow {
                id: r.get(0)?,
                created_at: r.get::<_, i64>(1)? as u64,
                sequence: r.get(2)?,
                description: r.get(3)?,
                reused: r.get::<_, i64>(4)? != 0,
            })
        })?
        .filter_map(Result::ok)
        .collect();
    Ok(rows)
}

fn enum_str<T: Serialize>(v: &T) -> String {
    serde_json::to_value(v).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
}

fn insert_finding(conn: &Connection, scan_id: i64, f: &Finding) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO findings (scan_id, check_id, status, detected_value, expected_value, impact, payload)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            scan_id,
            f.check_id,
            enum_str(&f.status),
            f.detected,
            f.expected,
            enum_str(&f.impact),
            serde_json::to_string(f).unwrap_or_default()
        ],
    )?;
    Ok(())
}

pub fn save_scan(conn: &mut Connection, scan: &ScanResult) -> rusqlite::Result<i64> {
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO scans (started_at, duration_ms, score, app_version, collection_errors) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            scan.started_at as i64,
            scan.duration_ms as i64,
            scan.score,
            scan.app_version,
            serde_json::to_string(&scan.collection_errors).unwrap_or_default()
        ],
    )?;
    let id = tx.last_insert_rowid();
    for f in &scan.findings {
        insert_finding(&tx, id, f)?;
    }
    tx.commit()?;
    Ok(id)
}

pub fn last_scan(conn: &Connection) -> rusqlite::Result<Option<ScanResult>> {
    let row = conn
        .query_row(
            "SELECT id, started_at, duration_ms, score, app_version, collection_errors FROM scans ORDER BY id DESC LIMIT 1",
            [],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                ))
            },
        )
        .optional()?;
    let Some((id, started_at, duration_ms, score, app_version, errors)) = row else {
        return Ok(None);
    };
    let mut stmt = conn.prepare("SELECT payload FROM findings WHERE scan_id = ?1 ORDER BY rowid")?;
    let findings: Vec<Finding> = stmt
        .query_map([id], |r| r.get::<_, String>(0))?
        .filter_map(|p| p.ok().and_then(|p| serde_json::from_str(&p).ok()))
        .collect();
    // Keep catalogue order even if findings were rechecked (re-inserted) later, and recompute
    // what can be fixed (scans saved before phase 2 have no fix information).
    let mut findings = findings;
    for f in &mut findings {
        f.fix = framewise_checks::fixes::fix_info(f);
    }
    findings.sort_by_key(|f| {
        framewise_checks::CHECK_IDS.iter().position(|c| *c == f.check_id).unwrap_or(usize::MAX)
    });
    Ok(Some(ScanResult {
        id,
        started_at: started_at as u64,
        duration_ms: duration_ms as u64,
        score: score as u8,
        app_version,
        findings,
        collection_errors: serde_json::from_str(&errors).unwrap_or_default(),
    }))
}

/// Replaces one finding in a scan after a Recheck and stores the new score.
pub fn update_finding(conn: &Connection, scan_id: i64, f: &Finding, new_score: u8) -> rusqlite::Result<()> {
    insert_finding(conn, scan_id, f)?;
    conn.execute("UPDATE scans SET score = ?1 WHERE id = ?2", params![new_score, scan_id])?;
    Ok(())
}

#[derive(Serialize)]
pub struct ScoreHistoryPoint {
    pub started_at: u64,
    pub score: u8,
}

pub fn score_history(conn: &Connection, limit: u32) -> rusqlite::Result<Vec<ScoreHistoryPoint>> {
    let mut stmt = conn.prepare("SELECT started_at, score FROM scans ORDER BY id DESC LIMIT ?1")?;
    let mut rows: Vec<ScoreHistoryPoint> = stmt
        .query_map([limit], |r| {
            Ok(ScoreHistoryPoint { started_at: r.get::<_, i64>(0)? as u64, score: r.get::<_, i64>(1)? as u8 })
        })?
        .filter_map(Result::ok)
        .collect();
    rows.reverse();
    Ok(rows)
}

pub fn insert_footprint(conn: &Connection, process: &str, at: u64, ram_mb: f64, cpu_pct: f64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO footprint_samples (process, sampled_at, ram_mb, cpu_pct) VALUES (?1, ?2, ?3, ?4)",
        params![process, at as i64, ram_mb, cpu_pct],
    )?;
    Ok(())
}

/// Footprint samples are kept for 7 days.
pub fn prune(conn: &Connection, now: u64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM footprint_samples WHERE sampled_at < ?1", [now as i64 - 7 * 86_400])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use framewise_checks::{evaluate_all, health_score, Snapshot};

    #[test]
    fn scan_round_trip() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        let findings = evaluate_all(&Snapshot::default());
        let scan = ScanResult {
            id: 0,
            started_at: 1_700_000_000,
            duration_ms: 600,
            score: health_score(&findings),
            app_version: "0.1.0".into(),
            findings: findings.clone(),
            collection_errors: vec!["x".into()],
        };
        let id = save_scan(&mut conn, &scan).unwrap();
        let back = last_scan(&conn).unwrap().unwrap();
        assert_eq!(back.id, id);
        assert_eq!(back.findings, findings);
        assert_eq!(back.collection_errors, vec!["x".to_string()]);

        let mut changed = findings[0].clone();
        changed.summary = "rechecked".into();
        update_finding(&conn, id, &changed, 42).unwrap();
        let back = last_scan(&conn).unwrap().unwrap();
        assert_eq!(back.score, 42);
        assert_eq!(back.findings[0].summary, "rechecked");
        assert_eq!(back.findings.len(), findings.len());
        assert_eq!(score_history(&conn, 10).unwrap().len(), 1);
    }

    #[test]
    fn undo_log_round_trip() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        // Migrating twice is a no-op.
        migrate(&conn).unwrap();
        let rp = insert_restore_point(&conn, 10, 77, "FrameWise", true).unwrap();
        let c = StoredChange {
            check_id: "game_mode".into(),
            applied_at: 100,
            old_value: Some(r#"{"type":"absent"}"#.into()),
            new_value: Some(r#"{"type":"dword","value":1}"#.into()),
            source: "manual".into(),
            target: Some(r#"{"kind":"game_mode"}"#.into()),
            title: "Game Mode".into(),
            restore_point_id: Some(rp.id),
            ..Default::default()
        };
        let id = insert_change(&conn, &c).unwrap();
        assert_eq!(active_changes(&conn).unwrap().len(), 1);
        mark_reverted(&conn, id, 200).unwrap();
        assert!(active_changes(&conn).unwrap().is_empty());
        let back = &changes_by_id(&conn, &[id]).unwrap()[0];
        assert_eq!(back.reverted_at, Some(200));
        assert_eq!(back.title, "Game Mode");
        assert_eq!(restore_points(&conn, 5).unwrap()[0].sequence, Some(77));
    }

    #[test]
    fn upgrades_a_phase_1_database() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_V1).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute("INSERT INTO changes (check_id, applied_at, source) VALUES ('x', 1, 'manual')", []).unwrap();
        migrate(&conn).unwrap();
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, 4);
        assert_eq!(changes(&conn, 10).unwrap()[0].title, "");
    }
}

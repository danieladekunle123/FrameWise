//! Local crash reports. A panic in any FrameWise process (app, watcher, helper) or an error in the
//! interface writes a scrubbed report to `%LOCALAPPDATA%\FrameWise\crashes`. Nothing is sent: the
//! next time FrameWise opens it says so, shows the exact text, and the user may open it as a
//! pre-filled GitHub issue in their browser (sending is their click on GitHub).

use std::path::PathBuf;

use framewise_checks::crash::{issue_body, scrub, short_location, CrashReport};
use serde::Serialize;

const KEEP: usize = 20;
const REPO_ISSUES: &str = "https://github.com/danieladekunle123/FrameWise/issues/new";

fn dir() -> PathBuf {
    crate::db::data_dir().join("crashes")
}

fn user_name() -> Option<String> {
    std::env::var("USERNAME").ok()
}

fn write(r: &CrashReport) {
    let d = dir();
    let _ = std::fs::create_dir_all(&d);
    let name = format!("{}-{}-{}.json", r.at, r.process, std::process::id());
    let _ = std::fs::write(d.join(&name), serde_json::to_vec_pretty(r).unwrap_or_default());
    // Keep the newest few.
    if let Ok(rd) = std::fs::read_dir(&d) {
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
        files.sort();
        let excess = files.len().saturating_sub(KEEP);
        for f in files.into_iter().take(excess) {
            let _ = std::fs::remove_file(f);
        }
    }
}

/// Records panics in this process (keeps the default hook's behaviour too).
pub fn install(process: &'static str) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic".into());
        write(&CrashReport {
            id: String::new(),
            at: crate::now_secs(),
            version: env!("CARGO_PKG_VERSION").into(),
            process: process.into(),
            message: scrub(&payload, user_name().as_deref()),
            location: info.location().map(|l| short_location(l.file(), l.line())),
        });
        previous(info);
    }));
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() < 80 && id.ends_with(".json") && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
}

#[tauri::command]
pub fn crash_reports() -> Vec<CrashReport> {
    let Ok(rd) = std::fs::read_dir(dir()) else { return Vec::new() };
    let mut out: Vec<CrashReport> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if !valid_id(&name) {
                return None;
            }
            let mut r: CrashReport = serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok()?;
            r.id = name;
            Some(r)
        })
        .collect();
    out.sort_by(|a, b| b.at.cmp(&a.at));
    out
}

#[tauri::command]
pub fn crash_dismiss(id: String) -> Result<(), String> {
    if !valid_id(&id) {
        return Err("Not a crash report.".into());
    }
    let _ = std::fs::remove_file(dir().join(id));
    Ok(())
}

/// The interface's error boundary reports through this.
#[tauri::command]
pub fn crash_record(message: String) {
    write(&CrashReport {
        id: String::new(),
        at: crate::now_secs(),
        version: env!("CARGO_PKG_VERSION").into(),
        process: "interface".into(),
        message: scrub(&message, user_name().as_deref()),
        location: None,
    });
}

/// "Windows 11 (build 26300)": the only system detail a report carries.
fn windows_version() -> String {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    let k = winreg::RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    let build: u32 = k.as_ref().ok().and_then(|k| k.get_value::<String, _>("CurrentBuild").ok()).and_then(|b| b.parse().ok()).unwrap_or(0);
    let name = if build >= 22000 { "Windows 11" } else { "Windows 10" };
    format!("{name} (build {build})")
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[derive(Serialize)]
pub struct IssueDraft {
    /// Exactly what the GitHub issue will contain, shown before anything opens.
    pub title: String,
    pub body: String,
    pub url: String,
}

fn draft(title: String, body: String) -> IssueDraft {
    let url = format!("{REPO_ISSUES}?title={}&body={}", encode(&title), encode(&body));
    IssueDraft { title, body, url }
}

#[tauri::command]
pub fn crash_issue(id: String) -> Result<IssueDraft, String> {
    let r = crash_reports().into_iter().find(|r| r.id == id).ok_or("That report no longer exists.")?;
    let title = format!("Crash: {} stopped unexpectedly ({})", r.process, r.version);
    Ok(draft(title, issue_body(&r, &windows_version())))
}

/// A report for the watcher going over its footprint budget: just the numbers and versions.
#[tauri::command]
pub fn watcher_budget_issue(ram_mb: f64, peak_ram_mb: f64, cpu_pct: f64, profiles: usize, active: bool) -> IssueDraft {
    let title = format!("Watcher over budget ({})", env!("CARGO_PKG_VERSION"));
    let body = format!(
        "**FrameWise {}** · {}\n\nThe game-profile watcher went over its budget (15 MB, 0.2% CPU).\n\n| | |\n| --- | --- |\n| RAM now | {ram_mb:.1} MB |\n| RAM peak | {peak_ram_mb:.1} MB |\n| CPU (last minute) | {cpu_pct:.3}% |\n| Profiles on | {profiles} |\n| A profile was active | {} |\n\n_No file paths, user names or process lists._\n\n**What I was doing (optional):**\n",
        env!("CARGO_PKG_VERSION"),
        windows_version(),
        if active { "yes" } else { "no" },
    );
    draft(title, body)
}

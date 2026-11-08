//! The standby-list option on the Myths page: an advanced, one-off action, off by default and only
//! worth keeping if a before/after benchmark shows a gain (design document, "What we don't do").

use serde::Serialize;
use std::time::Duration;
use tauri::{AppHandle, Manager};

use crate::elevate::{ApiError, Elevated};
use framewise_checks::tweak::HelperRequest;

/// The design document's range where clearing has sometimes helped.
const HELPS_UP_TO_GB: f64 = 16.5;

#[derive(Serialize)]
pub struct StandbyStatus {
    pub standby_bytes: Option<u64>,
    pub ram_total_bytes: u64,
    /// 16 GB of RAM or less: the only PCs where clearing has been seen to help.
    pub may_help: bool,
}

#[tauri::command]
pub fn standby_status() -> StandbyStatus {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    // SAFETY: sized struct.
    let total = if unsafe { GlobalMemoryStatusEx(&mut m) }.is_ok() { m.ullTotalPhys } else { 0 };
    StandbyStatus {
        standby_bytes: framewise_checks::sensors::standby_bytes(),
        ram_total_bytes: total,
        may_help: total > 0 && (total as f64) / 1024f64.powi(3) <= HELPS_UP_TO_GB,
    }
}

#[derive(Serialize, serde::Deserialize)]
pub struct PurgeResult {
    pub before: Option<u64>,
    pub after: Option<u64>,
}

/// One UAC prompt: the helper empties the standby list once.
#[tauri::command]
pub async fn standby_purge(app: AppHandle) -> Result<PurgeResult, ApiError> {
    let hwnd = app.get_webview_window("main").and_then(|w| w.hwnd().ok()).map(|h| h.0 as isize).unwrap_or(0);
    tauri::async_runtime::spawn_blocking(move || {
        let mut h = Elevated::start(hwnd)?;
        let v = h.call(&HelperRequest::PurgeStandbyList, Duration::from_secs(60))?;
        serde_json::from_value(v).map_err(|e| ApiError::new("error", e.to_string()))
    })
    .await
    .map_err(|e| ApiError::new("error", e.to_string()))?
}

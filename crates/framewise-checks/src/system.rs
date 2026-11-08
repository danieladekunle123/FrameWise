//! Reads and writes the settings in [`crate::tweak`] on Windows.
//!
//! This is the only module that changes the system. Every function changes exactly one setting;
//! the app core's change pipeline (check, snapshot, apply, verify, log) decides when to call it.
//! Machine-wide tweaks are only called from inside the elevated helper.

use winreg::enums::{RegType, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE};
use winreg::{RegKey, RegValue};

use crate::tweak::{StartupSource, Tweak, TweakValue};

const GAMEBAR: &str = r"Software\Microsoft\GameBar";
const DX_PREFS: &str = r"Software\Microsoft\DirectX\UserGpuPreferences";
const GRAPHICS_DRIVERS: &str = r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers";
pub const HVCI: &str = r"SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity";
const POWER_SCHEMES: &str = r"SYSTEM\CurrentControlSet\Control\Power\User\PowerSchemes";
const APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved";

type R<T> = Result<T, String>;

/// Registry location of a registry-backed tweak: (root, key path, value name).
fn reg_location(t: &Tweak) -> Option<(RegKey, String, String)> {
    let hkcu = || RegKey::predef(HKEY_CURRENT_USER);
    let hklm = || RegKey::predef(HKEY_LOCAL_MACHINE);
    Some(match t {
        Tweak::GameMode => (hkcu(), GAMEBAR.into(), "AutoGameModeEnabled".into()),
        Tweak::WindowedOptimizations => (hkcu(), DX_PREFS.into(), "DirectXUserGlobalSettings".into()),
        Tweak::Hags => (hklm(), GRAPHICS_DRIVERS.into(), "HwSchMode".into()),
        Tweak::MemoryIntegrity => (hklm(), HVCI.into(), "Enabled".into()),
        Tweak::StartupEntry { source, name } => {
            let root = if matches!(source, StartupSource::UserRun | StartupSource::UserFolder) { hkcu() } else { hklm() };
            (root, format!(r"{APPROVED}\{}", source.approved_subkey()), name.clone())
        }
        _ => return None,
    })
}

fn reg_read(root: &RegKey, path: &str, name: &str) -> R<TweakValue> {
    let key = match root.open_subkey_with_flags(path, KEY_READ) {
        Ok(k) => k,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(TweakValue::Absent),
        Err(e) => return Err(format!("Couldn't read the setting: {e}")),
    };
    match key.get_raw_value(name) {
        Ok(v) => Ok(match v.vtype {
            RegType::REG_DWORD if v.bytes.len() >= 4 => {
                TweakValue::Dword(u32::from_le_bytes([v.bytes[0], v.bytes[1], v.bytes[2], v.bytes[3]]))
            }
            RegType::REG_SZ | RegType::REG_EXPAND_SZ => TweakValue::Text(utf16_bytes_to_string(&v.bytes)),
            _ => TweakValue::Bytes(v.bytes),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(TweakValue::Absent),
        Err(e) => Err(format!("Couldn't read the setting: {e}")),
    }
}

fn utf16_bytes_to_string(b: &[u8]) -> String {
    let w: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let end = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..end])
}

fn reg_write(root: &RegKey, path: &str, name: &str, v: &TweakValue) -> R<()> {
    let denied = |e: std::io::Error| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            "Windows refused the change (access denied).".to_string()
        } else {
            format!("Couldn't change the setting: {e}")
        }
    };
    if let TweakValue::Absent = v {
        let key = match root.open_subkey_with_flags(path, KEY_SET_VALUE) {
            Ok(k) => k,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(denied(e)),
        };
        return match key.delete_value(name) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(denied(e)),
        };
    }
    let (key, _) = root.create_subkey(path).map_err(denied)?;
    match v {
        TweakValue::Dword(d) => key.set_value(name, d).map_err(denied),
        TweakValue::Text(s) => key.set_value(name, s).map_err(denied),
        TweakValue::Bytes(b) => key.set_raw_value(name, &RegValue { vtype: RegType::REG_BINARY, bytes: b.clone() }).map_err(denied),
        TweakValue::Absent => unreachable!(),
    }
}

/// Reads a setting's current value.
pub fn read(t: &Tweak) -> R<TweakValue> {
    match t {
        Tweak::DisplayRefresh { device, width, height, .. } => {
            let cur = current_mode(device)?;
            if cur.dmPelsWidth != *width || cur.dmPelsHeight != *height {
                return Err(format!(
                    "The display's resolution changed (now {}×{}), so FrameWise won't touch its refresh rate. Use Settings > Display instead.",
                    cur.dmPelsWidth, cur.dmPelsHeight
                ));
            }
            Ok(TweakValue::Dword(cur.dmDisplayFrequency))
        }
        Tweak::PowerScheme => active_scheme().map(TweakValue::Text),
        Tweak::PowerMode => {
            let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
            Ok(match reg_read(&hklm, POWER_SCHEMES, "ActiveOverlayAcPowerScheme")? {
                TweakValue::Text(s) => TweakValue::Text(s.to_ascii_lowercase()),
                _ => TweakValue::Text(crate::tweak::MODE_BALANCED.into()),
            })
        }
        _ => {
            let (root, path, name) = reg_location(t).ok_or("unsupported setting")?;
            reg_read(&root, &path, &name)
        }
    }
}

/// Writes a setting. Callers verify with [`read`] afterwards.
pub fn write(t: &Tweak, v: &TweakValue) -> R<()> {
    match (t, v) {
        (Tweak::DisplayRefresh { device, width, height, .. }, TweakValue::Dword(hz)) => set_refresh(device, *width, *height, *hz),
        (Tweak::PowerScheme, TweakValue::Text(g)) => set_scheme(g),
        (Tweak::PowerMode, TweakValue::Text(g)) => set_power_mode(g),
        (Tweak::StartupEntry { source, name }, _) => {
            if !startup_entry_exists(*source, name) {
                return Err(format!("The startup entry '{name}' no longer exists."));
            }
            let (root, path, value) = reg_location(t).ok_or("unsupported setting")?;
            reg_write(&root, &path, &value, v)
        }
        (Tweak::DisplayRefresh { .. } | Tweak::PowerScheme | Tweak::PowerMode, _) => Err("Invalid value for this setting.".into()),
        _ => {
            let (root, path, name) = reg_location(t).ok_or("unsupported setting")?;
            reg_write(&root, &path, &name, v)
        }
    }
}

fn startup_entry_exists(source: StartupSource, name: &str) -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let run = |root: &RegKey, path: &str| {
        root.open_subkey(path).map(|k| k.enum_values().flatten().any(|(n, _)| n.eq_ignore_ascii_case(name))).unwrap_or(false)
    };
    match source {
        StartupSource::UserRun => run(&hkcu, r"Software\Microsoft\Windows\CurrentVersion\Run"),
        StartupSource::MachineRun => run(&hklm, r"Software\Microsoft\Windows\CurrentVersion\Run"),
        StartupSource::MachineRun32 => run(&hklm, r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run"),
        StartupSource::UserFolder | StartupSource::MachineFolder => crate::collect::startup_folders(&hkcu, &hklm)
            .into_iter()
            .filter(|(_, _, s)| *s == source)
            .any(|(dir, _, _)| dir.join(name).is_file()),
    }
}

/// HVCI can be locked on by UEFI (`Locked = 1`), in which case only the firmware can turn it off.
pub fn memory_integrity_locked() -> bool {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    matches!(reg_read(&hklm, HVCI, "Locked"), Ok(TweakValue::Dword(1)))
}

// ---------------------------------------------------------------------------------------------
// Display

fn current_mode(device: &str) -> R<windows::Win32::Graphics::Gdi::DEVMODEW> {
    use windows::core::HSTRING;
    use windows::Win32::Graphics::Gdi::{EnumDisplaySettingsW, DEVMODEW, ENUM_CURRENT_SETTINGS};
    let name = HSTRING::from(device);
    let mut dm = DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() };
    // SAFETY: name outlives the call and dm is a sized DEVMODEW.
    if !unsafe { EnumDisplaySettingsW(&name, ENUM_CURRENT_SETTINGS, &mut dm) }.as_bool() {
        return Err(format!("The display {device} isn't connected any more."));
    }
    Ok(dm)
}

/// Whether the display offers `hz` at this resolution (so a black screen from an unsupported mode
/// can't happen through FrameWise).
///
/// Fixes only ever target rates from Windows' normal list (see `fixes::plan`). The driver's raw list
/// is accepted too so Undo can always go back to the rate the display was running: with no monitor
/// information (dummy plugs, KVMs, streaming displays, custom resolutions) the normal list can be
/// just 60 Hz while the display really runs a raw-only mode such as 240 Hz.
fn mode_available(device: &str, width: u32, height: u32, bits: u32, hz: u32) -> bool {
    use windows::core::HSTRING;
    use windows::Win32::Graphics::Gdi::{EnumDisplaySettingsExW, DEVMODEW, EDS_RAWMODE, ENUM_DISPLAY_SETTINGS_FLAGS, ENUM_DISPLAY_SETTINGS_MODE};
    let name = HSTRING::from(device);
    for flags in [ENUM_DISPLAY_SETTINGS_FLAGS(0), EDS_RAWMODE] {
        for m in 0..5000u32 {
            let mut dm = DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() };
            // SAFETY: as above.
            if !unsafe { EnumDisplaySettingsExW(&name, ENUM_DISPLAY_SETTINGS_MODE(m), &mut dm, flags) }.as_bool() {
                break;
            }
            if dm.dmPelsWidth == width && dm.dmPelsHeight == height && dm.dmDisplayFrequency == hz && dm.dmBitsPerPel >= bits {
                return true;
            }
        }
    }
    false
}

/// Refresh rates a display offers at a resolution, highest first.
pub fn refresh_rates(device: &str, width: u32, height: u32) -> Vec<u32> {
    use windows::core::HSTRING;
    use windows::Win32::Graphics::Gdi::{EnumDisplaySettingsW, DEVMODEW, ENUM_DISPLAY_SETTINGS_MODE};
    let name = HSTRING::from(device);
    let mut v = Vec::new();
    for m in 0..5000u32 {
        let mut dm = DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() };
        // SAFETY: name outlives the call and dm is sized.
        if !unsafe { EnumDisplaySettingsW(&name, ENUM_DISPLAY_SETTINGS_MODE(m), &mut dm) }.as_bool() {
            break;
        }
        if dm.dmPelsWidth == width && dm.dmPelsHeight == height {
            v.push(dm.dmDisplayFrequency);
        }
    }
    v.sort_unstable_by(|a, b| b.cmp(a));
    v.dedup();
    v
}

fn set_refresh(device: &str, width: u32, height: u32, hz: u32) -> R<()> {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{
        ChangeDisplaySettingsExW, CDS_TEST, CDS_UPDATEREGISTRY, DISP_CHANGE_RESTART, DISP_CHANGE_SUCCESSFUL, DM_DISPLAYFREQUENCY,
        DM_PELSHEIGHT, DM_PELSWIDTH,
    };
    let mut dm = current_mode(device)?;
    if dm.dmPelsWidth != width || dm.dmPelsHeight != height {
        return Err("The display's resolution changed, so FrameWise won't touch its refresh rate.".into());
    }
    if dm.dmDisplayFrequency == hz {
        return Ok(());
    }
    if !mode_available(device, width, height, dm.dmBitsPerPel, hz) {
        return Err(format!("The display doesn't offer {hz} Hz at {width}×{height}."));
    }
    dm.dmDisplayFrequency = hz;
    dm.dmFields = DM_DISPLAYFREQUENCY | DM_PELSWIDTH | DM_PELSHEIGHT;
    let name = HSTRING::from(device);
    // SAFETY: name and dm outlive both calls.
    let test = unsafe { ChangeDisplaySettingsExW(&name, Some(&dm), HWND::default(), CDS_TEST, None) };
    if test != DISP_CHANGE_SUCCESSFUL {
        return Err(format!("Windows says the display can't use {hz} Hz (code {}).", test.0));
    }
    // CDS_UPDATEREGISTRY without CDS_GLOBAL: saved for this user only, like the Settings app.
    let r = unsafe { ChangeDisplaySettingsExW(&name, Some(&dm), HWND::default(), CDS_UPDATEREGISTRY, None) };
    if r == DISP_CHANGE_SUCCESSFUL || r == DISP_CHANGE_RESTART {
        Ok(())
    } else {
        Err(format!("Windows couldn't change the refresh rate (code {}).", r.0))
    }
}

// ---------------------------------------------------------------------------------------------
// Power

pub fn guid_to_string(g: &windows::core::GUID) -> String {
    format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        g.data1, g.data2, g.data3, g.data4[0], g.data4[1], g.data4[2], g.data4[3], g.data4[4], g.data4[5], g.data4[6], g.data4[7]
    )
}

pub fn parse_guid(s: &str) -> R<windows::core::GUID> {
    let hex: String = s.trim().trim_matches(|c| c == '{' || c == '}').chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 {
        return Err(format!("Not a GUID: {s}"));
    }
    u128::from_str_radix(&hex, 16).map(windows::core::GUID::from_u128).map_err(|_| format!("Not a GUID: {s}"))
}

pub fn active_scheme() -> R<String> {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::System::Power::PowerGetActiveScheme;
    use windows::Win32::System::Registry::HKEY;
    let mut p: *mut windows::core::GUID = std::ptr::null_mut();
    // SAFETY: Windows allocates the GUID; we copy it and free it with LocalFree.
    unsafe {
        let r = PowerGetActiveScheme(HKEY::default(), &mut p);
        if r.is_err() || p.is_null() {
            return Err(format!("Couldn't read the power plan (code {}).", r.0));
        }
        let g = *p;
        let _ = LocalFree(HLOCAL(p as *mut core::ffi::c_void));
        Ok(guid_to_string(&g))
    }
}

/// Empties the standby list (Windows' file cache in RAM). Needs admin rights: it enables
/// SeProfileSingleProcessPrivilege on this process's token, then asks the memory manager to purge
/// the list (NtSetSystemInformation(SystemMemoryListInformation, MemoryPurgeStandbyList)).
pub fn purge_standby_list() -> Result<(), String> {
    use windows::core::w;
    use windows::Win32::Foundation::{CloseHandle, HANDLE, LUID};
    use windows::Win32::Security::{AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY};
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    const SYSTEM_MEMORY_LIST_INFORMATION: u32 = 80;
    const MEMORY_PURGE_STANDBY_LIST: i32 = 4;
    // SAFETY: token handle closed below; structs sized for the calls.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token).map_err(|e| e.to_string())?;
        let mut luid = LUID::default();
        let looked = LookupPrivilegeValueW(None, w!("SeProfileSingleProcessPrivilege"), &mut luid);
        let tp = TOKEN_PRIVILEGES { PrivilegeCount: 1, Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }] };
        let adjusted = looked.is_ok() && AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None).is_ok();
        let not_all = windows::Win32::Foundation::GetLastError() == windows::Win32::Foundation::ERROR_NOT_ALL_ASSIGNED;
        let _ = CloseHandle(token);
        if !adjusted || not_all {
            return Err("Clearing the standby list needs admin rights.".into());
        }
        type F = unsafe extern "system" fn(u32, *const core::ffi::c_void, u32) -> i32;
        let m = GetModuleHandleW(w!("ntdll.dll")).map_err(|e| e.to_string())?;
        let f = GetProcAddress(m, windows::core::s!("NtSetSystemInformation")).ok_or("NtSetSystemInformation is missing.")?;
        let f: F = std::mem::transmute(f);
        let cmd = MEMORY_PURGE_STANDBY_LIST;
        let status = f(SYSTEM_MEMORY_LIST_INFORMATION, &cmd as *const i32 as *const _, 4);
        if status < 0 {
            return Err(format!("Windows refused to clear the standby list (status 0x{:08X}).", status as u32));
        }
    }
    Ok(())
}

/// The power plans on this PC as (GUID, name), for the profile editor.
pub fn power_plans() -> Vec<(String, String)> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let Ok(root) = hklm.open_subkey(POWER_SCHEMES) else { return Vec::new() };
    let mut out: Vec<(String, String)> = root
        .enum_keys()
        .flatten()
        .filter(|k| crate::tweak::is_known_scheme_or_guid(k))
        .map(|k| {
            let guid = k.to_ascii_lowercase();
            let friendly = root.open_subkey(&k).and_then(|s| s.get_value::<String, _>("FriendlyName")).unwrap_or_default();
            // Indirect names look like "@%SystemRoot%\system32\powrprof.dll,-12,Balanced".
            let fallback = friendly.rsplit(',').next().unwrap_or("").trim().to_string();
            let known = crate::tweak::scheme_name(&guid);
            let name = if known != "Custom plan" { known.to_string() } else if !fallback.is_empty() && !fallback.starts_with('@') { fallback } else { known.to_string() };
            (guid, name)
        })
        .collect();
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

fn set_scheme(guid: &str) -> R<()> {
    use windows::Win32::System::Power::PowerSetActiveScheme;
    use windows::Win32::System::Registry::HKEY;
    let g = parse_guid(guid)?;
    // SAFETY: g outlives the call.
    let r = unsafe { PowerSetActiveScheme(HKEY::default(), Some(&g)) };
    if r.is_ok() {
        Ok(())
    } else if r.0 == 2 {
        Err(format!("This PC doesn't have the {} plan.", crate::tweak::scheme_name(guid)))
    } else {
        Err(format!("Windows couldn't switch the power plan (code {}).", r.0))
    }
}

/// Sets the Windows 11 power mode through `powrprof!PowerSetActiveOverlayScheme`, the function the
/// Settings app's Power mode list uses. It isn't in the Windows SDK headers, so it's looked up at
/// run time and the fix is skipped where it doesn't exist.
fn set_power_mode(guid: &str) -> R<()> {
    use windows::core::{s, w};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    let g = parse_guid(guid)?;
    // SAFETY: powrprof.dll is a system DLL; the exported function takes the GUID by value and
    // returns a Win32 error code, per its use by the Settings app.
    unsafe {
        let lib = LoadLibraryW(w!("powrprof.dll")).map_err(|e| format!("Couldn't load the power library: {e}"))?;
        let Some(f) = GetProcAddress(lib, s!("PowerSetActiveOverlayScheme")) else {
            return Err("This version of Windows doesn't support changing the power mode. Use Settings > System > Power.".into());
        };
        let f: extern "system" fn(windows::core::GUID) -> u32 = std::mem::transmute(f);
        let r = f(g);
        if r == 0 {
            Ok(())
        } else {
            Err(format!("Windows couldn't change the power mode (code {r})."))
        }
    }
}

// ---------------------------------------------------------------------------------------------
// System Restore (helper only: needs admin)

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct RestorePointInfo {
    pub sequence: i64,
    /// Windows made one in the last 24 hours, so it reused that instead of creating a new one.
    pub reused: bool,
    pub description: String,
}

/// Creates a restore point. Windows allows one every 24 hours for apps; within that window it
/// returns the existing point, which is reported as `reused`.
pub fn create_restore_point(description: &str) -> Result<RestorePointInfo, (String, String)> {
    use windows::Win32::System::Restore::{
        SRSetRestorePointW, BEGIN_SYSTEM_CHANGE, END_SYSTEM_CHANGE, MODIFY_SETTINGS, RESTOREPOINTINFOW, STATEMGRSTATUS,
    };
    let latest_before = latest_restore_point();
    let mut info = RESTOREPOINTINFOW { dwEventType: BEGIN_SYSTEM_CHANGE, dwRestorePtType: MODIFY_SETTINGS, ..Default::default() };
    for (i, c) in description.encode_utf16().take(255).enumerate() {
        info.szDescription[i] = c;
    }
    let mut status = STATEMGRSTATUS::default();
    // SAFETY: both structs are valid for the call.
    let ok = unsafe { SRSetRestorePointW(&info, &mut status) }.as_bool();
    if !ok {
        return Err(match status.nStatus.0 {
            1058 => (
                "restore_point_off".into(),
                "System Restore is turned off on this PC, so a restore point can't be made.".into(),
            ),
            code => ("restore_point_failed".into(), format!("Windows couldn't create a restore point (code {code}).")),
        });
    }
    let seq = status.llSequenceNumber;
    let end = RESTOREPOINTINFOW { dwEventType: END_SYSTEM_CHANGE, dwRestorePtType: MODIFY_SETTINGS, llSequenceNumber: seq, ..Default::default() };
    let mut status2 = STATEMGRSTATUS::default();
    // SAFETY: as above.
    let _ = unsafe { SRSetRestorePointW(&end, &mut status2) };
    // A new point can take a few seconds to show up in WMI, so wait for ours before deciding.
    let mut latest = latest_restore_point();
    for _ in 0..20 {
        if is_our_point(seq, description, latest_before.as_ref(), latest.as_ref()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
        latest = latest_restore_point();
    }
    Ok(restore_point_outcome(seq, description, latest_before.as_ref(), latest))
}

/// Whether the newest point WMI shows is the one we just asked for: it has our sequence number, or
/// it's newer than the newest point before the call *and* has our description. A newer point with
/// another description was made by something else at the same moment (an installer, Windows
/// Update), so it doesn't count as ours.
fn is_our_point(seq: i64, description: &str, before: Option<&(i64, String)>, latest: Option<&(i64, String)>) -> bool {
    match latest {
        Some((s, _)) if *s == seq => true,
        Some((s, d)) => d == description && before.map_or(true, |b| *s > b.0),
        None => false,
    }
}

fn restore_point_outcome(seq: i64, description: &str, before: Option<&(i64, String)>, latest: Option<(i64, String)>) -> RestorePointInfo {
    match latest {
        // Ours appeared.
        Some((s, d)) if is_our_point(seq, description, before, Some(&(s, d.clone()))) => RestorePointInfo { sequence: s, reused: false, description: d },
        // Windows skipped it (one already made in the last 24 hours). It still hands back a new
        // sequence number that belongs to no point, so report the point that actually covers us.
        Some((s, d)) => RestorePointInfo { sequence: s, reused: true, description: d },
        // WMI can't be read, so Windows' own answer is all there is.
        None => RestorePointInfo { sequence: seq, reused: false, description: description.to_string() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_point_outcomes() {
        let ours = "FrameWise: before changes";
        let before = (447, "Installed an app".to_string());
        // New point appears with our sequence number.
        let r = restore_point_outcome(450, ours, Some(&before), Some((450, ours.into())));
        assert!(!r.reused && r.sequence == 450);
        // Windows skipped it: the newest point is still the old one.
        let r = restore_point_outcome(449, ours, Some(&before), Some(before.clone()));
        assert!(r.reused && r.sequence == 447 && r.description == "Installed an app");
        // Another app's point appeared at the same moment: not ours, so ours was skipped.
        let r = restore_point_outcome(2, ours, None, Some((1, "Installed something".into())));
        assert!(r.reused && r.sequence == 1);
        // First point on the PC (nothing before) with our description counts as ours.
        let r = restore_point_outcome(5, ours, None, Some((1, ours.into())));
        assert!(!r.reused && r.sequence == 1);
        // WMI unreadable: trust Windows.
        let r = restore_point_outcome(9, ours, None, None);
        assert!(!r.reused && r.sequence == 9);
    }
}

/// The newest restore point's sequence number and description, from WMI (`root\default`).
fn latest_restore_point() -> Option<(i64, String)> {
    use std::collections::HashMap;
    use wmi::{COMLibrary, Variant, WMIConnection};
    let com = COMLibrary::new().or_else(|_| unsafe { Ok::<_, wmi::WMIError>(COMLibrary::assume_initialized()) }).ok()?;
    let conn = WMIConnection::with_namespace_path(r"root\default", com).ok()?;
    let rows: Vec<HashMap<String, Variant>> = conn.raw_query("SELECT SequenceNumber, Description FROM SystemRestore").ok()?;
    rows.into_iter()
        .filter_map(|r| {
            let seq = match r.get("SequenceNumber")? {
                Variant::UI4(v) => *v as i64,
                Variant::I4(v) => *v as i64,
                Variant::UI8(v) => *v as i64,
                Variant::I8(v) => *v,
                _ => return None,
            };
            let desc = match r.get("Description") {
                Some(Variant::String(s)) => s.clone(),
                _ => String::new(),
            };
            Some((seq, desc))
        })
        .max_by_key(|x| x.0)
}

/// Seconds since the PC started, for "restart pending" checks.
pub fn uptime_secs() -> u64 {
    // SAFETY: no arguments.
    unsafe { windows::Win32::System::SystemInformation::GetTickCount64() / 1000 }
}

/// Current time as a Windows FILETIME (for `StartupApproved` values).
pub fn filetime_now() -> u64 {
    // SAFETY: no arguments.
    let ft = unsafe { windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime() };
    ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64
}

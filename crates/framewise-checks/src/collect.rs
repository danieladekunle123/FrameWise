//! Read-only system collection for Windows. Nothing here changes the system.
//!
//! Independent sources run on their own threads so a scan finishes in a few seconds.

use std::collections::HashMap;
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
use winreg::RegKey;
use wmi::{COMLibrary, Variant, WMIConnection};

use crate::snapshot::*;

/// Collect a full snapshot. Errors in one source are recorded and never stop the others.
pub fn collect() -> Snapshot {
    let collected_at = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);

    let (wmi_part, net, games, procs) = std::thread::scope(|sc| {
        let wmi_h = sc.spawn(collect_wmi);
        let net_h = sc.spawn(probe_network);
        let games_h = sc.spawn(collect_games);
        let procs_h = sc.spawn(collect_processes);
        (
            wmi_h.join().unwrap_or_else(|_| WmiPart::failed("WMI collector crashed")),
            net_h.join().ok().flatten(),
            games_h.join().unwrap_or_default(),
            procs_h.join().ok(),
        )
    });

    let mut errors = wmi_part.errors;
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    let os = read_os(&hklm);
    let cpu_name = reg_string(&hklm, r"HARDWARE\DESCRIPTION\System\CentralProcessor\0", "ProcessorNameString")
        .map(|s| s.trim().to_string());

    let displays = match collect_displays() {
        Ok(d) => Some(d),
        Err(e) => {
            errors.push(format!("displays: {e}"));
            None
        }
    };

    let drives = collect_drives();

    Snapshot {
        collected_at,
        os,
        is_laptop: battery_present(),
        cpu_name,
        memory_modules: wmi_part.memory,
        displays,
        gpus: wmi_part.gpus,
        drives: Some(drives),
        system_drive: std::env::var("SystemDrive").ok(),
        game_installs: Some(games),
        processes: procs,
        startup_entries: Some(collect_startup(&hkcu, &hklm)),
        power: PowerInfo {
            active_scheme: reg_string(&hklm, r"SYSTEM\CurrentControlSet\Control\Power\User\PowerSchemes", "ActivePowerScheme")
                .map(|s| s.to_ascii_lowercase()),
            ac_overlay: reg_string(&hklm, r"SYSTEM\CurrentControlSet\Control\Power\User\PowerSchemes", "ActiveOverlayAcPowerScheme")
                .map(|s| s.to_ascii_lowercase()),
        },
        game_mode_auto: reg_u32(&hkcu, r"Software\Microsoft\GameBar", "AutoGameModeEnabled"),
        hags_mode: reg_u32(&hklm, r"SYSTEM\CurrentControlSet\Control\GraphicsDrivers", "HwSchMode"),
        hags_caps: collect_hags_caps(),
        dx_user_global_settings: reg_string(&hkcu, r"Software\Microsoft\DirectX\UserGpuPreferences", "DirectXUserGlobalSettings"),
        hvci_enabled: reg_u32(
            &hklm,
            r"SYSTEM\CurrentControlSet\Control\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity",
            "Enabled",
        ),
        hvci_running: collect_hvci_running(),
        network: net,
        collection_errors: errors,
    }
}

// ---------------------------------------------------------------------------------------------
// Registry

pub(crate) fn reg_string(root: &RegKey, path: &str, name: &str) -> Option<String> {
    root.open_subkey(path).ok()?.get_value::<String, _>(name).ok()
}

pub(crate) fn reg_u32(root: &RegKey, path: &str, name: &str) -> Option<u32> {
    root.open_subkey(path).ok()?.get_value::<u32, _>(name).ok()
}

fn read_os(hklm: &RegKey) -> OsInfo {
    let path = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    OsInfo {
        build: reg_string(hklm, path, "CurrentBuildNumber").and_then(|b| b.parse().ok()).unwrap_or(0),
        display_version: reg_string(hklm, path, "DisplayVersion"),
    }
}

// ---------------------------------------------------------------------------------------------
// WMI (memory modules and graphics adapters)

struct WmiPart {
    memory: Option<Vec<MemoryModule>>,
    gpus: Option<Vec<GpuInfo>>,
    errors: Vec<String>,
}

impl WmiPart {
    fn failed(msg: &str) -> Self {
        WmiPart { memory: None, gpus: None, errors: vec![msg.to_string()] }
    }
}

fn v_str(row: &HashMap<String, Variant>, key: &str) -> String {
    match row.get(key) {
        Some(Variant::String(s)) => s.trim().to_string(),
        _ => String::new(),
    }
}

fn v_u64(row: &HashMap<String, Variant>, key: &str) -> u64 {
    match row.get(key) {
        Some(Variant::String(s)) => s.trim().parse().unwrap_or(0),
        Some(Variant::UI1(v)) => *v as u64,
        Some(Variant::UI2(v)) => *v as u64,
        Some(Variant::UI4(v)) => *v as u64,
        Some(Variant::UI8(v)) => *v,
        Some(Variant::I1(v)) => (*v).max(0) as u64,
        Some(Variant::I2(v)) => (*v).max(0) as u64,
        Some(Variant::I4(v)) => (*v).max(0) as u64,
        Some(Variant::I8(v)) => (*v).max(0) as u64,
        _ => 0,
    }
}

fn collect_wmi() -> WmiPart {
    let con = match COMLibrary::new().and_then(WMIConnection::new) {
        Ok(c) => c,
        Err(e) => return WmiPart::failed(&format!("WMI unavailable: {e}")),
    };
    let mut errors = Vec::new();

    let memory = match con.raw_query::<HashMap<String, Variant>>(
        "SELECT Capacity, Speed, ConfiguredClockSpeed, Manufacturer, PartNumber, BankLabel, DeviceLocator, SMBIOSMemoryType FROM Win32_PhysicalMemory",
    ) {
        Ok(rows) => Some(
            rows.iter()
                .map(|r| MemoryModule {
                    capacity_bytes: v_u64(r, "Capacity"),
                    speed: v_u64(r, "Speed") as u32,
                    configured_speed: v_u64(r, "ConfiguredClockSpeed") as u32,
                    manufacturer: v_str(r, "Manufacturer"),
                    part_number: v_str(r, "PartNumber"),
                    bank_label: v_str(r, "BankLabel"),
                    device_locator: v_str(r, "DeviceLocator"),
                    smbios_memory_type: v_u64(r, "SMBIOSMemoryType") as u32,
                })
                .collect(),
        ),
        Err(e) => {
            errors.push(format!("memory modules: {e}"));
            None
        }
    };

    let gpus = match con.raw_query::<HashMap<String, Variant>>(
        "SELECT Name, AdapterCompatibility, DriverVersion, DriverDate, PNPDeviceID FROM Win32_VideoController",
    ) {
        Ok(rows) => Some(
            rows.iter()
                .map(|r| {
                    let date = v_str(r, "DriverDate");
                    GpuInfo {
                        name: v_str(r, "Name"),
                        vendor: v_str(r, "AdapterCompatibility"),
                        driver_version: v_str(r, "DriverVersion"),
                        driver_date: if date.len() >= 8 { Some(date[..8].to_string()) } else { None },
                        pnp_device_id: v_str(r, "PNPDeviceID"),
                    }
                })
                .collect(),
        ),
        Err(e) => {
            errors.push(format!("graphics adapters: {e}"));
            None
        }
    };

    WmiPart { memory, gpus, errors }
}

// ---------------------------------------------------------------------------------------------
// Displays

fn wide_to_string(w: &[u16]) -> String {
    let end = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..end]).trim().to_string()
}

pub fn collect_displays() -> Result<Vec<DisplayInfo>, String> {
    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayDevicesW, EnumDisplaySettingsW, DEVMODEW, DISPLAY_DEVICEW, DISPLAY_DEVICE_ATTACHED_TO_DESKTOP,
        DISPLAY_DEVICE_PRIMARY_DEVICE, ENUM_CURRENT_SETTINGS, ENUM_DISPLAY_SETTINGS_MODE,
    };

    let mut out = Vec::new();
    let mut i = 0u32;
    loop {
        let mut dd = DISPLAY_DEVICEW { cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32, ..Default::default() };
        // SAFETY: dd is a properly sized, initialised DISPLAY_DEVICEW.
        if !unsafe { EnumDisplayDevicesW(PCWSTR::null(), i, &mut dd, 0) }.as_bool() {
            break;
        }
        i += 1;
        if dd.StateFlags & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP == 0 {
            continue;
        }
        let name_ptr = PCWSTR(dd.DeviceName.as_ptr());
        let mut cur = DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() };
        // SAFETY: name_ptr points into dd, which outlives the call; cur is sized.
        if !unsafe { EnumDisplaySettingsW(name_ptr, ENUM_CURRENT_SETTINGS, &mut cur) }.as_bool() {
            continue;
        }
        let (w, h, hz) = (cur.dmPelsWidth, cur.dmPelsHeight, cur.dmDisplayFrequency);
        let mut max_at_res = hz;
        let mut max_any = hz;
        let mut m = 0u32;
        loop {
            let mut dm = DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() };
            // SAFETY: as above.
            if !unsafe { EnumDisplaySettingsW(name_ptr, ENUM_DISPLAY_SETTINGS_MODE(m), &mut dm) }.as_bool() {
                break;
            }
            m += 1;
            if m > 5000 {
                break;
            }
            // Ignore modes with fewer colours than the current one (legacy 8/16-bit modes).
            if dm.dmBitsPerPel < cur.dmBitsPerPel {
                continue;
            }
            max_any = max_any.max(dm.dmDisplayFrequency);
            if dm.dmPelsWidth == w && dm.dmPelsHeight == h {
                max_at_res = max_at_res.max(dm.dmDisplayFrequency);
            }
        }
        let mut mon = DISPLAY_DEVICEW { cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32, ..Default::default() };
        // SAFETY: as above.
        let monitor = if unsafe { EnumDisplayDevicesW(name_ptr, 0, &mut mon, 0) }.as_bool() {
            wide_to_string(&mon.DeviceString)
        } else {
            String::new()
        };
        out.push(DisplayInfo {
            device_name: wide_to_string(&dd.DeviceName),
            adapter: wide_to_string(&dd.DeviceString),
            monitor,
            primary: dd.StateFlags & DISPLAY_DEVICE_PRIMARY_DEVICE != 0,
            width: w,
            height: h,
            current_hz: hz,
            max_hz_at_resolution: max_at_res,
            max_hz_any: max_any,
        });
    }
    if out.is_empty() {
        return Err("no active displays found".into());
    }
    Ok(out)
}

/// Whether Memory Integrity (HVCI) is running right now, from Device Guard's WMI class (readable
/// without admin rights). None if Windows doesn't report it.
fn collect_hvci_running() -> Option<bool> {
    use std::collections::HashMap;
    // SAFETY: if COM is already initialised on this thread in another mode, keep using it.
    let com = COMLibrary::new().or_else(|_| Ok::<_, wmi::WMIError>(unsafe { COMLibrary::assume_initialized() })).ok()?;
    let conn = WMIConnection::with_namespace_path(r"root\Microsoft\Windows\DeviceGuard", com).ok()?;
    let rows: Vec<HashMap<String, Variant>> = conn.raw_query("SELECT SecurityServicesRunning FROM Win32_DeviceGuard").ok()?;
    let row = rows.into_iter().next()?;
    match row.get("SecurityServicesRunning")? {
        Variant::Array(v) => Some(v.iter().any(|x| matches!(x, Variant::UI4(2) | Variant::I4(2) | Variant::UI2(2) | Variant::I2(2) | Variant::UI1(2)))),
        Variant::Null | Variant::Empty => Some(false),
        _ => None,
    }
}

/// Asks the graphics kernel whether hardware-accelerated GPU scheduling is supported and enabled.
fn collect_hags_caps() -> Option<HagsCaps> {
    use windows::Wdk::Graphics::Direct3D::{
        D3DKMTCloseAdapter, D3DKMTEnumAdapters2, D3DKMTQueryAdapterInfo, D3DKMT_ADAPTERINFO, D3DKMT_CLOSEADAPTER,
        D3DKMT_ENUMADAPTERS2, D3DKMT_QUERYADAPTERINFO, KMTQAITYPE_WDDM_2_7_CAPS,
    };
    let mut e = D3DKMT_ENUMADAPTERS2::default();
    // SAFETY: first call with a null array only fills in NumAdapters.
    unsafe { D3DKMTEnumAdapters2(&mut e) }.ok().ok()?;
    let mut adapters = vec![D3DKMT_ADAPTERINFO::default(); e.NumAdapters as usize];
    e.pAdapters = adapters.as_mut_ptr();
    // SAFETY: pAdapters points to NumAdapters writable entries.
    unsafe { D3DKMTEnumAdapters2(&mut e) }.ok().ok()?;
    adapters.truncate(e.NumAdapters as usize);

    let (mut any, mut supported, mut enabled) = (false, false, false);
    for a in &adapters {
        // D3DKMT_WDDM_2_7_CAPS is a 32-bit bitfield: bit 0 HwSchSupported, bit 1 HwSchEnabled.
        let mut caps: u32 = 0;
        let mut q = D3DKMT_QUERYADAPTERINFO {
            hAdapter: a.hAdapter,
            Type: KMTQAITYPE_WDDM_2_7_CAPS,
            pPrivateDriverData: &mut caps as *mut u32 as *mut core::ffi::c_void,
            PrivateDriverDataSize: std::mem::size_of::<u32>() as u32,
        };
        // SAFETY: q points to a 4-byte buffer matching the caps struct size.
        if unsafe { D3DKMTQueryAdapterInfo(&mut q) }.is_ok() {
            any = true;
            if caps & 1 != 0 {
                supported = true;
                enabled |= caps & 2 != 0;
            }
        }
        // SAFETY: closing a handle returned by D3DKMTEnumAdapters2.
        let _ = unsafe { D3DKMTCloseAdapter(&D3DKMT_CLOSEADAPTER { hAdapter: a.hAdapter }) };
    }
    any.then_some(HagsCaps { supported, enabled })
}

// ---------------------------------------------------------------------------------------------
// Power / battery

fn battery_present() -> Option<bool> {
    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    let mut sps = SYSTEM_POWER_STATUS::default();
    // SAFETY: sps is a valid out pointer.
    unsafe { GetSystemPowerStatus(&mut sps) }.ok()?;
    match sps.BatteryFlag {
        128 => Some(false), // no system battery
        255 => None,        // unknown
        _ => Some(true),
    }
}

// ---------------------------------------------------------------------------------------------
// Drives, processes

fn collect_drives() -> Vec<DriveInfo> {
    use sysinfo::{DiskKind, Disks};
    Disks::new_with_refreshed_list()
        .iter()
        .map(|d| DriveInfo {
            mount: d.mount_point().to_string_lossy().to_string(),
            media: match d.kind() {
                DiskKind::SSD => MediaType::Ssd,
                DiskKind::HDD => MediaType::Hdd,
                _ => MediaType::Unknown,
            },
            total_bytes: d.total_space(),
            free_bytes: d.available_space(),
        })
        .collect()
}

fn collect_processes() -> Vec<String> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::new());
    let mut names: Vec<String> =
        sys.processes().values().map(|p| p.name().to_string_lossy().to_ascii_lowercase()).collect();
    names.sort();
    names.dedup();
    names
}

// ---------------------------------------------------------------------------------------------
// Startup entries

/// `StartupApproved` values: the first byte is even when enabled, odd when disabled.
fn approved_map(root: &RegKey, sub: &str) -> HashMap<String, bool> {
    let path = format!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\{sub}");
    let mut map = HashMap::new();
    if let Ok(key) = root.open_subkey(path) {
        for (name, value) in key.enum_values().flatten() {
            let enabled = value.bytes.first().map(|b| b & 1 == 0).unwrap_or(true);
            map.insert(name.to_ascii_lowercase(), enabled);
        }
    }
    map
}

pub(crate) fn collect_startup(hkcu: &RegKey, hklm: &RegKey) -> Vec<StartupEntry> {
    use crate::tweak::StartupSource;
    let mut out = Vec::new();
    let run = r"Software\Microsoft\Windows\CurrentVersion\Run";
    let run32 = r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run";
    let sources: [(&RegKey, &str, StartupSource); 3] = [
        (hkcu, run, StartupSource::UserRun),
        (hklm, run, StartupSource::MachineRun),
        (hklm, run32, StartupSource::MachineRun32),
    ];
    for (root, path, source) in sources {
        let approved = approved_map(root, source.approved_subkey());
        if let Ok(key) = root.open_subkey(path) {
            for (name, value) in key.enum_values().flatten() {
                if name.is_empty() {
                    continue;
                }
                let enabled = approved.get(&name.to_ascii_lowercase()).copied().unwrap_or(true);
                out.push(StartupEntry {
                    name: name.clone(),
                    location: source.label().to_string(),
                    enabled,
                    source: Some(source),
                    value_name: name,
                    command: crate::tweak::startup_display_command(&value.to_string()),
                });
            }
        }
    }
    for (dir, root, source) in startup_folders(hkcu, hklm) {
        let approved = approved_map(root, source.approved_subkey());
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let file = e.file_name().to_string_lossy().to_string();
                if file.eq_ignore_ascii_case("desktop.ini") {
                    continue;
                }
                let enabled = approved.get(&file.to_ascii_lowercase()).copied().unwrap_or(true);
                out.push(StartupEntry {
                    name: crate::tweak::startup_display_name(&file).to_string(),
                    location: source.label().to_string(),
                    enabled,
                    source: Some(source),
                    command: e.path().to_string_lossy().to_string(),
                    value_name: file,
                });
            }
        }
    }
    out
}

pub(crate) fn startup_folders<'a>(hkcu: &'a RegKey, hklm: &'a RegKey) -> Vec<(PathBuf, &'a RegKey, crate::tweak::StartupSource)> {
    use crate::tweak::StartupSource;
    let mut v = Vec::new();
    if let Some(p) = std::env::var_os("APPDATA") {
        v.push((PathBuf::from(p).join(r"Microsoft\Windows\Start Menu\Programs\Startup"), hkcu, StartupSource::UserFolder));
    }
    if let Some(p) = std::env::var_os("ProgramData") {
        v.push((PathBuf::from(p).join(r"Microsoft\Windows\Start Menu\Programs\StartUp"), hklm, StartupSource::MachineFolder));
    }
    v
}

/// Startup entries as they are now, for the "choose apps to disable" list.
pub fn startup_entries() -> Vec<StartupEntry> {
    collect_startup(&RegKey::predef(HKEY_CURRENT_USER), &RegKey::predef(HKEY_LOCAL_MACHINE))
}

// ---------------------------------------------------------------------------------------------
// Game libraries (Steam, Epic)

use crate::profiles::vdf_values;


fn collect_games() -> Vec<GameInstall> {
    let mut games = Vec::new();

    // Steam
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Some(steam) = reg_string(&hkcu, r"Software\Valve\Steam", "SteamPath") {
        let steam = PathBuf::from(steam.replace('/', "\\"));
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
                let fname = e.file_name().to_string_lossy().to_string();
                if !(fname.starts_with("appmanifest_") && fname.ends_with(".acf")) {
                    continue;
                }
                let Ok(c) = std::fs::read_to_string(e.path()) else { continue };
                let name = vdf_values(&c, "name").into_iter().next().unwrap_or_default();
                let dir = vdf_values(&c, "installdir").into_iter().next().unwrap_or_default();
                if name.is_empty() || dir.is_empty() || name.contains("Steamworks Common Redistributables") {
                    continue;
                }
                games.push(GameInstall {
                    name,
                    launcher: "Steam".into(),
                    path: apps.join("common").join(dir).to_string_lossy().to_string(),
                });
            }
        }
    }

    // Epic Games
    if let Some(pd) = std::env::var_os("ProgramData") {
        let manifests = PathBuf::from(pd).join(r"Epic\EpicGamesLauncher\Data\Manifests");
        if let Ok(rd) = std::fs::read_dir(manifests) {
            for e in rd.flatten() {
                if e.path().extension().and_then(|x| x.to_str()) != Some("item") {
                    continue;
                }
                let Ok(c) = std::fs::read_to_string(e.path()) else { continue };
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&c) else { continue };
                let name = v["DisplayName"].as_str().unwrap_or_default().to_string();
                let path = v["InstallLocation"].as_str().unwrap_or_default().to_string();
                let is_game = v["AppCategories"]
                    .as_array()
                    .map(|a| a.iter().any(|x| x.as_str() == Some("games")))
                    .unwrap_or(true);
                if !name.is_empty() && !path.is_empty() && is_game {
                    games.push(GameInstall { name, launcher: "Epic".into(), path });
                }
            }
        }
    }

    games.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    games
}

// ---------------------------------------------------------------------------------------------
// Network

/// Times TCP connections to a public DNS resolver on port 443. Only a handshake is made; no data
/// is sent.
fn probe_network() -> Option<NetworkProbe> {
    // All samples go to one target so differences between servers don't count as jitter.
    // The second target is only used if the first can't be reached at all.
    let mut last = None;
    for target in ["1.1.1.1:443", "8.8.8.8:443"] {
        let addr: SocketAddr = target.parse().ok()?;
        let mut samples = Vec::new();
        let mut failures = 0;
        for _ in 0..8 {
            let start = Instant::now();
            match TcpStream::connect_timeout(&addr, Duration::from_millis(1500)) {
                Ok(s) => {
                    samples.push(start.elapsed().as_secs_f64() * 1000.0);
                    drop(s);
                }
                Err(_) => failures += 1,
            }
            if failures >= 3 && samples.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(60));
        }
        let probe = NetworkProbe { target: target.trim_end_matches(":443").into(), samples_ms: samples, failures };
        if !probe.samples_ms.is_empty() {
            return Some(probe);
        }
        last = Some(probe);
    }
    last
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vdf_parsing() {
        let c = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\SteamLibrary\"\n\t\t\"label\"\t\t\"\"\n\t}\n}";
        assert_eq!(vdf_values(c, "path"), vec!["D:\\SteamLibrary".to_string()]);
    }
}

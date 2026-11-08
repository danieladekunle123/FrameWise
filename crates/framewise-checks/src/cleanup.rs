//! Cleanup: junk that takes real space, measured before anything is removed.
//!
//! Each category's folders are fixed here; a request only names the category, so neither the
//! interface nor anything talking to the helper can point a delete at another folder. Links and
//! junctions are never followed. Temp files newer than a day are left alone because an app may
//! still be using them.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CleanCategory {
    UserTemp,
    WindowsTemp,
    CrashDumps,
    SystemCrashDumps,
    UpdateDownloads,
    ShaderCache,
    DriverPackages,
}

pub const ALL_CATEGORIES: &[CleanCategory] = &[
    CleanCategory::UserTemp,
    CleanCategory::WindowsTemp,
    CleanCategory::CrashDumps,
    CleanCategory::SystemCrashDumps,
    CleanCategory::UpdateDownloads,
    CleanCategory::DriverPackages,
    CleanCategory::ShaderCache,
];

#[derive(Serialize, Clone, Debug)]
pub struct CategoryInfo {
    pub id: CleanCategory,
    pub title: &'static str,
    pub description: &'static str,
    /// Machine-wide folders: the elevated helper does the work, after a UAC prompt.
    pub needs_admin: bool,
    /// Moved to the Recycle Bin (unless the user chooses to delete permanently).
    pub recyclable: bool,
    /// Ticked by default in the interface.
    pub default_selected: bool,
}

impl CleanCategory {
    pub fn info(self) -> CategoryInfo {
        use CleanCategory::*;
        let (title, description, needs_admin, recyclable, default_selected) = match self {
            UserTemp => ("Your temporary files", "Files apps left in your Temp folder. Anything changed in the last 24 hours is kept.", false, true, true),
            WindowsTemp => ("Windows temporary files", "The Windows Temp folder. Anything changed in the last 24 hours is kept.", true, false, true),
            CrashDumps => ("App crash dumps and error reports", "Memory dumps and reports saved when apps crashed. Only useful if you're sending them to a developer.", false, true, true),
            SystemCrashDumps => ("Windows crash dumps", "Memory dumps from blue screens and driver resets, and queued Windows error reports.", true, false, true),
            UpdateDownloads => ("Windows Update downloads", "Update files Windows already downloaded more than 10 days ago. Windows downloads them again if it still needs one.", true, false, true),
            DriverPackages => ("Old graphics driver packages", "Previous GPU drivers kept in the driver store. The driver in use and the newest older one (for Roll Back Driver) are kept.", true, false, true),
            ShaderCache => ("GPU shader cache", "Compiled shaders. Only worth clearing if a game stutters after a driver update; games rebuild it, so the first launch is slower.", false, true, false),
        };
        CategoryInfo { id: self, title, description, needs_admin, recyclable, default_selected }
    }

    /// Items changed more recently than this are kept.
    pub fn min_age_secs(self) -> u64 {
        match self {
            CleanCategory::UserTemp | CleanCategory::WindowsTemp => 24 * 3600,
            CleanCategory::UpdateDownloads => 10 * 24 * 3600,
            _ => 0,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct DriverPackage {
    /// Published name in the driver store, for example `oem42.inf`.
    pub inf: String,
    pub provider: String,
    pub version: String,
    /// `yyyy-mm-dd`, from the package's DriverVer.
    pub date: String,
    pub bytes: u64,
    pub in_use: bool,
    /// Kept as the Roll Back Driver target.
    pub kept_for_rollback: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CategoryScan {
    pub id: Option<CleanCategory>,
    /// Bytes that would be removed.
    pub bytes: u64,
    pub items: u64,
    /// Items left alone because they changed recently.
    pub recent_items: u64,
    pub drivers: Vec<DriverPackage>,
    /// Some folders couldn't be read without admin rights, so the size is incomplete.
    #[serde(default)]
    pub needs_permission: bool,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CleanResult {
    pub id: Option<CleanCategory>,
    pub freed_bytes: u64,
    pub removed_items: u64,
    /// In use or access denied; left in place.
    pub skipped_items: u64,
    pub recycled: bool,
    pub error: Option<String>,
}

/// Parses one INF `[Version]` value, resolving a `%Token%` from `[Strings]`.
pub fn inf_value(inf: &str, key: &str) -> Option<String> {
    let mut section = String::new();
    let mut raw = None;
    let mut strings = std::collections::HashMap::new();
    for line in inf.lines() {
        let line = line.split(';').next().unwrap_or("").trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_ascii_lowercase();
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim().trim_matches('"').to_string());
        if section == "version" && k.eq_ignore_ascii_case(key) && raw.is_none() {
            raw = Some(v);
        } else if section == "strings" {
            strings.insert(k.to_ascii_lowercase(), v);
        }
    }
    let raw = raw?;
    if raw.len() > 2 && raw.starts_with('%') && raw.ends_with('%') {
        return strings.get(&raw[1..raw.len() - 1].to_ascii_lowercase()).cloned().or(Some(raw));
    }
    Some(raw)
}

/// `DriverVer = 09/04/2026,32.0.15.8142` → (`2026-09-04`, `32.0.15.8142`).
pub fn parse_driver_ver(v: &str) -> (String, String) {
    let (date, ver) = v.split_once(',').unwrap_or((v, ""));
    let parts: Vec<&str> = date.trim().split('/').collect();
    let date = if parts.len() == 3 {
        format!("{}-{:0>2}-{:0>2}", parts[2], parts[0], parts[1])
    } else {
        date.trim().to_string()
    };
    (date, ver.trim().to_string())
}

/// Marks which unused packages to keep: the newest unused one per provider, as the rollback
/// driver. Everything else that isn't in use may be removed.
pub fn mark_rollback(packages: &mut [DriverPackage]) {
    let mut providers: Vec<String> = packages.iter().map(|p| p.provider.to_ascii_lowercase()).collect();
    providers.sort();
    providers.dedup();
    for prov in providers {
        let newest = packages
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.in_use && p.provider.to_ascii_lowercase() == prov)
            .max_by(|a, b| a.1.date.cmp(&b.1.date).then_with(|| a.1.version.cmp(&b.1.version)))
            .map(|(i, _)| i);
        if let Some(i) = newest {
            packages[i].kept_for_rollback = true;
        }
    }
}

/// Only GPU vendors' packages count. Other Display-class packages are virtual displays for remote
/// access, streaming and screen capture (TeamViewer, Parsec, Sunshine, DisplayLink and so on), and
/// removing those breaks the app that installed them.
pub fn is_gpu_vendor(provider: &str) -> bool {
    let p = provider.trim().to_ascii_lowercase();
    ["nvidia", "advanced micro devices", "amd", "ati technologies", "intel"]
        .iter()
        .any(|v| p == *v || p.starts_with(&format!("{v} ")) || p.starts_with(&format!("{v},")))
}

pub fn removable(p: &DriverPackage) -> bool {
    !p.in_use && !p.kept_for_rollback
}

#[cfg(windows)]
pub use win::*;

#[cfg(windows)]
mod win {
    use super::*;
    use std::os::windows::fs::MetadataExt;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

    fn env_path(var: &str) -> Option<PathBuf> {
        std::env::var_os(var).map(PathBuf::from)
    }

    /// The folders (or single files) a category cleans. Their contents are cleaned, never the
    /// folder itself.
    pub fn roots(cat: CleanCategory) -> Vec<PathBuf> {
        use CleanCategory::*;
        let windir = env_path("SystemRoot").unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let local = env_path("LOCALAPPDATA");
        let mut v: Vec<PathBuf> = Vec::new();
        match cat {
            UserTemp => {
                // %TEMP% for a normal user is under their profile. Never clean a Temp that
                // resolves to the Windows folder (as it can for an elevated or service process).
                let t = std::env::temp_dir();
                if !t.starts_with(&windir) {
                    v.push(t);
                }
            }
            WindowsTemp => v.push(windir.join("Temp")),
            CrashDumps => {
                if let Some(l) = &local {
                    v.push(l.join("CrashDumps"));
                    v.push(l.join(r"Microsoft\Windows\WER\ReportArchive"));
                    v.push(l.join(r"Microsoft\Windows\WER\ReportQueue"));
                }
            }
            SystemCrashDumps => {
                v.push(windir.join("Minidump"));
                v.push(windir.join("MEMORY.DMP"));
                v.push(windir.join("LiveKernelReports"));
                if let Some(pd) = env_path("ProgramData") {
                    v.push(pd.join(r"Microsoft\Windows\WER\ReportArchive"));
                    v.push(pd.join(r"Microsoft\Windows\WER\ReportQueue"));
                }
            }
            UpdateDownloads => v.push(windir.join(r"SoftwareDistribution\Download")),
            ShaderCache => {
                if let Some(l) = &local {
                    for sub in [
                        "D3DSCache",
                        r"NVIDIA\DXCache",
                        r"NVIDIA\GLCache",
                        r"AMD\DxCache",
                        r"AMD\DxcCache",
                        r"AMD\VkCache",
                        r"AMD\GLCache",
                        r"Intel\ShaderCache",
                    ] {
                        v.push(l.join(sub));
                    }
                    // Newer NVIDIA drivers keep the cache in LocalLow.
                    if let Some(parent) = l.parent() {
                        v.push(parent.join(r"LocalLow\NVIDIA\PerDriverVersion\DXCache"));
                        v.push(parent.join(r"LocalLow\NVIDIA\PerDriverVersion\GLCache"));
                    }
                }
            }
            DriverPackages => {}
        }
        v
    }

    fn is_link(md: &std::fs::Metadata) -> bool {
        md.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }

    /// Size and newest modification time of a file or folder tree, without following links.
    fn measure(path: &Path) -> (u64, SystemTime) {
        let Ok(md) = std::fs::symlink_metadata(path) else { return (0, SystemTime::UNIX_EPOCH) };
        let mtime = md.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        if is_link(&md) {
            return (0, mtime);
        }
        if !md.is_dir() {
            return (md.len(), mtime);
        }
        let (mut size, mut newest) = (0u64, mtime);
        let mut stack = vec![path.to_path_buf()];
        let mut visited = 0u32;
        while let Some(dir) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                visited += 1;
                if visited > 2_000_000 {
                    return (size, newest);
                }
                let Ok(md) = e.metadata() else { continue };
                if let Ok(m) = md.modified() {
                    newest = newest.max(m);
                }
                if is_link(&md) {
                    continue;
                }
                if md.is_dir() {
                    stack.push(e.path());
                } else {
                    size += md.len();
                }
            }
        }
        (size, newest)
    }

    struct Item {
        path: PathBuf,
        bytes: u64,
    }

    /// Top-level entries of each root that are old enough to remove. Links are skipped.
    /// Returns (items, recently changed count, some root unreadable without admin).
    fn eligible(cat: CleanCategory) -> (Vec<Item>, u64, bool) {
        let cutoff = SystemTime::now() - Duration::from_secs(cat.min_age_secs());
        let mut items = Vec::new();
        let mut recent = 0;
        let mut denied = false;
        for root in roots(cat) {
            let md = match std::fs::symlink_metadata(&root) {
                Ok(md) => md,
                Err(e) => {
                    denied |= e.kind() == std::io::ErrorKind::PermissionDenied;
                    continue;
                }
            };
            if is_link(&md) {
                continue;
            }
            let entries: Vec<PathBuf> = if md.is_dir() {
                match std::fs::read_dir(&root) {
                    Ok(rd) => rd.flatten().map(|e| e.path()).collect(),
                    Err(e) => {
                        denied |= e.kind() == std::io::ErrorKind::PermissionDenied;
                        continue;
                    }
                }
            } else {
                vec![root.clone()]
            };
            for path in entries {
                let Ok(md) = std::fs::symlink_metadata(&path) else { continue };
                if is_link(&md) {
                    continue;
                }
                let (bytes, newest) = measure(&path);
                if newest > cutoff {
                    recent += 1;
                } else {
                    items.push(Item { path, bytes });
                }
            }
        }
        (items, recent, denied)
    }

    pub fn scan(cat: CleanCategory) -> CategoryScan {
        if cat == CleanCategory::DriverPackages {
            return match driver_packages() {
                Ok(drivers) => CategoryScan {
                    id: Some(cat),
                    bytes: drivers.iter().filter(|p| removable(p)).map(|p| p.bytes).sum(),
                    items: drivers.iter().filter(|p| removable(p)).count() as u64,
                    recent_items: 0,
                    drivers,
                    needs_permission: false,
                    error: None,
                },
                Err(e) => CategoryScan { id: Some(cat), error: Some(e), ..Default::default() },
            };
        }
        let (items, recent, denied) = eligible(cat);
        CategoryScan {
            id: Some(cat),
            bytes: items.iter().map(|i| i.bytes).sum(),
            items: items.len() as u64,
            recent_items: recent,
            drivers: vec![],
            needs_permission: denied,
            error: None,
        }
    }

    /// Cleans one file-based category. `recycle` moves items to the Recycle Bin.
    pub fn clean(cat: CleanCategory, recycle: bool) -> CleanResult {
        let (items, _, _) = eligible(cat);
        let recycle = recycle && cat.info().recyclable;
        let mut res = CleanResult { id: Some(cat), recycled: recycle, ..Default::default() };
        if recycle {
            // Recycle in batches; if a batch fails (one item in use), fall back to one at a time.
            for chunk in items.chunks(64) {
                let paths: Vec<&Path> = chunk.iter().map(|i| i.path.as_path()).collect();
                if recycle_paths(&paths).is_err() {
                    for p in &paths {
                        let _ = recycle_paths(&[p]);
                    }
                }
                tally(chunk, &mut res);
            }
        } else {
            for item in &items {
                let md = std::fs::symlink_metadata(&item.path);
                let _ = match md {
                    Ok(m) if is_link(&m) => continue,
                    Ok(m) if m.is_dir() => std::fs::remove_dir_all(&item.path),
                    Ok(_) => std::fs::remove_file(&item.path),
                    Err(_) => continue,
                };
            }
            tally(&items, &mut res);
        }
        res
    }

    /// Counts what actually went: an item partly removed (some files in use) counts the bytes
    /// that went and as skipped.
    fn tally(items: &[Item], res: &mut CleanResult) {
        for i in items {
            if std::fs::symlink_metadata(&i.path).is_err() {
                res.removed_items += 1;
                res.freed_bytes += i.bytes;
            } else {
                let (left, _) = measure(&i.path);
                res.freed_bytes += i.bytes.saturating_sub(left);
                res.skipped_items += 1;
            }
        }
    }

    fn recycle_paths(paths: &[&Path]) -> Result<(), i32> {
        use windows::core::PCWSTR;
        use windows::Win32::UI::Shell::{SHFileOperationW, SHFILEOPSTRUCTW};
        const FO_DELETE: u32 = 3;
        const FOF_SILENT: u16 = 0x4;
        const FOF_NOCONFIRMATION: u16 = 0x10;
        const FOF_ALLOWUNDO: u16 = 0x40;
        const FOF_NOCONFIRMMKDIR: u16 = 0x200;
        const FOF_NOERRORUI: u16 = 0x400;
        let mut from: Vec<u16> = Vec::new();
        for p in paths {
            from.extend(p.as_os_str().encode_wide_compat());
            from.push(0);
        }
        from.push(0);
        let mut op = SHFILEOPSTRUCTW {
            wFunc: FO_DELETE,
            pFrom: PCWSTR(from.as_ptr()),
            fFlags: FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI | FOF_NOCONFIRMMKDIR,
            ..Default::default()
        };
        // SAFETY: `from` is a double-null-terminated list that outlives the call.
        let r = unsafe { SHFileOperationW(&mut op) };
        if r == 0 && !op.fAnyOperationsAborted.as_bool() {
            Ok(())
        } else {
            Err(r)
        }
    }

    trait EncodeWide {
        fn encode_wide_compat(&self) -> Vec<u16>;
    }
    impl EncodeWide for std::ffi::OsStr {
        fn encode_wide_compat(&self) -> Vec<u16> {
            use std::os::windows::ffi::OsStrExt;
            self.encode_wide().collect()
        }
    }

    // -----------------------------------------------------------------------------------------
    // Driver store

    fn read_inf(path: &Path) -> Option<String> {
        let b = std::fs::read(path).ok()?;
        if b.len() >= 2 && b[0] == 0xFF && b[1] == 0xFE {
            let w: Vec<u16> = b[2..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            return Some(String::from_utf16_lossy(&w));
        }
        Some(String::from_utf8_lossy(&b).into_owned())
    }

    /// INF names bound to any device, from WMI.
    fn infs_in_use() -> Result<std::collections::HashSet<String>, String> {
        use std::collections::HashMap;
        use wmi::{COMLibrary, Variant, WMIConnection};
        // SAFETY: if COM is already initialised on this thread in another mode, keep using it.
        let com = COMLibrary::new().or_else(|_| Ok::<_, wmi::WMIError>(unsafe { COMLibrary::assume_initialized() })).map_err(|e| e.to_string())?;
        let conn = WMIConnection::new(com).map_err(|e| format!("WMI unavailable: {e}"))?;
        let rows: Vec<HashMap<String, Variant>> =
            conn.raw_query("SELECT InfName FROM Win32_PnPSignedDriver").map_err(|e| format!("Couldn't list drivers: {e}"))?;
        Ok(rows
            .into_iter()
            .filter_map(|r| match r.get("InfName") {
                Some(Variant::String(s)) => Some(s.to_ascii_lowercase()),
                _ => None,
            })
            .collect())
    }

    fn store_folder(inf_path: &Path) -> Option<PathBuf> {
        use windows::core::{HSTRING, PCWSTR};
        use windows::Win32::Devices::DeviceAndDriverInstallation::SetupGetInfDriverStoreLocationW;
        let name = HSTRING::from(inf_path.as_os_str());
        let mut buf = vec![0u16; 1024];
        // SAFETY: name outlives the call; buf is writable.
        unsafe { SetupGetInfDriverStoreLocationW(&name, None, PCWSTR::null(), &mut buf, None) }.ok()?;
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let p = PathBuf::from(String::from_utf16_lossy(&buf[..end]));
        p.parent().map(Path::to_path_buf)
    }

    /// Display driver packages in the driver store.
    pub fn driver_packages() -> Result<Vec<DriverPackage>, String> {
        let windir = env_path("SystemRoot").unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let in_use = infs_in_use()?;
        let mut out = Vec::new();
        let rd = std::fs::read_dir(windir.join("INF")).map_err(|e| format!("Couldn't read the driver list: {e}"))?;
        for e in rd.flatten() {
            let file = e.file_name().to_string_lossy().to_ascii_lowercase();
            if !crate::tweak::valid_oem_inf(&file) {
                continue;
            }
            let Some(text) = read_inf(&e.path()) else { continue };
            if !inf_value(&text, "Class").is_some_and(|c| c.eq_ignore_ascii_case("Display")) {
                continue;
            }
            let provider = inf_value(&text, "Provider").unwrap_or_default();
            if !is_gpu_vendor(&provider) {
                continue;
            }
            let (date, version) = parse_driver_ver(&inf_value(&text, "DriverVer").unwrap_or_default());
            let bytes = store_folder(&e.path()).map(|d| measure(&d).0).unwrap_or(0);
            out.push(DriverPackage {
                in_use: in_use.contains(&file),
                inf: file,
                provider,
                version,
                date,
                bytes,
                kept_for_rollback: false,
            });
        }
        mark_rollback(&mut out);
        out.sort_by(|a, b| b.date.cmp(&a.date));
        Ok(out)
    }

    /// Removes one package from the driver store. Windows refuses if a device uses it; the helper
    /// also re-checks that it's a removable display package first.
    pub fn remove_driver_package(inf: &str) -> Result<u64, String> {
        use windows::core::HSTRING;
        use windows::Win32::Devices::DeviceAndDriverInstallation::SetupUninstallOEMInfW;
        if !crate::tweak::valid_oem_inf(inf) {
            return Err("Not a driver store package name.".into());
        }
        let inf = inf.to_ascii_lowercase();
        let pkgs = driver_packages()?;
        let Some(p) = pkgs.iter().find(|p| p.inf == inf) else {
            return Err(format!("{inf} isn't a graphics driver package."));
        };
        if !removable(p) {
            return Err(format!("{inf} is in use or kept for Roll Back Driver."));
        }
        let name = HSTRING::from(inf.as_str());
        // Flags 0: never force, so Windows refuses a package a device is using.
        // SAFETY: name outlives the call.
        if unsafe { SetupUninstallOEMInfW(&name, 0, None) }.as_bool() {
            Ok(p.bytes)
        } else {
            Err(format!("Windows didn't remove {inf}: {}", std::io::Error::last_os_error()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NV: &str = "; comment\r\n[Version]\r\nSignature = \"$Windows NT$\"\r\nClass = Display\r\nProvider = %NVIDIA%\r\nDriverVer = 09/04/2026,32.0.16.1692\r\n\r\n[Strings]\r\nNVIDIA = \"NVIDIA\"\r\n";

    #[test]
    fn inf_parsing() {
        assert_eq!(inf_value(NV, "class").as_deref(), Some("Display"));
        assert_eq!(inf_value(NV, "Provider").as_deref(), Some("NVIDIA"));
        assert_eq!(parse_driver_ver(&inf_value(NV, "DriverVer").unwrap()), ("2026-09-04".into(), "32.0.16.1692".into()));
    }

    #[test]
    fn rollback_keeps_newest_unused() {
        let p = |inf: &str, date: &str, in_use: bool| DriverPackage {
            inf: inf.into(),
            provider: "NVIDIA".into(),
            date: date.into(),
            in_use,
            ..Default::default()
        };
        let mut v = vec![p("oem1.inf", "2025-01-01", false), p("oem2.inf", "2026-09-04", true), p("oem3.inf", "2026-05-01", false)];
        mark_rollback(&mut v);
        assert!(removable(&v[0]));
        assert!(!removable(&v[1]));
        assert!(v[2].kept_for_rollback && !removable(&v[2]));
    }

    #[test]
    fn only_gpu_vendor_packages() {
        for p in ["NVIDIA", "Advanced Micro Devices, Inc.", "AMD", "ATI Technologies Inc.", "Intel Corporation"] {
            assert!(is_gpu_vendor(p), "{p}");
        }
        for p in ["TeamViewer Germany GmbH", "Parsec Cloud, Inc.", "DisplayLink Corp.", "Microsoft", "AMDisplay Ltd", ""] {
            assert!(!is_gpu_vendor(p), "{p}");
        }
    }

    #[test]
    fn categories_are_sane() {
        for c in ALL_CATEGORIES {
            let i = c.info();
            // Machine-wide folders are never recycled into the user's bin.
            assert!(!(i.needs_admin && i.recyclable), "{c:?}");
        }
        assert!(!CleanCategory::ShaderCache.info().default_selected);
    }
}

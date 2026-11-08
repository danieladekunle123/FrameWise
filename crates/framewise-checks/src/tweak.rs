//! The settings FrameWise can change, as typed values, and the protocol to the elevated helper.
//!
//! Every change is one [`Tweak`] (which setting) set to one [`TweakValue`]. The undo log stores
//! both the old and new value, so Undo writes the old value back through the same code path.
//! There is no "run this command" or "write this registry path": the helper only accepts the
//! machine-wide tweaks listed in [`Tweak::needs_admin`], validated by [`validate_elevated`].

use serde::{Deserialize, Serialize};

use crate::cleanup::CleanCategory;

/// Where a startup entry comes from. Decides which `StartupApproved` key enables or disables it
/// (the same mechanism Task Manager and Settings use, which never deletes the entry).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[serde(rename_all = "snake_case")]
pub enum StartupSource {
    /// `HKCU\...\Run`
    UserRun,
    /// `HKLM\...\Run`
    MachineRun,
    /// `HKLM\...\WOW6432Node\...\Run`
    MachineRun32,
    /// The user's Startup folder.
    UserFolder,
    /// The all-users Startup folder.
    MachineFolder,
}

impl StartupSource {
    pub fn needs_admin(self) -> bool {
        matches!(self, Self::MachineRun | Self::MachineRun32 | Self::MachineFolder)
    }

    /// Sub-key of `...\Explorer\StartupApproved` that records the enabled state.
    pub fn approved_subkey(self) -> &'static str {
        match self {
            Self::UserRun | Self::MachineRun => "Run",
            Self::MachineRun32 => "Run32",
            Self::UserFolder | Self::MachineFolder => "StartupFolder",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::UserRun => "Current user (registry)",
            Self::MachineRun => "All users (registry)",
            Self::MachineRun32 => "All users, 32-bit (registry)",
            Self::UserFolder => "Current user (Startup folder)",
            Self::MachineFolder => "All users (Startup folder)",
        }
    }
}

/// One setting FrameWise can change.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Tweak {
    /// Refresh rate of one display at the resolution it had when the change was made.
    /// Value: `Dword(hz)`.
    DisplayRefresh { device: String, monitor: String, width: u32, height: u32 },
    /// Active power plan. Value: `Text(guid)`.
    PowerScheme,
    /// Windows 11 power mode (the overlay on top of the plan). Value: `Text(guid)`; the all-zero
    /// GUID is "Balanced".
    PowerMode,
    /// `HKCU\Software\Microsoft\GameBar\AutoGameModeEnabled`. Value: `Absent | Dword(0|1)`.
    GameMode,
    /// `HKCU\Software\Microsoft\DirectX\UserGpuPreferences\DirectXUserGlobalSettings`.
    /// Value: `Absent | Text`.
    WindowedOptimizations,
    /// `HKLM\SYSTEM\CurrentControlSet\Control\GraphicsDrivers\HwSchMode`.
    /// Value: `Absent | Dword(1|2)`. Machine-wide.
    Hags,
    /// `HKLM\...\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity\Enabled`.
    /// Value: `Absent | Dword(0|1)`. Machine-wide.
    MemoryIntegrity,
    /// The `StartupApproved` value for one entry. `name` is the registry value name, or the file
    /// name in a Startup folder. Value: `Absent | Bytes(12)`.
    StartupEntry { source: StartupSource, name: String },
}

impl Tweak {
    /// Machine-wide settings, which only the elevated helper may write.
    pub fn needs_admin(&self) -> bool {
        match self {
            Tweak::Hags | Tweak::MemoryIntegrity => true,
            Tweak::StartupEntry { source, .. } => source.needs_admin(),
            _ => false,
        }
    }

    pub fn needs_restart(&self) -> bool {
        matches!(self, Tweak::Hags | Tweak::MemoryIntegrity)
    }

    /// Short name for the History screen.
    pub fn title(&self) -> String {
        match self {
            Tweak::DisplayRefresh { monitor, device, .. } => {
                let who = if monitor.is_empty() { device.trim_start_matches(r"\\.\") } else { monitor };
                format!("Refresh rate · {who}")
            }
            Tweak::PowerScheme => "Power plan".into(),
            Tweak::PowerMode => "Power mode".into(),
            Tweak::GameMode => "Game Mode".into(),
            Tweak::WindowedOptimizations => "Optimisations for windowed games".into(),
            Tweak::Hags => "Hardware-accelerated GPU scheduling".into(),
            Tweak::MemoryIntegrity => "Memory Integrity".into(),
            Tweak::StartupEntry { name, .. } => format!("Startup app · {}", startup_display_name(name)),
        }
    }

    /// Plain-language reading of a value of this setting.
    pub fn describe(&self, v: &TweakValue) -> String {
        use TweakValue::*;
        match (self, v) {
            (Tweak::DisplayRefresh { .. }, Dword(hz)) => format!("{hz} Hz"),
            (Tweak::PowerScheme, Text(g)) => scheme_name(g).into(),
            (Tweak::PowerMode, Text(g)) => power_mode_name(g).into(),
            (Tweak::PowerMode, Absent) => "Balanced".into(),
            (Tweak::GameMode, Dword(0)) => "Off".into(),
            (Tweak::GameMode, Dword(_)) => "On".into(),
            (Tweak::GameMode, Absent) => "On (Windows default)".into(),
            (Tweak::WindowedOptimizations, Text(s)) => match dx_get(Some(s), "SwapEffectUpgradeEnable") {
                Some("1") => "On".into(),
                Some("0") => "Off".into(),
                _ => "Windows default".into(),
            },
            (Tweak::WindowedOptimizations, Absent) => "Windows default".into(),
            (Tweak::Hags, Dword(2)) => "On".into(),
            (Tweak::Hags, Dword(_)) => "Off".into(),
            (Tweak::Hags, Absent) => "Not set".into(),
            (Tweak::MemoryIntegrity, Dword(1)) => "On".into(),
            (Tweak::MemoryIntegrity, Dword(_)) => "Off".into(),
            (Tweak::MemoryIntegrity, Absent) => "Not configured".into(),
            (Tweak::StartupEntry { .. }, Bytes(b)) if startup_bytes_enabled(b) => "Enabled".into(),
            (Tweak::StartupEntry { .. }, Bytes(_)) => "Disabled".into(),
            (Tweak::StartupEntry { .. }, Absent) => "Enabled".into(),
            (_, v) => format!("{v:?}"),
        }
    }
}

/// A setting's value. `Absent` means the registry value doesn't exist (Windows uses its default),
/// so Undo can delete a value FrameWise created rather than leaving a different one behind.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum TweakValue {
    Absent,
    Dword(u32),
    Text(String),
    Bytes(Vec<u8>),
}

// ---------------------------------------------------------------------------------------------
// Value helpers (pure, unit-tested)

pub const SCHEME_BALANCED: &str = "381b4222-f694-41f0-9685-ff5bb260df2e";
pub const SCHEME_HIGH: &str = "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c";
pub const SCHEME_SAVER: &str = "a1841308-3541-4fab-bc81-f71556f20b4a";
pub const SCHEME_ULTIMATE: &str = "e9a42b02-d5df-448d-aa00-03f14749eb61";
/// A power plan GUID in `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` form (custom plans have their own).
pub fn is_known_scheme_or_guid(g: &str) -> bool {
    let g = g.trim();
    g.len() == 36
        && g.char_indices().all(|(i, c)| if matches!(i, 8 | 13 | 18 | 23) { c == '-' } else { c.is_ascii_hexdigit() })
}

pub const MODE_BEST_EFFICIENCY: &str = "961cc777-2547-4f9d-8174-7d86181b8a7a";
pub const MODE_BALANCED: &str = "00000000-0000-0000-0000-000000000000";
pub const MODE_BEST_PERFORMANCE: &str = "ded574b5-45a0-4f42-8737-46345c09c238";

pub fn scheme_name(guid: &str) -> &'static str {
    match guid.to_ascii_lowercase().as_str() {
        SCHEME_BALANCED => "Balanced",
        SCHEME_HIGH => "High performance",
        SCHEME_SAVER => "Power saver",
        SCHEME_ULTIMATE => "Ultimate performance",
        _ => "Custom plan",
    }
}

pub fn power_mode_name(guid: &str) -> &'static str {
    match guid.to_ascii_lowercase().as_str() {
        MODE_BEST_EFFICIENCY => "Best power efficiency",
        MODE_BEST_PERFORMANCE => "Best performance",
        "" | MODE_BALANCED => "Balanced",
        _ => "Custom mode",
    }
}

/// Reads one `Key=Value` from a `DirectXUserGlobalSettings` string such as
/// `"VRROptimizeEnable=0;SwapEffectUpgradeEnable=1;"`.
pub fn dx_get<'a>(setting: Option<&'a str>, key: &str) -> Option<&'a str> {
    setting?
        .split(';')
        .filter_map(|kv| kv.split_once('='))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case(key))
        .map(|(_, v)| v.trim())
}

/// Sets one key in a `DirectXUserGlobalSettings` string, keeping every other key as it was.
pub fn dx_set(setting: Option<&str>, key: &str, value: &str) -> String {
    let mut out = String::new();
    let mut found = false;
    for kv in setting.unwrap_or("").split(';').filter(|kv| !kv.trim().is_empty()) {
        match kv.split_once('=') {
            Some((k, _)) if k.trim().eq_ignore_ascii_case(key) => {
                found = true;
                out.push_str(&format!("{key}={value};"));
            }
            _ => {
                out.push_str(kv);
                out.push(';');
            }
        }
    }
    if !found {
        out.push_str(&format!("{key}={value};"));
    }
    out
}

/// `StartupApproved` values are 12 bytes: a state byte (even = enabled, odd = disabled), three
/// zero bytes, then the FILETIME when it was disabled.
pub fn startup_bytes_enabled(b: &[u8]) -> bool {
    b.first().map(|x| x & 1 == 0).unwrap_or(true)
}

pub fn startup_disabled_bytes(filetime: u64) -> Vec<u8> {
    let mut v = vec![3u8, 0, 0, 0];
    v.extend_from_slice(&filetime.to_le_bytes());
    v
}

/// Strips a Startup folder shortcut's extension for display ("Discord.lnk" → "Discord").
pub fn startup_display_name(name: &str) -> &str {
    for ext in [".lnk", ".url", ".exe", ".bat", ".cmd"] {
        if name.len() > ext.len() && name[name.len() - ext.len()..].eq_ignore_ascii_case(ext) {
            return &name[..name.len() - ext.len()];
        }
    }
    name
}

/// A startup command for display: `"C:\x\app.exe" -silent` becomes `C:\x\app.exe -silent`. Only
/// the quotes around the program path are dropped; quotes inside the arguments stay.
pub fn startup_display_command(raw: &str) -> String {
    let raw = raw.trim();
    if let Some(rest) = raw.strip_prefix('"') {
        if let Some(end) = rest.find('"') {
            let args = rest[end + 1..].trim_start();
            return if args.is_empty() { rest[..end].to_string() } else { format!("{} {args}", &rest[..end]) };
        }
        return rest.to_string();
    }
    raw.to_string()
}

/// Checks a request to the elevated helper before anything is written. The helper runs this on
/// every request; the app core runs it too so a bad request never reaches a UAC prompt.
pub fn validate_elevated(tweak: &Tweak, value: &TweakValue) -> Result<(), String> {
    use TweakValue::*;
    let ok = match tweak {
        Tweak::Hags => matches!(value, Absent | Dword(1) | Dword(2)),
        Tweak::MemoryIntegrity => matches!(value, Absent | Dword(0) | Dword(1)),
        Tweak::StartupEntry { source, name } if source.needs_admin() => {
            let name_ok = !name.trim().is_empty()
                && name.len() <= 260
                && !name.chars().any(|c| c.is_control() || matches!(c, '\\' | '/'));
            let value_ok = match value {
                Absent => true,
                Bytes(b) => b.len() == 12 && b[1..4] == [0, 0, 0],
                _ => false,
            };
            name_ok && value_ok
        }
        _ => return Err(format!("{} isn't a setting the helper changes.", tweak.title())),
    };
    if ok {
        Ok(())
    } else {
        Err(format!("Refused an invalid value for {}.", tweak.title()))
    }
}

// ---------------------------------------------------------------------------------------------
// Helper protocol: one JSON object per line over the named pipe.

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum HelperRequest {
    /// Create a Windows System Restore point (or report the one Windows made in the last 24 hours).
    CreateRestorePoint { description: String },
    /// Write one machine-wide setting; the reply carries the value read back.
    Write { tweak: Tweak, value: TweakValue },
    /// Measure machine-wide Cleanup categories (their folders aren't readable without admin).
    ScanCleanup { categories: Vec<CleanCategory> },
    /// Clean one machine-wide Cleanup category. Paths come from the category, never the request.
    Clean { category: CleanCategory },
    /// Remove one unused display driver package from the driver store.
    RemoveDriverPackage { inf: String },
    /// Start frame-time capture: the helper runs the bundled PresentMon (checked against its
    /// pinned hash) with fixed arguments. Nothing about the command line comes from the request.
    StartCapture,
    /// Return up to `max_lines` lines of capture output gathered since the last read.
    ReadCapture { max_lines: u32 },
    /// Stop the capture (PresentMon is also killed if the helper exits).
    StopCapture,
    /// Add the user who started the helper to Windows' "Performance Log Users" group, so monitoring
    /// works without a prompt after they next sign in. The user comes from the parent process's
    /// token, never from the request.
    AllowMonitoringWithoutPrompt,
    /// End of task; the helper exits.
    Done,
}

/// The PresentMon build FrameWise bundles, pinned by SHA-256. Anything else is refused, so the
/// elevated helper can never be made to run a swapped executable.
pub const PRESENTMON_VERSION: &str = "2.6.0";
pub const PRESENTMON_SHA256: &str = "b2a706bc6ad475749e3b7e3409263aa1e6906d45bdcf993f6dbc0f660188f1af";

/// PresentMon's arguments, the same for direct and helper capture: CSV on stdout for every app
/// that presents frames, except the desktop compositor and FrameWise's own interface.
pub const PRESENTMON_ARGS: &[&str] = &[
    "--output_stdout",
    "--no_console_stats",
    "--no_track_input",
    "--session_name",
    "FrameWise",
    "--stop_existing_session",
    "--exclude",
    "dwm.exe",
    "--exclude",
    "msedgewebview2.exe",
];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct HelperReply {
    pub ok: bool,
    #[serde(default)]
    pub data: serde_json::Value,
    #[serde(default)]
    pub error: Option<String>,
    /// Machine-readable reason, for example `restore_point_off`.
    #[serde(default)]
    pub code: Option<String>,
}

impl HelperReply {
    pub fn ok(data: serde_json::Value) -> Self {
        HelperReply { ok: true, data, error: None, code: None }
    }
    pub fn err(code: &str, msg: impl Into<String>) -> Self {
        HelperReply { ok: false, data: serde_json::Value::Null, error: Some(msg.into()), code: Some(code.into()) }
    }
}

/// Driver store package names look like `oem12.inf`.
pub fn valid_oem_inf(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l.len() <= 16
        && l.starts_with("oem")
        && l.ends_with(".inf")
        && l.len() > 7
        && l[3..l.len() - 4].chars().all(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_commands_display_without_path_quotes() {
        assert_eq!(startup_display_command(r#""C:\Epic\Launcher.exe" -silent -x=boot"#), r"C:\Epic\Launcher.exe -silent -x=boot");
        assert_eq!(startup_display_command(r#""C:\GG\gg.exe" -dataPath="C:\Data" -auto=true"#), r#"C:\GG\gg.exe -dataPath="C:\Data" -auto=true"#);
        assert_eq!(startup_display_command(r#""C:\a b\app.exe""#), r"C:\a b\app.exe");
        assert_eq!(startup_display_command(r"C:\app.exe --hidden"), r"C:\app.exe --hidden");
        assert_eq!(startup_display_command(r#""C:\broken.exe"#), r"C:\broken.exe");
    }

    #[test]
    fn dx_set_keeps_other_keys() {
        assert_eq!(dx_set(None, "SwapEffectUpgradeEnable", "1"), "SwapEffectUpgradeEnable=1;");
        assert_eq!(
            dx_set(Some("VRROptimizeEnable=0;SwapEffectUpgradeEnable=0;"), "SwapEffectUpgradeEnable", "1"),
            "VRROptimizeEnable=0;SwapEffectUpgradeEnable=1;"
        );
        assert_eq!(dx_set(Some("AutoHDREnable=1"), "SwapEffectUpgradeEnable", "1"), "AutoHDREnable=1;SwapEffectUpgradeEnable=1;");
        assert_eq!(dx_get(Some("A=1;SwapEffectUpgradeEnable=0;"), "swapeffectupgradeenable"), Some("0"));
        assert_eq!(dx_get(Some(""), "SwapEffectUpgradeEnable"), None);
    }

    #[test]
    fn startup_bytes() {
        let d = startup_disabled_bytes(0x01D9_0000_0000_0000);
        assert_eq!(d.len(), 12);
        assert!(!startup_bytes_enabled(&d));
        assert!(startup_bytes_enabled(&[2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        assert!(startup_bytes_enabled(&[6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        assert_eq!(startup_display_name("Discord.lnk"), "Discord");
        assert_eq!(startup_display_name("Steam"), "Steam");
    }

    #[test]
    fn helper_allowlist() {
        use TweakValue::*;
        assert!(validate_elevated(&Tweak::Hags, &Dword(2)).is_ok());
        assert!(validate_elevated(&Tweak::Hags, &Dword(7)).is_err());
        assert!(validate_elevated(&Tweak::MemoryIntegrity, &Text("0".into())).is_err());
        // Per-user settings never go through the helper.
        assert!(validate_elevated(&Tweak::GameMode, &Dword(1)).is_err());
        let user = Tweak::StartupEntry { source: StartupSource::UserRun, name: "X".into() };
        assert!(validate_elevated(&user, &Absent).is_err());
        let machine = Tweak::StartupEntry { source: StartupSource::MachineRun, name: "SteelSeriesGG".into() };
        assert!(validate_elevated(&machine, &Bytes(startup_disabled_bytes(1))).is_ok());
        assert!(validate_elevated(&machine, &Bytes(vec![3; 40])).is_err());
        let sneaky = Tweak::StartupEntry { source: StartupSource::MachineFolder, name: r"..\..\x.lnk".into() };
        assert!(validate_elevated(&sneaky, &Absent).is_err());
        assert!(valid_oem_inf("oem12.inf") && valid_oem_inf("OEM3.INF"));
        assert!(!valid_oem_inf("nvlddmkm.inf") && !valid_oem_inf("oem.inf") && !valid_oem_inf(r"oem1.inf\..\x"));
    }

    #[test]
    fn protocol_shape() {
        let r = HelperRequest::Write { tweak: Tweak::Hags, value: TweakValue::Dword(2) };
        let j = serde_json::to_string(&r).unwrap();
        assert_eq!(j, r#"{"op":"write","tweak":{"kind":"hags"},"value":{"type":"dword","value":2}}"#);
        assert_eq!(serde_json::from_str::<HelperRequest>(&j).unwrap(), r);
        assert!(serde_json::from_str::<HelperRequest>(r#"{"op":"run","cmd":"calc"}"#).is_err());
    }

    #[test]
    fn describe_values() {
        assert_eq!(Tweak::PowerScheme.describe(&TweakValue::Text(SCHEME_SAVER.into())), "Power saver");
        assert_eq!(Tweak::GameMode.describe(&TweakValue::Absent), "On (Windows default)");
        let d = Tweak::DisplayRefresh { device: r"\\.\DISPLAY1".into(), monitor: String::new(), width: 1, height: 1 };
        assert_eq!(d.title(), "Refresh rate · DISPLAY1");
        assert_eq!(d.describe(&TweakValue::Dword(165)), "165 Hz");
    }
}

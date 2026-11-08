//! A read-only picture of the system, gathered once per scan.
//!
//! Every field that comes from a source which can fail is an `Option`: `None` means "couldn't be
//! read", which evaluates to [`crate::Status::Unknown`] rather than a guess.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Snapshot {
    /// Unix seconds when collected. Used for age checks such as driver dates.
    pub collected_at: u64,
    pub os: OsInfo,
    /// `None` when the battery state couldn't be read.
    pub is_laptop: Option<bool>,
    pub cpu_name: Option<String>,
    pub memory_modules: Option<Vec<MemoryModule>>,
    pub displays: Option<Vec<DisplayInfo>>,
    pub gpus: Option<Vec<GpuInfo>>,
    pub drives: Option<Vec<DriveInfo>>,
    pub system_drive: Option<String>,
    pub game_installs: Option<Vec<GameInstall>>,
    /// Lower-case executable names of running processes.
    pub processes: Option<Vec<String>>,
    pub startup_entries: Option<Vec<StartupEntry>>,
    pub power: PowerInfo,
    /// `HKCU\Software\Microsoft\GameBar\AutoGameModeEnabled`; `None` = not set (Windows default: on).
    pub game_mode_auto: Option<u32>,
    /// `HKLM\SYSTEM\CurrentControlSet\Control\GraphicsDrivers\HwSchMode`; 2 = on, 1 = off.
    pub hags_mode: Option<u32>,
    /// Hardware scheduling caps reported by the graphics kernel (more reliable than the registry).
    pub hags_caps: Option<HagsCaps>,
    /// `HKCU\Software\Microsoft\DirectX\UserGpuPreferences\DirectXUserGlobalSettings`.
    pub dx_user_global_settings: Option<String>,
    /// `...\DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity\Enabled`.
    pub hvci_enabled: Option<u32>,
    /// Whether Memory Integrity is actually running now (`Win32_DeviceGuard.SecurityServicesRunning`
    /// contains 2). Differs from `hvci_enabled` until a restart, or if the PC can't run it.
    #[serde(default)]
    pub hvci_running: Option<bool>,
    pub network: Option<NetworkProbe>,
    pub collection_errors: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct OsInfo {
    pub build: u32,
    pub display_version: Option<String>,
}

impl OsInfo {
    pub fn is_windows_11(&self) -> bool {
        self.build >= 22000
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct MemoryModule {
    pub capacity_bytes: u64,
    /// `Win32_PhysicalMemory.Speed`: the module's reported speed (often its JEDEC base, not XMP).
    pub speed: u32,
    /// `Win32_PhysicalMemory.ConfiguredClockSpeed`: the speed it actually runs at.
    pub configured_speed: u32,
    pub manufacturer: String,
    pub part_number: String,
    pub bank_label: String,
    pub device_locator: String,
    /// SMBIOS memory type: 26 = DDR4, 34 = DDR5.
    pub smbios_memory_type: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct DisplayInfo {
    pub device_name: String,
    pub adapter: String,
    pub monitor: String,
    pub primary: bool,
    pub width: u32,
    pub height: u32,
    pub current_hz: u32,
    /// Highest refresh rate available at the current resolution.
    pub max_hz_at_resolution: u32,
    /// Highest refresh rate at any resolution.
    pub max_hz_any: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct GpuInfo {
    pub name: String,
    pub vendor: String,
    pub driver_version: String,
    /// `yyyymmdd`, if known.
    pub driver_date: Option<String>,
    pub pnp_device_id: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MediaType {
    Ssd,
    Hdd,
    Unknown,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DriveInfo {
    /// Mount point, for example `C:\`.
    pub mount: String,
    pub media: MediaType,
    pub total_bytes: u64,
    pub free_bytes: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct GameInstall {
    pub name: String,
    pub launcher: String,
    pub path: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StartupEntry {
    pub name: String,
    pub location: String,
    pub enabled: bool,
    /// Where it's registered; `None` in snapshots recorded before phase 2.
    #[serde(default)]
    pub source: Option<crate::tweak::StartupSource>,
    /// Registry value name, or the file name in a Startup folder (the `StartupApproved` key).
    #[serde(default)]
    pub value_name: String,
    /// The command or shortcut target, for display.
    #[serde(default)]
    pub command: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct PowerInfo {
    /// Active power scheme GUID, lower case.
    pub active_scheme: Option<String>,
    /// Windows 11 power mode overlay on AC power, lower case GUID.
    pub ac_overlay: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HagsCaps {
    /// At least one GPU and its driver support hardware-accelerated GPU scheduling.
    pub supported: bool,
    /// It's switched on for a supporting GPU.
    pub enabled: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct NetworkProbe {
    pub target: String,
    /// Successful round-trip samples in milliseconds.
    pub samples_ms: Vec<f64>,
    pub failures: u32,
}

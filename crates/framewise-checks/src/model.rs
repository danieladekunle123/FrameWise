use serde::{Deserialize, Serialize};

/// Typical impact of an issue on FPS, stutter or latency. A starting estimate; benchmark mode
/// measures the real effect on each PC.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Impact {
    High,
    Medium,
    LowMedium,
    Low,
}

/// Result of one check.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Something costs performance and can be improved.
    Issue,
    /// Checked and fine.
    Ok,
    /// FrameWise couldn't determine this automatically; the guide says how to check by hand.
    Unknown,
    /// Doesn't apply to this PC (for example a desktop-only check on a laptop).
    NotApplicable,
}

/// What the user can do about an issue.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// FrameWise can change it (one-click from phase 2; manual steps until then).
    Fix,
    /// FrameWise can't change it (BIOS, hardware); step-by-step instructions.
    Guide,
    /// A trade-off (for example security vs FPS); both options explained, never applied automatically.
    Choose,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Link {
    pub label: String,
    pub url: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Choice {
    pub label: String,
    pub gains: String,
    pub costs: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Guide {
    pub steps: Vec<String>,
    /// A Windows settings page that can be opened directly (`ms-settings:` or `windowsdefender:`).
    pub settings_uri: Option<String>,
    pub links: Vec<Link>,
    /// Only for [`Action::Choose`].
    pub choices: Vec<Choice>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Finding {
    pub check_id: String,
    pub title: String,
    pub status: Status,
    pub impact: Impact,
    pub action: Action,
    /// One-line result for this PC, for example "Running at 60 Hz; supports 165 Hz".
    pub summary: String,
    /// What it is and why it matters, in plain language.
    pub explanation: String,
    /// What the fix changes.
    pub fix_changes: String,
    pub detected: Option<String>,
    pub expected: Option<String>,
    /// Supporting detail lines (module list, game list, ...).
    pub details: Vec<String>,
    pub guide: Guide,
    /// For `Fix` actions: the roadmap phase in which the one-click fix ships.
    pub one_click_phase: Option<u8>,
    /// What FrameWise can do about it right now. `None` when there's nothing to apply (it's fine,
    /// or only a guide helps). Recomputed from the check result, see [`crate::fixes::fix_info`].
    #[serde(default)]
    pub fix: Option<FixInfo>,
}

/// How a one-click fix is offered in the interface.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum FixKind {
    /// Apply directly.
    Apply,
    /// The user picks which startup entries to disable first.
    PickStartup,
    /// A trade-off the user chooses explicitly, after a confirmation.
    Choose,
    /// Opens the Cleanup screen.
    Cleanup,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FixInfo {
    pub kind: FixKind,
    /// Button label, for example "Turn on".
    pub label: String,
    /// Included in "Fix all safe": low risk, no restart, no choice to make.
    pub safe: bool,
    /// Windows asks for permission (UAC) because the setting is machine-wide.
    pub needs_admin: bool,
    /// Takes effect after a restart.
    pub needs_restart: bool,
    /// Shown in a confirmation dialog before applying, when the change deserves a second look.
    pub confirm: Option<String>,
}

/// A completed scan.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ScanResult {
    pub id: i64,
    pub started_at: u64,
    pub duration_ms: u64,
    pub score: u8,
    pub app_version: String,
    pub findings: Vec<Finding>,
    pub collection_errors: Vec<String>,
}

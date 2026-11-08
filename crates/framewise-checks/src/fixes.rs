//! Which checks FrameWise can fix, and the exact changes each fix makes.
//!
//! [`fix_info`] is pure: it decides from a finding what the interface offers. [`plan`] (Windows)
//! reads the live system and returns the list of [`Planned`] changes; the app core then runs each
//! through the change pipeline.

use serde::{Deserialize, Serialize};

use crate::model::{Finding, FixInfo, FixKind, Status};
use crate::tweak::{StartupSource, Tweak, TweakValue};

/// Fixes "Fix all safe" applies: per-user, instant, no restart, no choice involved.
pub const SAFE_FIXES: &[&str] = &["power_plan", "game_mode", "windowed_optimizations"];

pub fn fix_info(f: &Finding) -> Option<FixInfo> {
    if f.status != Status::Issue {
        return None;
    }
    let info = |kind, label: &str, safe, needs_admin, needs_restart, confirm: Option<&str>| FixInfo {
        kind,
        label: label.to_string(),
        safe,
        needs_admin,
        needs_restart,
        confirm: confirm.map(String::from),
    };
    Some(match f.check_id.as_str() {
        "refresh_rate" => info(FixKind::Apply, "Set highest refresh rate", false, false, false, None),
        "startup_bloat" => info(FixKind::PickStartup, "Choose apps to disable", false, false, false, None),
        "power_plan" => info(FixKind::Apply, "Switch to Balanced", true, false, false, None),
        "game_mode" => info(FixKind::Apply, "Turn on", true, false, false, None),
        "hags" => info(FixKind::Apply, "Turn on", false, true, true, None),
        "windowed_optimizations" => info(FixKind::Apply, "Turn on", true, false, false, None),
        "memory_integrity" => info(
            FixKind::Choose,
            "Turn it off",
            false,
            true,
            true,
            Some("Memory Integrity protects Windows from malicious and vulnerable drivers. Turning it off can recover a few percent of FPS, mostly on older CPUs, but removes that protection. Some anti-cheat systems and work or school PCs require it. It takes effect after a restart, and you can turn it back on from History or Windows Security."),
        ),
        "system_drive_space" => info(FixKind::Cleanup, "Open Cleanup", false, false, false, None),
        _ => return None,
    })
}

/// One change a fix will make.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Planned {
    pub check_id: String,
    pub tweak: Tweak,
    pub value: TweakValue,
}

/// A startup entry the user picked to disable.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct StartupPick {
    pub source: StartupSource,
    pub value_name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct PlanInput {
    /// For `startup_bloat`.
    #[serde(default)]
    pub startup: Vec<StartupPick>,
    /// For `memory_integrity`: true to turn it on, false (default) to turn it off.
    #[serde(default)]
    pub enable: bool,
}

#[cfg(windows)]
pub fn plan(check_id: &str, input: &PlanInput) -> Result<Vec<Planned>, String> {
    use crate::system;
    use crate::tweak::*;
    let one = |tweak: Tweak, value: TweakValue| Ok(vec![Planned { check_id: check_id.into(), tweak, value }]);
    match check_id {
        "refresh_rate" => {
            let displays = crate::collect::collect_displays()?;
            Ok(displays
                .into_iter()
                .filter(|d| d.max_hz_at_resolution > d.current_hz)
                .map(|d| Planned {
                    check_id: check_id.into(),
                    tweak: Tweak::DisplayRefresh { device: d.device_name, monitor: d.monitor, width: d.width, height: d.height },
                    value: TweakValue::Dword(d.max_hz_at_resolution),
                })
                .collect())
        }
        "power_plan" => {
            let mut v = Vec::new();
            if system::active_scheme()?.eq_ignore_ascii_case(SCHEME_SAVER) {
                v.push(Planned { check_id: check_id.into(), tweak: Tweak::PowerScheme, value: TweakValue::Text(SCHEME_BALANCED.into()) });
            }
            if let Ok(TweakValue::Text(m)) = system::read(&Tweak::PowerMode) {
                if m.eq_ignore_ascii_case(MODE_BEST_EFFICIENCY) {
                    v.push(Planned { check_id: check_id.into(), tweak: Tweak::PowerMode, value: TweakValue::Text(MODE_BALANCED.into()) });
                }
            }
            Ok(v)
        }
        "game_mode" => one(Tweak::GameMode, TweakValue::Dword(1)),
        "hags" => one(Tweak::Hags, TweakValue::Dword(2)),
        "windowed_optimizations" => {
            let cur = match system::read(&Tweak::WindowedOptimizations)? {
                TweakValue::Text(s) => Some(s),
                _ => None,
            };
            one(Tweak::WindowedOptimizations, TweakValue::Text(dx_set(cur.as_deref(), "SwapEffectUpgradeEnable", "1")))
        }
        "memory_integrity" => {
            if system::memory_integrity_locked() {
                return Err("Memory Integrity is locked on by this PC's firmware (UEFI lock), so only the BIOS can turn it off.".into());
            }
            one(Tweak::MemoryIntegrity, TweakValue::Dword(input.enable as u32))
        }
        "startup_bloat" => {
            if input.startup.is_empty() {
                return Err("Pick at least one app to disable.".into());
            }
            let now = system::filetime_now();
            Ok(input
                .startup
                .iter()
                .map(|p| Planned {
                    check_id: check_id.into(),
                    tweak: Tweak::StartupEntry { source: p.source, name: p.value_name.clone() },
                    value: TweakValue::Bytes(startup_disabled_bytes(now)),
                })
                .collect())
        }
        _ => Err(format!("There's no one-click fix for {check_id}.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{evaluate_one, Snapshot};

    #[test]
    fn offers_fixes_only_for_issues() {
        let mut snap = Snapshot::default();
        snap.game_mode_auto = Some(0);
        let f = evaluate_one(&snap, "game_mode").unwrap();
        let fix = f.fix.expect("game mode off is fixable");
        assert!(fix.safe && !fix.needs_admin);
        snap.game_mode_auto = Some(1);
        assert!(evaluate_one(&snap, "game_mode").unwrap().fix.is_none());
        // Guides never get a fix.
        assert!(evaluate_one(&snap, "ram_speed").unwrap().fix.is_none());
    }

    #[test]
    fn security_trade_offs_are_never_safe() {
        let mut snap = Snapshot::default();
        snap.hvci_enabled = Some(1);
        let fix = evaluate_one(&snap, "memory_integrity").unwrap().fix.unwrap();
        assert_eq!(fix.kind, FixKind::Choose);
        assert!(!fix.safe && fix.needs_admin && fix.confirm.is_some());
        assert!(!SAFE_FIXES.contains(&"memory_integrity") && !SAFE_FIXES.contains(&"hags"));
    }
}

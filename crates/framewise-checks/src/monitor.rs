//! Live monitor data: one sensor sample per second, and the rules that turn sensor readings into
//! throttling warnings. Pure code; the Windows readers live in `sensors.rs`.

use serde::{Deserialize, Serialize};

/// One second of sensor readings. Every field is optional: what's available depends on the GPU
/// vendor and the PC (CPU temperature, for example, needs a kernel driver FrameWise doesn't ship).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SensorSample {
    /// CPU busy, all cores, 0–100 (Task Manager's "Utility").
    pub cpu_pct: Option<f32>,
    /// CPU speed relative to its base clock, 0–100+ (over 100 when boosting).
    pub cpu_perf_pct: Option<f32>,
    /// The most performance the firmware currently allows, as % of base clock (Windows' `% Performance
    /// Limit`): 100 means no limit; lower means a power or temperature limit is holding the CPU back.
    pub cpu_cap_pct: Option<f32>,
    /// Busiest GPU engine (usually 3D), 0–100.
    pub gpu_pct: Option<f32>,
    pub gpu_temp_c: Option<f32>,
    pub gpu_clock_mhz: Option<u32>,
    pub gpu_power_w: Option<f32>,
    /// The GPU is slowing itself down because it's too hot (NVIDIA only).
    pub gpu_thermal_throttle: Option<bool>,
    /// The GPU is at its power limit. Normal under heavy load; shown, never warned about.
    pub gpu_power_limited: Option<bool>,
    pub ram_used_bytes: Option<u64>,
    pub ram_total_bytes: Option<u64>,
    pub vram_used_bytes: Option<u64>,
    pub vram_total_bytes: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThrottleKind {
    /// GPU reports thermal slowdown.
    GpuThermal,
    /// CPU busy but running well below its base clock, or held back by the firmware.
    Cpu,
}

/// Seconds a condition must hold before it's reported, so a one-second blip doesn't raise a warning.
pub const THROTTLE_SECONDS: u32 = 5;

/// Watches consecutive samples for sustained throttling.
#[derive(Default, Debug, Clone)]
pub struct ThrottleDetector {
    gpu: u32,
    cpu: u32,
}

impl ThrottleDetector {
    /// Feeds one sample (one second) and returns what's currently throttling.
    pub fn feed(&mut self, s: &SensorSample) -> Vec<ThrottleKind> {
        self.gpu = if s.gpu_thermal_throttle == Some(true) { self.gpu + 1 } else { 0 };
        // Only meaningful under load: an idle CPU runs slow on purpose.
        let busy = s.cpu_pct.is_some_and(|p| p >= 50.0);
        let slow = s.cpu_perf_pct.is_some_and(|p| p < 80.0) || s.cpu_cap_pct.is_some_and(|p| p < 85.0);
        self.cpu = if busy && slow { self.cpu + 1 } else { 0 };
        let mut out = Vec::new();
        if self.gpu >= THROTTLE_SECONDS {
            out.push(ThrottleKind::GpuThermal);
        }
        if self.cpu >= THROTTLE_SECONDS {
            out.push(ThrottleKind::Cpu);
        }
        out
    }
}

/// What the live monitor sends to the interface (and the overlay) about once a second.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MonitorTick {
    /// The app being measured (exe name) and its process ID, if any app is presenting frames.
    pub app: Option<String>,
    pub pid: Option<u32>,
    /// Stats over the last few seconds of frames.
    pub fps: Option<f64>,
    pub low_1: Option<f64>,
    pub frame_ms: Option<f64>,
    /// Frame times since the previous tick, for the graph (capped).
    pub frames: Vec<f32>,
    pub sensors: SensorSample,
    pub throttling: Vec<ThrottleKind>,
    /// Other apps presenting frames right now (exe, pid, frames in the last second), busiest first.
    pub presenting: Vec<(String, u32, u32)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(cpu: f32, perf: f32, gpu_hot: bool) -> SensorSample {
        SensorSample { cpu_pct: Some(cpu), cpu_perf_pct: Some(perf), cpu_cap_pct: Some(100.0), gpu_thermal_throttle: Some(gpu_hot), ..Default::default() }
    }

    #[test]
    fn throttling_needs_to_be_sustained() {
        let mut d = ThrottleDetector::default();
        for _ in 0..THROTTLE_SECONDS - 1 {
            assert!(d.feed(&s(90.0, 60.0, true)).is_empty());
        }
        assert_eq!(d.feed(&s(90.0, 60.0, true)), vec![ThrottleKind::GpuThermal, ThrottleKind::Cpu]);
        // One good second resets it.
        assert!(d.feed(&s(90.0, 105.0, false)).is_empty());
    }

    #[test]
    fn idle_cpu_running_slow_is_not_throttling() {
        let mut d = ThrottleDetector::default();
        for _ in 0..20 {
            assert!(d.feed(&s(5.0, 40.0, false)).is_empty());
        }
        // Firmware limit under load counts.
        let mut d = ThrottleDetector::default();
        let limited = SensorSample { cpu_pct: Some(80.0), cpu_perf_pct: Some(95.0), cpu_cap_pct: Some(70.0), ..Default::default() };
        let mut last = vec![];
        for _ in 0..THROTTLE_SECONDS {
            last = d.feed(&limited);
        }
        assert_eq!(last, vec![ThrottleKind::Cpu]);
    }
}

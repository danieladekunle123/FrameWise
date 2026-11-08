//! Windows sensor readers for the live monitor. Everything here works without admin rights:
//!
//! * CPU busy, speed vs base clock and firmware limit: Windows performance counters
//!   (`Processor Information`), the same ones Task Manager uses.
//! * GPU busy and dedicated VRAM in use: the `GPU Engine` and `GPU Adapter Memory` counters,
//!   which work for every GPU vendor.
//! * NVIDIA temperature, clock, power and thermal slowdown: NVML (`nvml.dll`, installed with the
//!   driver), loaded from System32 by full path.
//!
//! CPU temperature needs a kernel driver, which FrameWise deliberately doesn't ship, so it isn't
//! read.

use crate::monitor::SensorSample;
use windows::core::PCWSTR;
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW, PdhGetFormattedCounterValue,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_FMT_LARGE,
};

type PdhQuery = isize;
type PdhCounter = isize;

const PDH_MORE_DATA: u32 = 0x8000_07D2;

pub struct Sensors {
    query: PdhQuery,
    cpu_util: Option<PdhCounter>,
    cpu_perf: Option<PdhCounter>,
    cpu_limit: Option<PdhCounter>,
    gpu_engine: Option<PdhCounter>,
    gpu_mem: Option<PdhCounter>,
    nvml: Option<nvml_wrapper::Nvml>,
    vram_total: Option<u64>,
}

// The PDH query handle is only used from the thread that owns `Sensors`.
unsafe impl Send for Sensors {}

impl Sensors {
    pub fn new() -> Sensors {
        let mut query: PdhQuery = 0;
        // SAFETY: out-pointer to a local.
        let ok = unsafe { PdhOpenQueryW(PCWSTR::null(), 0, &mut query) } == 0;
        let add = |path: &str| -> Option<PdhCounter> {
            if !ok {
                return None;
            }
            let w: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
            let mut c: PdhCounter = 0;
            // SAFETY: w is NUL-terminated and outlives the call.
            (unsafe { PdhAddEnglishCounterW(query, PCWSTR(w.as_ptr()), 0, &mut c) } == 0).then_some(c)
        };
        let s = Sensors {
            query,
            cpu_util: add(r"\Processor Information(_Total)\% Processor Utility"),
            cpu_perf: add(r"\Processor Information(_Total)\% Processor Performance"),
            cpu_limit: add(r"\Processor Information(_Total)\% Performance Limit"),
            gpu_engine: add(r"\GPU Engine(*)\Utilization Percentage"),
            gpu_mem: add(r"\GPU Adapter Memory(*)\Dedicated Usage"),
            nvml: nvml_wrapper::Nvml::builder().lib_path(std::ffi::OsStr::new(r"C:\Windows\System32\nvml.dll")).init().ok(),
            vram_total: None,
        };
        // Rate counters need a first collection to have a baseline.
        if ok {
            // SAFETY: valid query.
            unsafe { PdhCollectQueryData(s.query) };
        }
        s
    }

    /// Whether NVIDIA's NVML is available (temperature, clock and thermal slowdown readings).
    pub fn has_nvml(&self) -> bool {
        self.nvml.is_some()
    }

    /// Reads one sample. Call about once a second; rate counters average over the interval.
    pub fn sample(&mut self) -> SensorSample {
        let mut s = SensorSample::default();
        // SAFETY: valid query (a failed open leaves every counter None).
        let collected = unsafe { PdhCollectQueryData(self.query) } == 0;
        if collected {
            s.cpu_pct = self.cpu_util.and_then(double).map(|v| v.clamp(0.0, 100.0) as f32);
            s.cpu_perf_pct = self.cpu_perf.and_then(double).map(|v| v as f32);
            s.cpu_cap_pct = self.cpu_limit.and_then(double).map(|v| v.clamp(0.0, 100.0) as f32);
            s.gpu_pct = self.gpu_engine.and_then(|c| array(c, PDH_FMT_DOUBLE)).map(|items: Vec<(String, f64)>| gpu_busy(&items));
            s.vram_used_bytes = self
                .gpu_mem
                .and_then(|c| array(c, PDH_FMT_LARGE))
                .map(|items| items.iter().map(|(_, v)| *v as u64).max().unwrap_or(0));
        }
        // RAM.
        {
            use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
            let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
            // SAFETY: sized struct.
            if unsafe { GlobalMemoryStatusEx(&mut m) }.is_ok() {
                s.ram_total_bytes = Some(m.ullTotalPhys);
                s.ram_used_bytes = Some(m.ullTotalPhys - m.ullAvailPhys);
            }
        }
        if let Some(nvml) = &self.nvml {
            use nvml_wrapper::bitmasks::device::ThrottleReasons;
            use nvml_wrapper::enum_wrappers::device::{Clock, TemperatureSensor};
            if let Ok(d) = nvml.device_by_index(0) {
                s.gpu_temp_c = d.temperature(TemperatureSensor::Gpu).ok().map(|t| t as f32);
                s.gpu_clock_mhz = d.clock_info(Clock::Graphics).ok();
                s.gpu_power_w = d.power_usage().ok().map(|mw| mw as f32 / 1000.0);
                if let Ok(r) = d.current_throttle_reasons() {
                    s.gpu_thermal_throttle = Some(r.intersects(ThrottleReasons::SW_THERMAL_SLOWDOWN | ThrottleReasons::HW_THERMAL_SLOWDOWN));
                    s.gpu_power_limited = Some(r.contains(ThrottleReasons::SW_POWER_CAP));
                }
                if self.vram_total.is_none() {
                    self.vram_total = d.memory_info().ok().map(|m| m.total);
                }
                if s.gpu_pct.is_none() {
                    s.gpu_pct = d.utilization_rates().ok().map(|u| u.gpu as f32);
                }
            }
        }
        s.vram_total_bytes = self.vram_total;
        s
    }
}

impl Drop for Sensors {
    fn drop(&mut self) {
        // SAFETY: closing our own query handle once.
        unsafe { PdhCloseQuery(self.query) };
    }
}

fn double(c: PdhCounter) -> Option<f64> {
    let mut v = PDH_FMT_COUNTERVALUE::default();
    // SAFETY: out-pointer to a local.
    let r = unsafe { PdhGetFormattedCounterValue(c, PDH_FMT_DOUBLE, None, &mut v) };
    // SAFETY: the union member matches the requested format.
    (r == 0 && v.CStatus == 0).then(|| unsafe { v.Anonymous.doubleValue })
}

/// All instances of a wildcard counter as (instance name, value).
fn array(c: PdhCounter, fmt: windows::Win32::System::Performance::PDH_FMT) -> Option<Vec<(String, f64)>> {
    let mut size = 0u32;
    let mut count = 0u32;
    // SAFETY: size query with no buffer.
    let r = unsafe { PdhGetFormattedCounterArrayW(c, fmt, &mut size, &mut count, None) };
    if r != PDH_MORE_DATA || size == 0 {
        return None;
    }
    // Items are followed by their instance-name strings in the same buffer; u64 alignment.
    let mut buf = vec![0u64; (size as usize).div_ceil(8)];
    let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
    // SAFETY: buffer has at least `size` bytes.
    if unsafe { PdhGetFormattedCounterArrayW(c, fmt, &mut size, &mut count, Some(items)) } != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        // SAFETY: PDH wrote `count` items; names point into the same buffer.
        let it = unsafe { &*items.add(i) };
        if it.FmtValue.CStatus != 0 {
            continue;
        }
        let name = unsafe { it.szName.to_string() }.unwrap_or_default();
        let v = unsafe {
            if fmt == PDH_FMT_LARGE {
                it.FmtValue.Anonymous.largeValue as f64
            } else {
                it.FmtValue.Anonymous.doubleValue
            }
        };
        out.push((name, v));
    }
    Some(out)
}

/// Task Manager's GPU figure: sum utilisation per engine type (across processes), take the
/// busiest engine type. Instance names look like `pid_123_luid_..._phys_0_eng_0_engtype_3D`.
pub fn gpu_busy(items: &[(String, f64)]) -> f32 {
    let mut per_type: std::collections::HashMap<String, f64> = Default::default();
    for (name, v) in items {
        let key = match (name.find("_luid_"), name.find("_engtype_")) {
            (Some(l), Some(e)) => format!("{}{}", &name[l..name[l..].find("_phys").map(|p| l + p).unwrap_or(l)], &name[e..]),
            _ => name.clone(),
        };
        *per_type.entry(key).or_default() += *v;
    }
    per_type.values().cloned().fold(0.0, f64::max).clamp(0.0, 100.0) as f32
}

#[cfg(test)]
mod tests {
    #[test]
    fn gpu_busy_takes_the_busiest_engine_type() {
        let items = vec![
            ("pid_1_luid_0x0_0x1_phys_0_eng_0_engtype_3D".to_string(), 40.0),
            ("pid_2_luid_0x0_0x1_phys_0_eng_0_engtype_3D".to_string(), 25.0),
            ("pid_2_luid_0x0_0x1_phys_0_eng_3_engtype_VideoDecode".to_string(), 30.0),
            ("pid_3_luid_0x0_0x1_phys_0_eng_5_engtype_Copy".to_string(), 5.0),
        ];
        assert_eq!(super::gpu_busy(&items), 65.0);
        assert_eq!(super::gpu_busy(&[("pid_1_luid_0x0_0x1_phys_0_eng_0_engtype_3D".into(), 180.0)]), 100.0);
    }
}

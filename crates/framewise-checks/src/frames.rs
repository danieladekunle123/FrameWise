//! Frame timing: parsing PresentMon's CSV output, frame-time statistics, and the before/after
//! comparison used by benchmark mode. Pure code, no Windows calls, so it's fully unit-tested.
//!
//! Definitions (shown in the interface too):
//! * **Average FPS** = frames ÷ total time.
//! * **1% low / 0.1% low** = the average frame rate of the slowest 1% / 0.1% of frames (at least
//!   one frame), as reviewers like GamersNexus and CapFrameX report it. A percentile definition
//!   was rejected: with exactly 1% hitches it lands on the boundary and hides them.
//! * A difference between two sets of runs only counts if it's bigger than the run-to-run
//!   variation inside either set, which needs at least two runs per side.

use serde::{Deserialize, Serialize};

/// One presented frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub pid: u32,
    pub app: String,
    /// Seconds since capture start.
    pub t: f64,
    /// Frame time in ms: PresentMon 2's `MsBetweenAppStart` (start of this frame to start of the
    /// next, its own "FrameTime"), or `MsBetweenPresents` from PresentMon 1.x. 2.x writes 0 in
    /// `MsBetweenPresents` for frames Windows composes, so that column can't be used there.
    pub ms: f32,
    /// Time the previous frame stayed on screen, if it was displayed (`MsBetweenDisplayChange`).
    pub displayed_ms: Option<f32>,
    /// GPU busy time for the frame (`MsGPUBusy`, or `MsGPUActive` in older versions).
    pub gpu_ms: Option<f32>,
}

/// Maps PresentMon's CSV header to column positions. Column names differ a little between
/// PresentMon versions, so each value has fallbacks and every column except the frame time and
/// application is optional.
#[derive(Clone, Debug)]
pub struct CsvLayout {
    app: usize,
    pid: Option<usize>,
    time: Option<usize>,
    /// Multiplier to seconds (PresentMon 2.6 reports `TimeInMs`; older versions seconds).
    time_scale: f64,
    ms: usize,
    displayed: Option<usize>,
    gpu: Option<usize>,
    columns: usize,
}

impl CsvLayout {
    pub fn from_header(header: &str) -> Option<CsvLayout> {
        let cols: Vec<String> = header.trim().split(',').map(|c| c.trim().to_ascii_lowercase()).collect();
        let find = |names: &[&str]| names.iter().find_map(|n| cols.iter().position(|c| c == n));
        let (time, time_scale) = match find(&["cpustarttime", "timeinseconds"]) {
            Some(i) => (Some(i), 1.0),
            None => (find(&["cpustarttimeinms", "timeinms"]), 0.001),
        };
        Some(CsvLayout {
            app: find(&["application"])?,
            pid: find(&["processid"]),
            time,
            time_scale,
            ms: find(&["msbetweenappstart", "msbetweenpresents", "frametime"])?,
            displayed: find(&["msbetweendisplaychange", "displayedtime"]),
            gpu: find(&["msgpubusy", "msgpuactive", "gpubusy"]),
            columns: cols.len(),
        })
    }

    /// Parses one data line. Returns None for lines that aren't frames (headers, warnings, lines
    /// with a missing or non-positive frame time).
    pub fn parse(&self, line: &str) -> Option<Frame> {
        let f: Vec<&str> = line.trim_end().split(',').collect();
        if f.len() < self.columns.min(self.ms + 1) {
            return None;
        }
        let num = |i: Option<usize>| i.and_then(|i| f.get(i)).and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite());
        let ms = num(Some(self.ms))?;
        if ms <= 0.0 || ms > 10_000.0 {
            return None;
        }
        let pos = |v: f64| if v > 0.0 { Some(v as f32) } else { None };
        Some(Frame {
            app: f.get(self.app)?.trim().to_string(),
            pid: num(self.pid).map(|p| p as u32).unwrap_or(0),
            t: num(self.time).unwrap_or(0.0) * self.time_scale,
            ms: ms as f32,
            displayed_ms: num(self.displayed).and_then(pos),
            gpu_ms: num(self.gpu).and_then(pos),
        })
    }
}

/// Statistics for one run (or one window of live frames).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FrameStats {
    pub frames: u64,
    pub duration_s: f64,
    pub avg_fps: f64,
    pub low_1: f64,
    pub low_01: f64,
    pub avg_ms: f64,
    pub p50_ms: f64,
    pub p99_ms: f64,
    pub p999_ms: f64,
    pub max_ms: f64,
    /// Frames that took more than twice the median and at least 8 ms longer: visible hitches.
    pub stutters: u64,
}

/// The value at percentile `p` (0–100) of sorted data, by linear interpolation.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (p / 100.0) * (sorted.len() - 1) as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    sorted[lo] + (sorted[hi] - sorted[lo]) * (rank - lo as f64)
}

pub fn stats(frame_ms: &[f32]) -> FrameStats {
    if frame_ms.is_empty() {
        return FrameStats::default();
    }
    let mut v: Vec<f64> = frame_ms.iter().map(|&x| x as f64).collect();
    let total: f64 = v.iter().sum();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let fps = |ms: f64| if ms > 0.0 { 1000.0 / ms } else { 0.0 };
    let p50 = percentile(&v, 50.0);
    let p99 = percentile(&v, 99.0);
    let p999 = percentile(&v, 99.9);
    // Average of the slowest `share` of frames (v is sorted ascending, so they're at the end).
    let slowest = |share: f64| {
        let n = ((v.len() as f64 * share).ceil() as usize).clamp(1, v.len());
        v[v.len() - n..].iter().sum::<f64>() / n as f64
    };
    FrameStats {
        frames: v.len() as u64,
        duration_s: total / 1000.0,
        avg_fps: if total > 0.0 { v.len() as f64 * 1000.0 / total } else { 0.0 },
        low_1: fps(slowest(0.01)),
        low_01: fps(slowest(0.001)),
        avg_ms: total / v.len() as f64,
        p50_ms: p50,
        p99_ms: p99,
        p999_ms: p999,
        max_ms: *v.last().unwrap(),
        stutters: frame_ms.iter().filter(|&&x| (x as f64) > (p50 * 2.0).max(p50 + 8.0)).count() as u64,
    }
}

/// Reduces a frame-time series to at most `points` points for a graph, keeping the slowest frame
/// of each bucket so spikes stay visible.
pub fn graph_points(frame_ms: &[f32], points: usize) -> Vec<f32> {
    if points == 0 || frame_ms.len() <= points {
        return frame_ms.to_vec();
    }
    let size = frame_ms.len() as f64 / points as f64;
    (0..points)
        .map(|i| {
            let a = (i as f64 * size) as usize;
            let b = (((i + 1) as f64 * size) as usize).clamp(a + 1, frame_ms.len());
            frame_ms[a..b].iter().cloned().fold(0.0f32, f32::max)
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Better,
    Worse,
    /// The difference is no bigger than the variation between runs on the same side.
    WithinVariation,
    /// Fewer than two runs on a side, so variation can't be measured.
    NeedMoreRuns,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MetricComparison {
    pub metric: String,
    pub before: f64,
    pub after: f64,
    pub diff: f64,
    pub diff_pct: f64,
    /// Run-to-run variation: the larger spread (max − min) of the two sides.
    pub variation: f64,
    pub verdict: Verdict,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Comparison {
    pub before_runs: usize,
    pub after_runs: usize,
    pub metrics: Vec<MetricComparison>,
    /// The headline: the average FPS verdict.
    pub verdict: Verdict,
}

/// Compares two sets of runs. Higher is better for every metric compared (FPS values).
pub fn compare(before: &[FrameStats], after: &[FrameStats]) -> Comparison {
    let metric = |name: &str, get: fn(&FrameStats) -> f64| {
        let b: Vec<f64> = before.iter().map(get).collect();
        let a: Vec<f64> = after.iter().map(get).collect();
        let mean = |x: &[f64]| if x.is_empty() { 0.0 } else { x.iter().sum::<f64>() / x.len() as f64 };
        let spread = |x: &[f64]| {
            let max = x.iter().cloned().fold(f64::MIN, f64::max);
            let min = x.iter().cloned().fold(f64::MAX, f64::min);
            if x.len() < 2 { 0.0 } else { max - min }
        };
        let (bm, am) = (mean(&b), mean(&a));
        let variation = spread(&b).max(spread(&a));
        let diff = am - bm;
        let verdict = if b.len() < 2 || a.len() < 2 {
            Verdict::NeedMoreRuns
        } else if diff > variation {
            Verdict::Better
        } else if -diff > variation {
            Verdict::Worse
        } else {
            Verdict::WithinVariation
        };
        MetricComparison {
            metric: name.into(),
            before: bm,
            after: am,
            diff,
            diff_pct: if bm > 0.0 { diff / bm * 100.0 } else { 0.0 },
            variation,
            verdict,
        }
    };
    let metrics = vec![
        metric("avg_fps", |s| s.avg_fps),
        metric("low_1", |s| s.low_1),
        metric("low_01", |s| s.low_01),
    ];
    Comparison { before_runs: before.len(), after_runs: after.len(), verdict: metrics[0].verdict, metrics }
}

/// How repeatable a set of runs of the same scene is: the spread of average FPS as a percentage
/// of the mean. Used for the phase 3 gate and shown next to a set of runs.
pub fn spread_pct(runs: &[FrameStats]) -> Option<f64> {
    if runs.len() < 2 {
        return None;
    }
    let v: Vec<f64> = runs.iter().map(|s| s.avg_fps).collect();
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    let max = v.iter().cloned().fold(f64::MIN, f64::max);
    let min = v.iter().cloned().fold(f64::MAX, f64::min);
    (mean > 0.0).then(|| (max - min) / mean * 100.0)
}

/// Frame times as compact little-endian f32 bytes, for the per-run frame-time file.
pub fn encode_frametimes(ms: &[f32]) -> Vec<u8> {
    ms.iter().flat_map(|v| v.to_le_bytes()).collect()
}

pub fn decode_frametimes(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const V2_HEADER: &str = "Application,ProcessID,SwapChainAddress,PresentRuntime,SyncInterval,PresentFlags,AllowsTearing,PresentMode,FrameType,CPUStartTime,MsBetweenSimulationStart,MsBetweenPresents,MsBetweenDisplayChange,MsInPresentAPI,MsRenderPresentLatency,MsUntilDisplayed,MsPCLatency,MsBetweenAppStart,MsCPUBusy,MsCPUWait,MsGPULatency,MsGPUTime,MsGPUBusy,MsGPUWait";
    /// The header PresentMon 2.6.0 actually writes (captured on the development PC).
    const V26_HEADER: &str = "Application,ProcessID,SwapChainAddress,PresentRuntime,SyncInterval,PresentFlags,AllowsTearing,PresentMode,TimeInMs,MsBetweenSimulationStart,MsBetweenPresents,MsBetweenDisplayChange,MsInPresentAPI,MsRenderPresentLatency,MsUntilDisplayed,CPUStartTimeInMs,MsBetweenAppStart,MsCPUBusy,MsCPUWait,MsGPULatency,MsGPUTime,MsGPUBusy,MsGPUWait,MsAnimationError,AnimationTime,MsFlipDelay";
    const V1_HEADER: &str = "Application,ProcessID,SwapChainAddress,Runtime,SyncInterval,PresentFlags,AllowsTearing,PresentMode,Dropped,TimeInSeconds,msInPresentAPI,msBetweenPresents,msBetweenDisplayChange,msUntilRenderComplete,msUntilDisplayed,msGPUActive";

    #[test]
    fn parses_v2_and_v1_csv() {
        let l = CsvLayout::from_header(V2_HEADER).unwrap();
        let f = l
            .parse("game.exe,1234,0x1,DXGI,0,512,1,Hardware: Independent Flip,Application,1.250,NA,6.944,6.950,0.1,7.0,8.0,NA,6.94,5.0,1.9,0.5,6.0,5.5,0.4")
            .unwrap();
        // Frame time from MsBetweenAppStart (6.94), not MsBetweenPresents (6.944).
        assert_eq!((f.app.as_str(), f.pid, f.ms), ("game.exe", 1234, 6.94));
        assert_eq!(f.t, 1.25);
        assert_eq!(f.displayed_ms, Some(6.95));
        assert_eq!(f.gpu_ms, Some(5.5));
        // Not displayed (dropped): display change is NA.
        let f = l.parse("game.exe,1234,0x1,DXGI,0,512,1,Composed: Flip,Application,1.26,NA,7.1,NA,0.1,7.0,NA,NA,7.0,5,2,0.5,6,5.5,0.4").unwrap();
        assert_eq!(f.displayed_ms, None);

        // 2.6: times in milliseconds.
        let l = CsvLayout::from_header(V26_HEADER).unwrap();
        let f = l.parse("test_scene.exe,34304,0x1,DXGI,0,0,0,Hardware: Independent Flip,2500.5,NA,8.4,8.3,0.1,9,10,2500.4,8.4,6,2.4,0.5,7.9,7.8,0.6,NA,NA,NA").unwrap();
        assert_eq!((f.pid, f.ms, f.gpu_ms), (34304, 8.4, Some(7.8)));
        assert!((f.t - 2.5004).abs() < 1e-9, "{}", f.t);
        // A composed frame: 2.6 writes MsBetweenPresents = 0 but a real MsBetweenAppStart (captured row).
        let f = l.parse("test_scene.exe,9152,0x1CFAC186D30,DXGI,0,512,0,Composed: Flip,32669.8664,NA,0.00000000000000,NA,9.27600000000000,36.52470000000000,NA,32688.2996,9.2760,0.0000,9.2760,9.0603,9.0312,9.0265,0.0047,NA,NA,NA").unwrap();
        assert_eq!(f.ms, 9.276);

        let l = CsvLayout::from_header(V1_HEADER).unwrap();
        let f = l.parse("old.exe,99,0x2,DXGI,1,0,0,Composed: Flip,0,3.5,0.2,16.67,16.67,10,20,9.1").unwrap();
        assert_eq!((f.pid, f.ms, f.gpu_ms), (99, 16.67, Some(9.1)));
        assert_eq!(f.t, 3.5);
    }

    #[test]
    fn rejects_non_frames() {
        let l = CsvLayout::from_header(V2_HEADER).unwrap();
        assert!(l.parse(V2_HEADER).is_none());
        assert!(l.parse("warning: something").is_none());
        assert!(l.parse("game.exe,1,0x1,DXGI,0,0,0,x,y,1.0,NA,0,0,0,0,0,0,0,0,0,0,0,0,0").is_none(), "zero frame time");
        assert!(CsvLayout::from_header("not,a,presentmon,header").is_none());
    }

    #[test]
    fn stats_of_a_steady_run() {
        // 1000 frames at 10 ms = 100 FPS, 10 s.
        let s = stats(&vec![10.0; 1000]);
        assert_eq!(s.frames, 1000);
        assert!((s.avg_fps - 100.0).abs() < 1e-9);
        assert!((s.low_1 - 100.0).abs() < 1e-9 && (s.low_01 - 100.0).abs() < 1e-9);
        assert!((s.duration_s - 10.0).abs() < 1e-9);
        assert_eq!(s.stutters, 0);
    }

    #[test]
    fn lows_follow_the_slowest_frames() {
        // 990 frames at 10 ms and 10 hitches of 50 ms: average drops a little, 1% low a lot.
        let mut v = vec![10.0f32; 990];
        v.extend(std::iter::repeat(50.0).take(10));
        let s = stats(&v);
        assert!((s.avg_fps - 1000.0 / 10.4).abs() < 0.01, "{}", s.avg_fps);
        // The slowest 1% (10 frames) are exactly the hitches.
        assert!((s.low_1 - 20.0).abs() < 1e-6, "1% low {}", s.low_1);
        assert!((s.low_01 - 20.0).abs() < 1e-6, "0.1% low {}", s.low_01);
        assert_eq!(s.stutters, 10);
        assert_eq!(s.max_ms, 50.0);
        // Half as many hitches: the 1% low averages hitches with normal frames.
        let mut v = vec![10.0f32; 995];
        v.extend(std::iter::repeat(50.0).take(5));
        let s = stats(&v);
        assert!((s.low_1 - 1000.0 / 30.0).abs() < 1e-6, "1% low {}", s.low_1);
        assert_eq!(s.stutters, 5);
    }

    #[test]
    fn graph_keeps_spikes() {
        let mut v = vec![5.0f32; 10_000];
        v[4321] = 80.0;
        let g = graph_points(&v, 500);
        assert_eq!(g.len(), 500);
        assert!(g.contains(&80.0));
        assert_eq!(graph_points(&[1.0, 2.0], 500), vec![1.0, 2.0]);
    }

    fn run(avg: f64) -> FrameStats {
        FrameStats { avg_fps: avg, low_1: avg * 0.7, low_01: avg * 0.5, ..Default::default() }
    }

    #[test]
    fn comparison_respects_run_to_run_variation() {
        // Before 100/102, after 110/111: +9.5 FPS against 2 FPS of variation.
        let c = compare(&[run(100.0), run(102.0)], &[run(110.0), run(111.0)]);
        assert_eq!(c.verdict, Verdict::Better);
        assert!((c.metrics[0].diff - 9.5).abs() < 1e-9);
        assert!((c.metrics[0].diff_pct - 9.5 / 101.0 * 100.0).abs() < 1e-9);
        // Same difference but noisy runs: not an improvement.
        let c = compare(&[run(95.0), run(107.0)], &[run(110.0), run(111.0)]);
        assert_eq!(c.verdict, Verdict::WithinVariation);
        // Worse.
        assert_eq!(compare(&[run(110.0), run(111.0)], &[run(100.0), run(101.0)]).verdict, Verdict::Worse);
        // One run per side can't show variation.
        assert_eq!(compare(&[run(100.0)], &[run(120.0), run(121.0)]).verdict, Verdict::NeedMoreRuns);
    }

    #[test]
    fn spread_and_files() {
        assert_eq!(spread_pct(&[run(100.0)]), None);
        assert!((spread_pct(&[run(99.0), run(101.0)]).unwrap() - 2.0).abs() < 1e-9);
        let v = vec![1.5f32, 16.6667, 33.3];
        assert_eq!(decode_frametimes(&encode_frametimes(&v)), v);
    }
}

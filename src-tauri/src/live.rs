//! Live monitor and benchmark mode (phase 3).
//!
//! One monitor thread owns the capture (PresentMon, direct or through the helper) and the sensors.
//! Four times a second it reads new frames; once a second it sends a `monitor-tick` to the
//! interface and the overlay. A benchmark is a state machine on the same thread: countdown, then
//! recording one app's frames for a fixed time, then saving the run.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use framewise_checks::frames::{self, CsvLayout, FrameStats};
use framewise_checks::monitor::{MonitorTick, ThrottleDetector, ThrottleKind};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::capture::{Capture, Mode};
use crate::elevate::ApiError;
use crate::{db, with_db, AppState};

/// How long an app may go without presenting before it's no longer "presenting".
const ACTIVE_WINDOW: Duration = Duration::from_secs(2);
/// Frames kept per app for live statistics.
const KEEP_SECONDS: f64 = 10.0;
/// Live FPS / 1% low window.
const STATS_SECONDS: f64 = 5.0;
/// How long a benchmark waits for the game's first frames after its countdown.
const START_GRACE: Duration = Duration::from_secs(6);

#[derive(Clone, Debug, Deserialize)]
pub struct BenchmarkRequest {
    /// Measure this process; otherwise whatever app is in front when recording starts.
    pub pid: Option<u32>,
    pub duration_s: u32,
    /// Seconds to switch back to the game before recording starts.
    pub delay_s: u32,
    #[serde(default)]
    pub label: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct BenchmarkProgress {
    /// countdown | recording | saving
    pub phase: &'static str,
    pub seconds_left: f64,
    pub app: Option<String>,
    pub fps: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct MonitorStatus {
    pub running: bool,
    pub starting: bool,
    pub mode: Option<Mode>,
    /// PresentMon can run without a UAC prompt for this user.
    pub direct_ok: bool,
    pub benchmarking: bool,
    pub pinned: Option<u32>,
    pub overlay: bool,
    pub nvml: bool,
    pub hotkey: String,
}

pub const HOTKEY: &str = "Ctrl+Shift+F10";

#[derive(Default)]
struct Control {
    stop: bool,
    pinned: Option<u32>,
    bench: Option<BenchmarkRequest>,
    cancel_bench: bool,
    /// Stop the monitor when the current benchmark ends (it was started for the benchmark).
    stop_after_bench: bool,
}

#[derive(Clone, Default)]
pub struct Live {
    ctl: Arc<Mutex<Control>>,
    running: Arc<AtomicBool>,
    starting: Arc<AtomicBool>,
    benchmarking: Arc<AtomicBool>,
    status: Arc<Mutex<MonitorStatus>>,
}

impl Live {
    pub fn status(&self) -> MonitorStatus {
        let mut s = self.status.lock().map(|s| s.clone()).unwrap_or_default();
        s.running = self.running.load(Ordering::SeqCst);
        s.starting = self.starting.load(Ordering::SeqCst);
        s.benchmarking = self.benchmarking.load(Ordering::SeqCst);
        s.pinned = self.ctl.lock().ok().and_then(|c| c.pinned);
        s.direct_ok = crate::capture::can_capture_directly();
        s.hotkey = HOTKEY.into();
        s
    }

    /// Starts the monitor thread. Returns once capture is running (after any UAC prompt).
    pub fn start(&self, app: &AppHandle) -> Result<(), ApiError> {
        if self.running.load(Ordering::SeqCst) {
            return Ok(());
        }
        if self.starting.swap(true, Ordering::SeqCst) {
            return Err(ApiError::new("busy", "The monitor is already starting."));
        }
        let hwnd = app.get_webview_window("main").and_then(|w| w.hwnd().ok()).map(|h| h.0 as isize).unwrap_or(0);
        let (tx, rx) = std::sync::mpsc::channel();
        let me = self.clone();
        let app2 = app.clone();
        if let Ok(mut c) = self.ctl.lock() {
            c.stop = false;
            c.cancel_bench = false;
        }
        std::thread::Builder::new()
            .name("monitor".into())
            .spawn(move || {
                // The capture is created on this thread and stays here (the helper pipe is per-thread).
                match Capture::start(hwnd) {
                    Ok((capture, mode)) => {
                        let _ = tx.send(Ok(mode));
                        me.run(&app2, capture, mode);
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                    }
                }
            })
            .map_err(|e| ApiError::new("error", e.to_string()))?;
        let r = rx.recv().unwrap_or_else(|_| Err(ApiError::new("error", "The monitor stopped while starting.")));
        self.starting.store(false, Ordering::SeqCst);
        if let Ok(mode) = &r {
            self.running.store(true, Ordering::SeqCst);
            if let Ok(mut s) = self.status.lock() {
                s.mode = Some(*mode);
            }
        }
        r.map(|_| ())
    }

    pub fn stop(&self) {
        if let Ok(mut c) = self.ctl.lock() {
            c.stop = true;
        }
    }

    pub fn pin(&self, pid: Option<u32>) {
        if let Ok(mut c) = self.ctl.lock() {
            c.pinned = pid;
        }
    }

    pub fn start_benchmark(&self, app: &AppHandle, req: BenchmarkRequest) -> Result<(), ApiError> {
        if self.benchmarking.load(Ordering::SeqCst) {
            return Err(ApiError::new("busy", "A benchmark is already recording."));
        }
        let was_running = self.running.load(Ordering::SeqCst);
        if !was_running {
            self.start(app)?;
        }
        let mut c = self.ctl.lock().map_err(|_| ApiError::new("error", "lock"))?;
        c.cancel_bench = false;
        c.stop_after_bench = !was_running;
        c.bench = Some(BenchmarkRequest {
            duration_s: req.duration_s.clamp(10, 600),
            delay_s: req.delay_s.min(60),
            label: req.label.chars().filter(|c| !c.is_control()).take(80).collect(),
            ..req
        });
        self.benchmarking.store(true, Ordering::SeqCst);
        Ok(())
    }

    pub fn cancel_benchmark(&self) {
        if let Ok(mut c) = self.ctl.lock() {
            c.cancel_bench = true;
        }
    }

    /// Hotkey: start a benchmark of the app in front, or cancel the one recording.
    pub fn hotkey(&self, app: &AppHandle) {
        if self.benchmarking.load(Ordering::SeqCst) {
            self.cancel_benchmark();
            return;
        }
        // Starting the monitor may need a UAC prompt, and Windows doesn't pop a prompt up over a
        // game (it only flashes in the taskbar). So in a game, the hotkey only uses a monitor
        // that's already running, or one that can start without a prompt.
        if !self.running.load(Ordering::SeqCst) && !crate::capture::can_capture_directly() {
            let _ = app.emit(
                "benchmark-failed",
                "Start the monitor in FrameWise first (Windows asks for permission), then press the hotkey in your game.",
            );
            return;
        }
        let r = self.start_benchmark(app, BenchmarkRequest { pid: foreground_pid(), duration_s: 60, delay_s: 0, label: String::new() });
        if let Err(e) = r {
            let _ = app.emit("benchmark-failed", e.message);
        }
    }

    fn run(&self, app: &AppHandle, mut capture: Capture, mode: Mode) {
        let own = own_pids();
        let mut sensors = framewise_checks::sensors::Sensors::new();
        if let Ok(mut s) = self.status.lock() {
            s.nvml = sensors.has_nvml();
            s.mode = Some(mode);
        }
        let mut layout: Option<CsvLayout> = None;
        let mut apps: HashMap<u32, AppFrames> = HashMap::new();
        let mut latest_t = 0.0f64;
        let mut detector = ThrottleDetector::default();
        let mut last_tick = Instant::now();
        let mut graph: Vec<f32> = Vec::new();
        let mut bench: Option<Bench> = None;
        let mut stop_reason: Option<String> = None;

        loop {
            std::thread::sleep(Duration::from_millis(250));
            let (stop, pinned, new_bench, cancel) = match self.ctl.lock() {
                Ok(mut c) => (c.stop, c.pinned, c.bench.take(), std::mem::take(&mut c.cancel_bench)),
                Err(_) => (true, None, None, false),
            };
            if stop {
                break;
            }

            // New frames.
            match capture.read(5000) {
                Ok(lines) => {
                    for line in lines {
                        if layout.is_none() || line.starts_with("Application,") {
                            if let Some(l) = CsvLayout::from_header(&line) {
                                layout = Some(l);
                                continue;
                            }
                        }
                        let Some(f) = layout.as_ref().and_then(|l| l.parse(&line)) else { continue };
                        if own.contains(&f.pid) {
                            continue;
                        }
                        latest_t = latest_t.max(f.t);
                        let a = apps.entry(f.pid).or_insert_with(|| AppFrames { app: f.app.clone(), frames: VecDeque::new(), last_seen: Instant::now() });
                        a.last_seen = Instant::now();
                        a.frames.push_back((f.t, f.ms));
                        // The run follows the game's own frames: PresentMon reports each app with its
                        // own delay, so another app's timestamps can't start or end the window.
                        if let Some(Bench::Recording { pid, start_t, t0, target_t, duration, frames, .. }) = bench.as_mut() {
                            if f.pid == *pid && f.t >= *start_t {
                                let first = *t0.get_or_insert(f.t);
                                if f.t < first + *duration {
                                    frames.push(f.ms);
                                }
                                *target_t = target_t.max(f.t);
                            }
                        }
                    }
                }
                Err(e) => {
                    stop_reason = Some(e.message);
                    break;
                }
            }
            for a in apps.values_mut() {
                while a.frames.front().is_some_and(|(t, _)| *t < latest_t - KEEP_SECONDS) {
                    a.frames.pop_front();
                }
            }
            apps.retain(|_, a| a.last_seen.elapsed() < Duration::from_secs(30));
            let target = pick_target(&apps, pinned);

            // Benchmark state machine.
            if cancel && bench.is_some() {
                bench = None;
                self.benchmarking.store(false, Ordering::SeqCst);
                let _ = app.emit("benchmark-failed", "Benchmark cancelled.");
            }
            if let Some(req) = new_bench {
                bench = Some(Bench::Countdown { until: Instant::now() + Duration::from_secs(req.delay_s as u64), req });
            }
            let mut finished: Option<Result<db::BenchmarkRow, String>> = None;
            match bench.take() {
                Some(Bench::Countdown { until, req }) if Instant::now() >= until => {
                    let pid = req.pid.filter(|p| apps.contains_key(p)).or(target);
                    // A monitor that has only just started (the hotkey starts one) hasn't seen any
                    // frames yet: give the game a few seconds to show up before giving up.
                    let waiting = pid.is_none_or(|p| !apps.contains_key(&p)) && Instant::now() < until + START_GRACE;
                    match pid.and_then(|p| apps.get(&p).map(|a| (p, a.app.clone()))) {
                        _ if waiting => bench = Some(Bench::Countdown { until, req }),
                        Some((pid, name)) => {
                            bench = Some(Bench::Recording {
                                pid,
                                app: name,
                                start_t: latest_t,
                                t0: None,
                                target_t: 0.0,
                                duration: req.duration_s as f64,
                                deadline: Instant::now() + Duration::from_secs(req.duration_s as u64 + 10),
                                frames: Vec::new(),
                                req,
                            });
                        }
                        None => finished = Some(Err("No game is drawing frames yet. Start your game, then record the benchmark.".into())),
                    }
                }
                Some(Bench::Countdown { until, req }) => {
                    let _ = app.emit(
                        "benchmark-progress",
                        BenchmarkProgress {
                            phase: "countdown",
                            seconds_left: until.saturating_duration_since(Instant::now()).as_secs_f64(),
                            app: target.and_then(|p| apps.get(&p)).map(|a| a.app.clone()),
                            fps: None,
                        },
                    );
                    bench = Some(Bench::Countdown { until, req });
                }
                Some(Bench::Recording { pid, app: name, start_t, t0, target_t, duration, deadline, frames, req }) => {
                    let covered = t0.map(|t| target_t - t).unwrap_or(0.0);
                    let complete = covered >= duration;
                    let gone = !apps.contains_key(&pid);
                    if complete || Instant::now() >= deadline || gone {
                        let _ = app.emit("benchmark-progress", BenchmarkProgress { phase: "saving", seconds_left: 0.0, app: Some(name.clone()), fps: None });
                        // Only frames inside the window (a batch can run past the end).
                        let kept = frames_until(&frames, duration);
                        let captured: f64 = kept.iter().map(|ms| *ms as f64 / 1000.0).sum();
                        let window = covered.min(duration);
                        finished = Some(if covered < duration * 0.95 {
                            Err(format!("{name} closed or stopped drawing frames during the benchmark, so the run wasn't saved."))
                        } else if captured < window * 0.95 {
                            // The frames don't add up to the time they span: PresentMon missed some
                            // (it can, for a game that started at the same moment as the monitor).
                            Err(format!(
                                "PresentMon missed {:.0}% of {name}'s frames during this run, so it wasn't saved. Record it again.",
                                (1.0 - captured / window) * 100.0
                            ))
                        } else {
                            save_run(app, &name, &req.label, &kept)
                        });
                    } else {
                        let s = frames::stats(&frames);
                        let _ = app.emit(
                            "benchmark-progress",
                            BenchmarkProgress { phase: "recording", seconds_left: (duration - covered).max(0.0), app: Some(name.clone()), fps: (s.frames > 0).then_some(s.avg_fps) },
                        );
                        bench = Some(Bench::Recording { pid, app: name, start_t, t0, target_t, duration, deadline, frames, req });
                    }
                }
                None => {}
            }
            if let Some(r) = finished {
                self.benchmarking.store(false, Ordering::SeqCst);
                match r {
                    Ok(row) => {
                        let _ = app.emit("benchmark-done", row);
                    }
                    Err(e) => {
                        let _ = app.emit("benchmark-failed", e);
                    }
                }
                let stop_now = self.ctl.lock().map(|mut c| std::mem::take(&mut c.stop_after_bench)).unwrap_or(false);
                if stop_now {
                    break;
                }
            }

            // Graph frames for the target since the last tick.
            if let Some(a) = target.and_then(|p| apps.get(&p)) {
                let since = latest_t - last_tick.elapsed().as_secs_f64() - 0.25;
                graph.extend(a.frames.iter().filter(|(t, _)| *t > since).map(|(_, ms)| *ms));
            }

            if last_tick.elapsed() >= Duration::from_secs(1) {
                last_tick = Instant::now();
                let sensors_now = sensors.sample();
                let throttling: Vec<ThrottleKind> = detector.feed(&sensors_now);
                let mut tick = MonitorTick { sensors: sensors_now, throttling, ..Default::default() };
                if let Some(pid) = target {
                    if let Some(a) = apps.get(&pid) {
                        let recent: Vec<f32> = a.frames.iter().filter(|(t, _)| *t >= latest_t - STATS_SECONDS).map(|(_, ms)| *ms).collect();
                        let s: FrameStats = frames::stats(&recent);
                        tick.app = Some(a.app.clone());
                        tick.pid = Some(pid);
                        if s.frames > 0 {
                            tick.fps = Some(s.avg_fps);
                            tick.low_1 = Some(s.low_1);
                            tick.frame_ms = Some(s.avg_ms);
                        }
                    }
                }
                let mut g = std::mem::take(&mut graph);
                if g.len() > 400 {
                    g = frames::graph_points(&g, 400);
                }
                tick.frames = g;
                let mut presenting: Vec<(String, u32, u32)> = apps
                    .iter()
                    .filter(|(_, a)| a.last_seen.elapsed() < ACTIVE_WINDOW)
                    .map(|(pid, a)| (a.app.clone(), *pid, a.frames.iter().filter(|(t, _)| *t >= latest_t - 1.0).count() as u32))
                    .collect();
                presenting.sort_by(|a, b| b.2.cmp(&a.2));
                tick.presenting = presenting;
                let _ = app.emit("monitor-tick", tick);
            }
        }

        drop(capture);
        if bench.is_some() || self.benchmarking.swap(false, Ordering::SeqCst) {
            let _ = app.emit("benchmark-failed", "The monitor stopped, so the benchmark was cancelled.");
        }
        self.running.store(false, Ordering::SeqCst);
        if let Ok(mut s) = self.status.lock() {
            s.mode = None;
        }
        let _ = app.emit("monitor-stopped", stop_reason);
    }
}

struct AppFrames {
    app: String,
    /// (PresentMon time in s, frame time in ms)
    frames: VecDeque<(f64, f32)>,
    last_seen: Instant,
}

enum Bench {
    Countdown { until: Instant, req: BenchmarkRequest },
    Recording {
        pid: u32,
        app: String,
        /// Frames from before this PresentMon time belong to before the run.
        start_t: f64,
        /// Time of the game's first frame in the run.
        t0: Option<f64>,
        /// Time of the game's newest frame so far.
        target_t: f64,
        duration: f64,
        deadline: Instant,
        frames: Vec<f32>,
        req: BenchmarkRequest,
    },
}

/// The frames whose cumulative time fits in `seconds` (drops frames past the end of the window).
fn frames_until(frames: &[f32], seconds: f64) -> Vec<f32> {
    let mut total = 0.0f64;
    frames
        .iter()
        .take_while(|&&ms| {
            total += ms as f64 / 1000.0;
            total <= seconds + 0.001
        })
        .copied()
        .collect()
}

/// Which app to show: the pinned one, else the app in front if it's presenting, else the busiest.
fn pick_target(apps: &HashMap<u32, AppFrames>, pinned: Option<u32>) -> Option<u32> {
    let active = |pid: &u32| apps.get(pid).is_some_and(|a| a.last_seen.elapsed() < ACTIVE_WINDOW);
    if let Some(p) = pinned.filter(|p| apps.contains_key(p)) {
        return Some(p);
    }
    if let Some(p) = foreground_pid().filter(active) {
        return Some(p);
    }
    apps.iter().filter(|(p, _)| active(p)).max_by_key(|(_, a)| a.frames.len()).map(|(p, _)| *p)
}

pub fn foreground_pid() -> Option<u32> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    // SAFETY: plain queries.
    unsafe {
        let w = GetForegroundWindow();
        if w.0.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(w, Some(&mut pid));
        (pid != 0).then_some(pid)
    }
}

/// FrameWise's own processes (the window draws frames too, but isn't what anyone wants measured).
fn own_pids() -> Vec<u32> {
    vec![std::process::id()]
}

fn save_run(app: &AppHandle, name: &str, label: &str, frame_ms: &[f32]) -> Result<db::BenchmarkRow, String> {
    if frame_ms.len() < 30 {
        return Err("Too few frames were recorded (was the game paused or minimised?), so the run wasn't saved.".into());
    }
    let stats = frames::stats(frame_ms);
    let at = crate::now_secs();
    let dir = db::benchmarks_dir();
    let _ = std::fs::create_dir_all(&dir);
    let file = format!("{at}-{}.ft", std::process::id() ^ (frame_ms.len() as u32));
    let file = std::fs::write(dir.join(&file), frames::encode_frametimes(frame_ms)).ok().map(|_| file);
    let state = app.state::<AppState>();
    let id = with_db(&state, |c| {
        let id = db::insert_benchmark(c, name, label, at, &stats, file.as_deref())?;
        db::prune_benchmark_files(c)?;
        Ok(id)
    })?;
    with_db(&state, |c| db::benchmarks_by_id(c, &[id]))?.into_iter().next().ok_or_else(|| "The run was saved but couldn't be read back.".into())
}

// ---------------------------------------------------------------------------------------------
// Commands

#[tauri::command]
pub fn monitor_status(state: State<'_, AppState>, app: AppHandle) -> MonitorStatus {
    let mut s = state.live.status();
    s.overlay = app.get_webview_window("overlay").is_some();
    s
}

#[tauri::command]
pub async fn monitor_start(state: State<'_, AppState>, app: AppHandle) -> Result<MonitorStatus, ApiError> {
    let live = state.live.clone();
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || live.start(&app2)).await.map_err(|e| ApiError::new("error", e.to_string()))??;
    Ok(state.live.status())
}

#[tauri::command]
pub fn monitor_stop(state: State<'_, AppState>) {
    state.live.stop();
}

#[tauri::command]
pub fn monitor_pin(state: State<'_, AppState>, pid: Option<u32>) {
    state.live.pin(pid);
}

#[tauri::command]
pub async fn benchmark_start(state: State<'_, AppState>, app: AppHandle, request: BenchmarkRequest) -> Result<(), ApiError> {
    let live = state.live.clone();
    tauri::async_runtime::spawn_blocking(move || live.start_benchmark(&app, request)).await.map_err(|e| ApiError::new("error", e.to_string()))?
}

#[tauri::command]
pub fn benchmark_cancel(state: State<'_, AppState>) {
    state.live.cancel_benchmark();
}

#[tauri::command]
pub fn benchmarks_list(state: State<'_, AppState>) -> Result<Vec<db::BenchmarkRow>, String> {
    with_db(&state, |c| db::benchmarks(c))
}

#[derive(Serialize)]
pub struct BenchmarkDetail {
    pub run: db::BenchmarkRow,
    /// Up to 600 points of frame time (ms), slowest frame per bucket.
    pub graph: Vec<f32>,
}

#[tauri::command]
pub fn benchmark_detail(state: State<'_, AppState>, id: i64) -> Result<BenchmarkDetail, String> {
    let run = with_db(&state, |c| db::benchmarks_by_id(c, &[id]))?.into_iter().next().ok_or("That run no longer exists.")?;
    let file = with_db(&state, |c| db::benchmark_file(c, id))?;
    let graph = file
        .and_then(|f| std::fs::read(db::benchmarks_dir().join(f)).ok())
        .map(|b| frames::graph_points(&frames::decode_frametimes(&b), 600))
        .unwrap_or_default();
    Ok(BenchmarkDetail { run, graph })
}

#[tauri::command]
pub fn benchmark_rename(state: State<'_, AppState>, id: i64, label: String) -> Result<(), String> {
    let label: String = label.chars().filter(|c| !c.is_control()).take(80).collect();
    with_db(&state, |c| db::rename_benchmark(c, id, &label))
}

#[tauri::command]
pub fn benchmark_delete(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    with_db(&state, |c| db::delete_benchmark(c, id))
}

#[derive(Serialize)]
pub struct CompareResult {
    pub comparison: frames::Comparison,
    pub before_spread_pct: Option<f64>,
    pub after_spread_pct: Option<f64>,
}

#[tauri::command]
pub fn benchmark_compare(state: State<'_, AppState>, before: Vec<i64>, after: Vec<i64>) -> Result<CompareResult, String> {
    let b: Vec<FrameStats> = with_db(&state, |c| db::benchmarks_by_id(c, &before))?.into_iter().map(|r| r.stats).collect();
    let a: Vec<FrameStats> = with_db(&state, |c| db::benchmarks_by_id(c, &after))?.into_iter().map(|r| r.stats).collect();
    Ok(CompareResult { comparison: frames::compare(&b, &a), before_spread_pct: frames::spread_pct(&b), after_spread_pct: frames::spread_pct(&a) })
}

/// One UAC prompt: adds the user to "Performance Log Users" so monitoring needs no prompt after
/// they next sign in.
#[tauri::command]
pub async fn monitor_allow_without_prompt(app: AppHandle) -> Result<(), ApiError> {
    let hwnd = app.get_webview_window("main").and_then(|w| w.hwnd().ok()).map(|h| h.0 as isize).unwrap_or(0);
    tauri::async_runtime::spawn_blocking(move || {
        let mut h = crate::elevate::Elevated::start(hwnd)?;
        h.call(&framewise_checks::tweak::HelperRequest::AllowMonitoringWithoutPrompt, Duration::from_secs(20)).map(|_| ())
    })
    .await
    .map_err(|e| ApiError::new("error", e.to_string()))?
}

#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Shows or hides the overlay: a small, click-through, always-on-top window. It sits over games
/// in borderless or windowed mode; exclusive full-screen games draw over it.
/// Async on purpose: creating a window from a synchronous command deadlocks on Windows.
#[tauri::command]
pub async fn overlay_set(app: AppHandle, visible: bool, corner: Option<Corner>) -> Result<(), String> {
    let existing = app.get_webview_window("overlay");
    if !visible {
        if let Some(w) = existing {
            let _ = w.close();
        }
        return Ok(());
    }
    let w = match existing {
        Some(w) => w,
        None => tauri::WebviewWindowBuilder::new(&app, "overlay", tauri::WebviewUrl::App("index.html#overlay".into()))
            .title("FrameWise overlay")
            .inner_size(230.0, 124.0)
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .resizable(false)
            .visible(false)
            .build()
            .map_err(|e| e.to_string())?,
    };
    let _ = w.set_ignore_cursor_events(true);
    if let Ok(Some(m)) = w.primary_monitor() {
        let (sw, sh) = (m.size().width as f64 / m.scale_factor(), m.size().height as f64 / m.scale_factor());
        let (x, y) = match corner.unwrap_or(Corner::TopLeft) {
            Corner::TopLeft => (16.0, 16.0),
            Corner::TopRight => (sw - 246.0, 16.0),
            Corner::BottomLeft => (16.0, sh - 180.0),
            Corner::BottomRight => (sw - 246.0, sh - 180.0),
        };
        let _ = w.set_position(tauri::LogicalPosition::new(x, y));
    }
    let _ = w.show();
    Ok(())
}

/// Debug builds: `framewise.exe --capture-test <seconds>` starts capture exactly as the monitor
/// does (directly, or through the helper with a UAC prompt), then prints frames per app.
#[cfg(debug_assertions)]
pub fn capture_selftest(seconds: u64) -> i32 {
    let (mut cap, mode) = match Capture::start(0) {
        Ok(c) => c,
        Err(e) => {
            println!("FAIL start: {} ({})", e.message, e.code);
            return 1;
        }
    };
    println!("capture running: {mode:?}");
    let mut layout: Option<CsvLayout> = None;
    let mut per_app: HashMap<String, Vec<f32>> = HashMap::new();
    // Diagnostics per app: (first t, last t, unparsed lines, sample unparsed line)
    let mut diag: HashMap<String, (f64, f64, u32, String)> = HashMap::new();
    // Per app: modes seen, previous t, gaps over 50 ms (count, total seconds), raw-line count
    let mut modes: HashMap<String, HashMap<String, u32>> = HashMap::new();
    let mut gaps: HashMap<String, (f64, u32, f64)> = HashMap::new();
    let mut header = String::new();
    // Optional: FW_SCENE=<test_scene.exe> runs the scene FW_ROUNDS times (15 s each) during capture
    // and prints how many of each run's frames PresentMon reported.
    let scene = std::env::var("FW_SCENE").ok();
    let rounds: u32 = std::env::var("FW_ROUNDS").ok().and_then(|r| r.parse().ok()).unwrap_or(5);
    let scene_runs: Arc<Mutex<Vec<(u32, String)>>> = Default::default();
    let scene_done = Arc::new(AtomicBool::new(scene.is_none()));
    if let Some(path) = scene.clone() {
        let runs = scene_runs.clone();
        let done = scene_done.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            for _ in 0..rounds {
                if let Ok(child) = std::process::Command::new(&path).args(["15", "6000"]).stdout(std::process::Stdio::piped()).spawn() {
                    let pid = child.id();
                    let out = child.wait_with_output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
                    runs.lock().unwrap().push((pid, out));
                }
                std::thread::sleep(Duration::from_secs(1));
            }
            done.store(true, Ordering::SeqCst);
        });
    }
    let end = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < end || !scene_done.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(250));
        match cap.read(5000) {
            Ok(lines) => {
                for line in lines {
                    if layout.is_none() {
                        if let Some(l) = CsvLayout::from_header(&line) {
                            header = line.clone();
                            layout = Some(l);
                        }
                        continue;
                    }
                    if let Some(f) = layout.as_ref().and_then(|l| l.parse(&line)) {
                        let key = format!("{} ({})", f.app, f.pid);
                        let mode = line.split(',').nth(7).unwrap_or("").to_string();
                        *modes.entry(key.clone()).or_default().entry(mode).or_default() += 1;
                        let g = gaps.entry(key.clone()).or_insert((f.t, 0, 0.0));
                        if f.t - g.0 > 0.05 {
                            g.1 += 1;
                            g.2 += f.t - g.0;
                        }
                        g.0 = f.t;
                        let d = diag.entry(key.clone()).or_insert((f.t, f.t, 0, String::new()));
                        d.1 = d.1.max(f.t);
                        per_app.entry(key).or_default().push(f.ms);
                    } else {
                        let cols: Vec<&str> = line.split(',').collect();
                        if cols.len() > 1 {
                            let key = format!("{} ({})", cols[0], cols[1]);
                            let d = diag.entry(key).or_insert((0.0, 0.0, 0, String::new()));
                            d.2 += 1;
                            if d.3.is_empty() {
                                d.3 = line.chars().take(300).collect();
                            }
                        }
                    }
                }
            }
            Err(e) => {
                println!("FAIL read: {} ({})", e.message, e.code);
                return 1;
            }
        }
    }
    drop(cap);
    println!("header: {}", header.chars().take(400).collect::<String>());
    for (pid, out) in scene_runs.lock().unwrap().iter() {
        let key = per_app.keys().find(|k| k.ends_with(&format!("({pid})"))).cloned();
        let got = key.as_ref().and_then(|k| per_app.get(k)).map(|v| v.len()).unwrap_or(0);
        let truth: f64 = out.split_whitespace().nth(2).and_then(|n| n.parse().ok()).unwrap_or(0.0);
        let gaps = key.as_ref().and_then(|k| gaps.get(k)).map(|g| (g.1, g.2)).unwrap_or((0, 0.0));
        println!("SCENE pid {pid}: PresentMon {got} frames of {truth} ({:.1}%), gaps over 50 ms: {} totalling {:.2} s · {out}", got as f64 / truth.max(1.0) * 100.0, gaps.0, gaps.1);
    }
    let mut v: Vec<_> = per_app.into_iter().collect();
    v.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    for (app, ms) in v.iter().take(8) {
        let s = frames::stats(ms);
        println!("{app}: {} frames, {:.1} FPS avg, 1% low {:.1}, 0.1% low {:.1}", s.frames, s.avg_fps, s.low_1, s.low_01);
        if let Some(d) = diag.get(app) {
            println!("    span {:.2} s, frame times sum {:.2} s, unparsed lines {}", d.1 - d.0, s.duration_s, d.2);
            if let Some(g) = gaps.get(app) {
                println!("    gaps over 50 ms: {} totalling {:.2} s", g.1, g.2);
            }
            if let Some(m) = modes.get(app) {
                println!("    present modes: {m:?}");
            }
            if !d.3.is_empty() {
                println!("    unparsed sample: {}", d.3);
            }
        }
    }
    if v.is_empty() {
        println!("FAIL no frames captured");
        return 1;
    }
    0
}
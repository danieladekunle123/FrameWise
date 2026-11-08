//! Self-footprint: FrameWise's own RAM and CPU, live.
//!
//! Every 2 seconds while the window is open this samples the app process and every process it
//! started (the WebView2 processes that draw the interface). Each minute one summed sample goes
//! into the database for the 7-day history. Phase 1 has only the UI; the watcher, helper and
//! monitor will report into the same structure.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use framewise_checks::procmeter;
use serde::Serialize;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

pub const SAMPLE_EVERY: Duration = Duration::from_secs(2);
const HISTORY_LEN: usize = 300; // 10 minutes at 2 s
const TREE_REFRESH_EVERY: Duration = Duration::from_secs(10);

/// Budget for the UI process group while the window is open. The design document estimated 80 MB;
/// measured WebView2 use is 85–91 MB on a typical PC, so the budget was raised to 100 MB on 2026-11-07
/// (see scripts/measure-footprint.ps1 and the README).
pub const UI_BUDGET_RAM_MB: f64 = 100.0;
pub const UI_BUDGET_CPU_PCT: f64 = 2.0;
/// Design document budget for the monitor while it runs.
pub const MONITOR_BUDGET_RAM_MB: f64 = 40.0;
pub const MONITOR_BUDGET_CPU_PCT: f64 = 1.5;

#[derive(Serialize, Clone, Debug)]
pub struct ProcessFootprint {
    pub role: String,
    pub name: String,
    pub count: u32,
    pub ram_mb: f64,
    pub cpu_pct: f64,
}

#[derive(Serialize, Clone, Copy, Debug)]
pub struct HistoryPoint {
    pub t: u64,
    pub ram_mb: f64,
    pub cpu_pct: f64,
}

#[derive(Serialize, Clone, Debug)]
pub struct FootprintReport {
    pub ram_mb: f64,
    pub cpu_pct: f64,
    pub processes: Vec<ProcessFootprint>,
    pub history: Vec<HistoryPoint>,
    pub budget_ram_mb: f64,
    pub budget_cpu_pct: f64,
    pub over_budget: bool,
}

#[derive(Serialize, Clone, Copy, Debug, Default)]
pub struct LiveStats {
    pub cpu_pct: f64,
    pub ram_used_gb: f64,
    pub ram_total_gb: f64,
}

#[derive(Default)]
struct Shared {
    report: Option<FootprintReport>,
    history: VecDeque<HistoryPoint>,
    live: LiveStats,
}

#[derive(Clone, Default)]
pub struct FootprintMeter {
    shared: Arc<Mutex<Shared>>,
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Our process and all of its descendants (WebView2 spawns a small process tree).
fn process_tree(sys: &System, root: Pid) -> Vec<(Pid, String)> {
    let mut children: HashMap<Pid, Vec<Pid>> = HashMap::new();
    for (pid, p) in sys.processes() {
        if let Some(parent) = p.parent() {
            children.entry(parent).or_default().push(*pid);
        }
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = vec![root];
    while let Some(pid) = stack.pop() {
        if !seen.insert(pid) {
            continue;
        }
        let name = sys.process(pid).map(|p| p.name().to_string_lossy().to_string()).unwrap_or_default();
        out.push((pid, name));
        if let Some(kids) = children.get(&pid) {
            stack.extend(kids.iter().copied());
        }
    }
    out
}

impl FootprintMeter {
    pub fn report(&self) -> Option<FootprintReport> {
        self.shared.lock().ok()?.report.clone()
    }

    pub fn live(&self) -> LiveStats {
        self.shared.lock().map(|s| s.live).unwrap_or_default()
    }

    /// Starts the sampler thread. `persist` is called about once a minute with a summed sample.
    pub fn start(&self, persist: impl Fn(u64, f64, f64) + Send + 'static) {
        let shared = self.shared.clone();
        std::thread::Builder::new()
            .name("footprint".into())
            .spawn(move || {
                let own = sysinfo::get_current_pid().unwrap_or(Pid::from_u32(std::process::id()));
                let ncpu = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64;
                let mut sys = System::new();
                let mut tree: Vec<(Pid, String)> = Vec::new();
                let mut last_tree = Instant::now() - TREE_REFRESH_EVERY;
                let mut prev_cpu: HashMap<Pid, u64> = HashMap::new();
                let mut prev_at = Instant::now();
                let mut last_persist = Instant::now();
                let mut minute_ram = Vec::new();
                let mut minute_cpu = Vec::new();

                loop {
                    if last_tree.elapsed() >= TREE_REFRESH_EVERY {
                        sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::new());
                        tree = process_tree(&sys, own);
                        // The profile watcher is its own process (not a child): find it by its PID file.
                        if let Some(w) = std::fs::read_to_string(crate::watcher::pid_file()).ok().and_then(|s| s.trim().parse::<u32>().ok()) {
                            let pid = Pid::from_u32(w);
                            let is_ours = sys.process(pid).is_some_and(|p| p.name().to_string_lossy().eq_ignore_ascii_case("framewise.exe"));
                            // Started by the app it's a child (already in the tree); started at sign-in
                            // it isn't. Either way it gets its own line.
                            if is_ours {
                                tree.retain(|(p, _)| *p != pid);
                                tree.push((pid, "framewise.exe (watcher)".into()));
                            }
                        }
                        last_tree = Instant::now();
                    }
                    sys.refresh_cpu_usage();
                    sys.refresh_memory();

                    let wall = prev_at.elapsed().as_secs_f64().max(0.001);
                    prev_at = Instant::now();
                    let mut groups: HashMap<&'static str, ProcessFootprint> = HashMap::new();
                    let mut next_cpu = HashMap::new();
                    for (pid, name) in &tree {
                        let Some(s) = procmeter::sample(pid.as_u32()) else { continue };
                        let cpu_pct = prev_cpu
                            .get(pid)
                            .map(|prev| (s.cpu_time_100ns.saturating_sub(*prev)) as f64 / 1e7 / wall / ncpu * 100.0)
                            .unwrap_or(0.0);
                        next_cpu.insert(*pid, s.cpu_time_100ns);
                        let (key, role) = if name.ends_with("(watcher)") {
                            ("watcher", "Watcher · game profiles")
                        } else if *pid == own {
                            ("core", "Interface · app core")
                        } else if name.to_ascii_lowercase().starts_with("presentmon") {
                            ("monitor", "Monitor · PresentMon")
                        } else if name.to_ascii_lowercase().starts_with("msedgewebview2") {
                            ("webview", "Interface · WebView2 renderer")
                        } else {
                            ("other", "Interface · other")
                        };
                        let g = groups.entry(key).or_insert_with(|| ProcessFootprint {
                            role: role.into(),
                            name: name.clone(),
                            count: 0,
                            ram_mb: 0.0,
                            cpu_pct: 0.0,
                        });
                        g.count += 1;
                        g.ram_mb += s.private_ws_bytes as f64 / 1024.0 / 1024.0;
                        g.cpu_pct += cpu_pct;
                    }
                    prev_cpu = next_cpu;

                    // While the monitor runs, its own budget is added to the interface's.
                    let monitoring = groups.contains_key("monitor");
                    let watching = groups.contains_key("watcher");
                    let budget_ram = UI_BUDGET_RAM_MB
                        + if monitoring { MONITOR_BUDGET_RAM_MB } else { 0.0 }
                        + if watching { crate::watcher::BUDGET_RAM_MB } else { 0.0 };
                    let budget_cpu = UI_BUDGET_CPU_PCT
                        + if monitoring { MONITOR_BUDGET_CPU_PCT } else { 0.0 }
                        + if watching { crate::watcher::BUDGET_CPU_PCT } else { 0.0 };
                    let mut processes: Vec<ProcessFootprint> = groups.into_values().collect();
                    processes.sort_by(|a, b| a.role.cmp(&b.role));
                    let ram_mb: f64 = processes.iter().map(|p| p.ram_mb).sum();
                    let cpu_pct: f64 = processes.iter().map(|p| p.cpu_pct).sum();
                    let point = HistoryPoint { t: now_secs(), ram_mb, cpu_pct };
                    minute_ram.push(ram_mb);
                    minute_cpu.push(cpu_pct);

                    if let Ok(mut sh) = shared.lock() {
                        sh.history.push_back(point);
                        while sh.history.len() > HISTORY_LEN {
                            sh.history.pop_front();
                        }
                        sh.live = LiveStats {
                            cpu_pct: sys.global_cpu_usage() as f64,
                            ram_used_gb: sys.used_memory() as f64 / 1024f64.powi(3),
                            ram_total_gb: sys.total_memory() as f64 / 1024f64.powi(3),
                        };
                        // CPU is judged on the last minute's average; single samples spike on clicks.
                        let recent: Vec<f64> = sh.history.iter().rev().take(30).map(|p| p.cpu_pct).collect();
                        let recent_cpu = recent.iter().sum::<f64>() / recent.len().max(1) as f64;
                        sh.report = Some(FootprintReport {
                            ram_mb,
                            cpu_pct,
                            processes,
                            history: sh.history.iter().copied().collect(),
                            budget_ram_mb: budget_ram,
                            budget_cpu_pct: budget_cpu,
                            over_budget: ram_mb > budget_ram || (recent.len() >= 15 && recent_cpu > budget_cpu),
                        });
                    }

                    if last_persist.elapsed() >= Duration::from_secs(60) && !minute_ram.is_empty() {
                        let avg = |v: &Vec<f64>| v.iter().sum::<f64>() / v.len() as f64;
                        persist(now_secs(), avg(&minute_ram), avg(&minute_cpu));
                        minute_ram.clear();
                        minute_cpu.clear();
                        last_persist = Instant::now();
                    }

                    std::thread::sleep(SAMPLE_EVERY);
                }
            })
            .expect("spawn footprint thread");
    }
}

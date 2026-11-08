//! Samples one process's private working set and CPU every 2 seconds (the same measure as
//! FrameWise's footprint meter) and checks it against a budget. Used for the watcher's phase 4
//! gate.
//!
//! footprint_watch <pid> <seconds> [budget_mb=15] [budget_cpu_pct=0.2]
//! Exits 1 if the peak RAM or the average CPU is over budget, 2 if the process ends early.

#[cfg(windows)]
fn main() {
    use framewise_checks::procmeter::sample;
    use std::time::{Duration, Instant};
    let a: Vec<String> = std::env::args().collect();
    let pid: u32 = a.get(1).and_then(|s| s.parse().ok()).expect("pid");
    let secs: u64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(600);
    let budget_mb: f64 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(15.0);
    let budget_cpu: f64 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(0.2);
    let ncpu = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64;
    let Some(first) = sample(pid) else {
        println!("FAIL process {pid} not found");
        std::process::exit(2);
    };
    let start = Instant::now();
    let (mut peak, mut sum, mut n) = (0.0f64, 0.0f64, 0u32);
    let mut last = first.cpu_time_100ns;
    let mut last_at = Instant::now();
    let mut peak_cpu_2s = 0.0f64;
    while start.elapsed() < Duration::from_secs(secs) {
        std::thread::sleep(Duration::from_secs(2));
        let Some(s) = sample(pid) else {
            println!("FAIL process {pid} ended after {:.0} s", start.elapsed().as_secs_f64());
            std::process::exit(2);
        };
        let mb = s.private_ws_bytes as f64 / 1048576.0;
        let cpu2 = (s.cpu_time_100ns - last) as f64 / 1e7 / last_at.elapsed().as_secs_f64() / ncpu * 100.0;
        last = s.cpu_time_100ns;
        last_at = Instant::now();
        peak = peak.max(mb);
        peak_cpu_2s = peak_cpu_2s.max(cpu2);
        sum += mb;
        n += 1;
    }
    let end = sample(pid).map(|s| s.cpu_time_100ns).unwrap_or(last);
    let avg_cpu = (end - first.cpu_time_100ns) as f64 / 1e7 / start.elapsed().as_secs_f64() / ncpu * 100.0;
    let ok = peak <= budget_mb && avg_cpu <= budget_cpu;
    println!(
        "{} RAM peak {:.2} MB, average {:.2} MB (budget {budget_mb} MB) · CPU average {:.4}% (budget {budget_cpu}%), busiest 2 s {:.3}% · {:.0} s, {n} samples",
        if ok { "PASS" } else { "FAIL" },
        peak,
        sum / n.max(1) as f64,
        avg_cpu,
        peak_cpu_2s,
        start.elapsed().as_secs_f64()
    );
    std::process::exit(if ok { 0 } else { 1 });
}

#[cfg(not(windows))]
fn main() {}

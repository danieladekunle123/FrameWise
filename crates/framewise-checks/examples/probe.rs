//! Read-only probe of the phase 2 readers: current values of every fixable setting, startup
//! entries and Cleanup sizes. Changes nothing.
//!
//! cargo run -p framewise-checks --example probe

#[cfg(windows)]
fn main() {
    use framewise_checks::cleanup::{scan, ALL_CATEGORIES};
    use framewise_checks::system::read;
    use framewise_checks::tweak::Tweak;

    for t in [Tweak::PowerScheme, Tweak::PowerMode, Tweak::GameMode, Tweak::WindowedOptimizations, Tweak::Hags, Tweak::MemoryIntegrity] {
        match read(&t) {
            Ok(v) => println!("{:<40} {:<28} {:?}", t.title(), t.describe(&v), v),
            Err(e) => println!("{:<40} error: {e}", t.title()),
        }
    }
    println!("memory integrity locked: {}", framewise_checks::system::memory_integrity_locked());
    for p in ["refresh_rate", "power_plan", "game_mode", "windowed_optimizations", "hags"] {
        println!("plan {p}: {:?}", framewise_checks::fixes::plan(p, &Default::default()));
    }
    println!("\nstartup entries:");
    for e in framewise_checks::collect::startup_entries() {
        let t = Tweak::StartupEntry { source: e.source.unwrap(), name: e.value_name.clone() };
        println!("  {:<5} {:<34} {:<32} {:?}", e.enabled, e.name, e.location, read(&t).map(|v| t.describe(&v)));
    }
    println!("\ncleanup:");
    for c in ALL_CATEGORIES {
        let t = std::time::Instant::now();
        let s = scan(*c);
        println!(
            "  {:<36} {:>9.1} MB  {:>6} items  {:>5} recent  {:>6} ms  {:?}",
            c.info().title,
            s.bytes as f64 / 1048576.0,
            s.items,
            s.recent_items,
            t.elapsed().as_millis(),
            s.error
        );
        for d in &s.drivers {
            println!("      {} {} {} {} {:.0} MB in_use={} rollback={}", d.inf, d.provider, d.version, d.date, d.bytes as f64 / 1048576.0, d.in_use, d.kept_for_rollback);
        }
    }
}

#[cfg(not(windows))]
fn main() {}

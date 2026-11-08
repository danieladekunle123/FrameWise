//! Run a scan from the command line.
//!
//! * `cargo run -p framewise-checks --example scan` scans this PC (read-only) and prints findings.
//! * `--snapshot` prints the raw snapshot JSON instead (to record a test fixture).
//! * `--from <file.json>` evaluates a recorded snapshot instead of this PC.
//! * `--json` prints a full `ScanResult` as JSON (used to generate the UI's mock data).

use framewise_checks::{evaluate_all, health_score, ScanResult, Snapshot};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let from = args.iter().position(|a| a == "--from").and_then(|i| args.get(i + 1));
    let start = std::time::Instant::now();

    let snap: Snapshot = match from {
        Some(path) => serde_json::from_str(&std::fs::read_to_string(path).expect("read snapshot")).expect("parse snapshot"),
        None => collect_here(),
    };
    let elapsed = start.elapsed();

    if args.iter().any(|a| a == "--snapshot") {
        println!("{}", serde_json::to_string_pretty(&snap).unwrap());
        return;
    }
    let findings = evaluate_all(&snap);
    if args.iter().any(|a| a == "--json") {
        let scan = ScanResult {
            id: 1,
            started_at: snap.collected_at,
            duration_ms: 640,
            score: health_score(&findings),
            app_version: env!("CARGO_PKG_VERSION").into(),
            findings,
            collection_errors: snap.collection_errors,
        };
        println!("{}", serde_json::to_string_pretty(&scan).unwrap());
        return;
    }
    for f in &findings {
        println!("[{:?}/{:?}] {} — {}", f.status, f.impact, f.check_id, f.summary);
        for d in &f.details {
            println!("      {d}");
        }
    }
    println!("\nScore: {}  ·  collected in {:.2?}", health_score(&findings), elapsed);
    for e in &snap.collection_errors {
        println!("collection error: {e}");
    }
}

#[cfg(windows)]
fn collect_here() -> Snapshot {
    framewise_checks::collect::collect()
}

#[cfg(not(windows))]
fn collect_here() -> Snapshot {
    eprintln!("Live scanning needs Windows; pass --from <snapshot.json>.");
    std::process::exit(1);
}

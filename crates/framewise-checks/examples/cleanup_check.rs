//! Cleanup check for the machine-wide categories (run elevated). Measures each category, and with
//! `--clean` removes what's eligible and measures again, checking that nothing newer than the
//! category's minimum age was touched. Driver packages are only listed, never removed here.
//!
//! cargo run -p framewise-checks --example cleanup_check            # measure only
//! cargo run -p framewise-checks --example cleanup_check -- --clean # measure, clean, re-measure
//!
//! Exits non-zero if a check fails.

#[cfg(windows)]
fn main() {
    use framewise_checks::cleanup::*;
    let clean_too = std::env::args().any(|a| a == "--clean");
    let mb = |b: u64| format!("{:.1} MB", b as f64 / 1_048_576.0);
    let mut failed = 0;
    for &cat in [CleanCategory::WindowsTemp, CleanCategory::SystemCrashDumps, CleanCategory::UpdateDownloads, CleanCategory::DriverPackages].iter() {
        let title = cat.info().title;
        let before = scan(cat);
        if before.needs_permission {
            failed += 1;
            println!("FAIL {title}: still needs permission to measure (run elevated)");
            continue;
        }
        if let Some(e) = &before.error {
            failed += 1;
            println!("FAIL {title}: {e}");
            continue;
        }
        println!("MEASURED {title}: {} in {} items, {} recent items kept", mb(before.bytes), before.items, before.recent_items);
        if cat == CleanCategory::DriverPackages {
            for d in &before.drivers {
                let what = if d.in_use { "kept: in use" } else if d.kept_for_rollback { "kept: Roll Back Driver" } else { "removable" };
                println!("    {} {} {} {} {} ({what})", d.inf, d.provider, d.version, d.date, mb(d.bytes));
            }
            continue;
        }
        if !clean_too {
            continue;
        }
        // Top-level entries that changed recently must survive the clean.
        let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(cat.min_age_secs());
        let recent: Vec<std::path::PathBuf> = roots(cat)
            .iter()
            .filter_map(|r| std::fs::read_dir(r).ok())
            .flat_map(|rd| rd.flatten())
            .filter(|e| e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t > cutoff))
            .map(|e| e.path())
            .collect();
        let res = clean(cat, false);
        let after = scan(cat);
        let lost: Vec<_> = recent.iter().filter(|p| !p.exists()).collect();
        if !lost.is_empty() {
            println!("    recent entries that disappeared (may have been removed by their own app): {lost:?}");
        }
        // Everything eligible is gone (or was in use and skipped), and recent entries are untouched.
        let ok = res.error.is_none() && after.items <= res.skipped_items && lost.is_empty();
        if ok {
            println!(
                "PASS {title}: freed {}, removed {}, skipped {} in use; {} eligible items left; all {} recent entries kept",
                mb(res.freed_bytes),
                res.removed_items,
                res.skipped_items,
                after.items,
                recent.len()
            );
        } else {
            failed += 1;
            println!("FAIL {title}: {res:?}; after: {} eligible items, {} recent", after.items, after.recent_items);
        }
    }
    if failed > 0 {
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {}

//! Restart test for the fixes that only take effect after a restart (GPU scheduling, Memory
//! Integrity). Run elevated, ideally in a disposable VM:
//!
//!   restart_check before   # saves the current values, writes new ones, verifies, shows the checks
//!   (restart Windows)
//!   restart_check after    # checks the new values survived and what the checks say, then puts
//!                          # the originals back and verifies them (a second restart applies them)
//!
//! State is kept in restart-state.json next to the exe. Exits non-zero if a check fails.

#[cfg(windows)]
fn main() {
    use framewise_checks::evaluate_all;
    use framewise_checks::system::{read, write};
    use framewise_checks::tweak::{Tweak, TweakValue};

    let phase = std::env::args().nth(1).unwrap_or_default();
    let state_path = std::env::current_exe().unwrap().with_file_name("restart-state.json");
    let tweaks = [Tweak::Hags, Tweak::MemoryIntegrity];
    let show_checks = || {
        let snap = framewise_checks::collect::collect();
        println!("  hvci_enabled={:?} hvci_running={:?} hags_mode={:?} hags_caps={:?}", snap.hvci_enabled, snap.hvci_running, snap.hags_mode, snap.hags_caps);
        for f in evaluate_all(&snap).into_iter().filter(|f| f.check_id == "hags" || f.check_id == "memory_integrity") {
            println!("  check {}: {:?} · {} · {}", f.check_id, f.status, f.detected.clone().unwrap_or_default(), f.summary);
        }
    };
    let mut failed = 0;
    match phase.as_str() {
        "before" => {
            let mut saved = Vec::new();
            for t in tweaks {
                let orig = read(&t).expect("read");
                let new = match (&t, &orig) {
                    (Tweak::Hags, TweakValue::Dword(2)) => TweakValue::Dword(1),
                    (Tweak::Hags, _) => TweakValue::Dword(2),
                    (Tweak::MemoryIntegrity, TweakValue::Dword(1)) => TweakValue::Dword(0),
                    _ => TweakValue::Dword(1),
                };
                let ok = write(&t, &new).is_ok() && read(&t).ok().as_ref() == Some(&new);
                println!("{} {}: {} → {}", if ok { "SET " } else { "FAIL" }, t.title(), t.describe(&orig), t.describe(&new));
                if !ok {
                    failed += 1;
                }
                saved.push((t, orig, new));
            }
            std::fs::write(&state_path, serde_json::to_string_pretty(&saved).unwrap()).expect("save state");
            println!("Before restart:");
            show_checks();
        }
        "after" => {
            let saved: Vec<(Tweak, TweakValue, TweakValue)> =
                serde_json::from_str(&std::fs::read_to_string(&state_path).expect("run `before` first")).expect("state");
            println!("After restart:");
            show_checks();
            for (t, orig, new) in saved {
                let now = read(&t).ok();
                let kept = now.as_ref() == Some(&new);
                println!("{} {} survived the restart: {}", if kept { "PASS" } else { "FAIL" }, t.title(), now.as_ref().map(|v| t.describe(v)).unwrap_or_default());
                let restored = write(&t, &orig).is_ok() && read(&t).ok().as_ref() == Some(&orig);
                println!("{} {} put back: {}", if restored { "PASS" } else { "FAIL" }, t.title(), t.describe(&orig));
                if !kept || !restored {
                    failed += 1;
                }
            }
            let _ = std::fs::remove_file(&state_path);
            println!("Originals written back (they take effect at the next restart).");
        }
        _ => {
            eprintln!("usage: restart_check before|after");
            std::process::exit(2);
        }
    }
    if failed > 0 {
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {}

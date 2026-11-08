//! Fix round-trip test from the design document: for each setting, apply a new value, verify it,
//! put the original back, and verify the original is back exactly. Every setting ends as it began.
//!
//! cargo run -p framewise-checks --example roundtrip                # per-user settings
//! cargo run -p framewise-checks --example roundtrip -- --display   # also refresh rate (screen flickers)
//! cargo run -p framewise-checks --example roundtrip -- --admin     # also machine-wide settings;
//!                                                                    run elevated, ideally in a VM
//!
//! Exits non-zero if any round trip fails.

#[cfg(windows)]
fn main() {
    use framewise_checks::system::{read, write};
    use framewise_checks::tweak::*;

    let args: Vec<String> = std::env::args().collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    let mut cases: Vec<(Tweak, TweakValue)> = Vec::new();

    // Per-user settings.
    cases.push((Tweak::GameMode, TweakValue::Dword(0)));
    cases.push((Tweak::GameMode, TweakValue::Dword(1)));
    let dx = match read(&Tweak::WindowedOptimizations) {
        Ok(TweakValue::Text(s)) => Some(s),
        _ => None,
    };
    cases.push((Tweak::WindowedOptimizations, TweakValue::Text(dx_set(dx.as_deref(), "SwapEffectUpgradeEnable", "1"))));
    let scheme = read(&Tweak::PowerScheme).ok();
    let other = if matches!(&scheme, Some(TweakValue::Text(s)) if s.eq_ignore_ascii_case(SCHEME_BALANCED)) { SCHEME_HIGH } else { SCHEME_BALANCED };
    cases.push((Tweak::PowerScheme, TweakValue::Text(other.into())));
    let mode = read(&Tweak::PowerMode).ok();
    let other_mode =
        if matches!(&mode, Some(TweakValue::Text(s)) if s.eq_ignore_ascii_case(MODE_BEST_PERFORMANCE)) { MODE_BALANCED } else { MODE_BEST_PERFORMANCE };
    cases.push((Tweak::PowerMode, TweakValue::Text(other_mode.into())));
    if let Some(e) = framewise_checks::collect::startup_entries().into_iter().find(|e| e.source == Some(StartupSource::UserRun)) {
        let t = Tweak::StartupEntry { source: StartupSource::UserRun, name: e.value_name };
        cases.push((t, TweakValue::Bytes(startup_disabled_bytes(framewise_checks::system::filetime_now()))));
    }
    if has("--display") {
        if let Ok(displays) = framewise_checks::collect::collect_displays() {
            for d in displays {
                // A lower supported rate and back.
                let rates = framewise_checks::system::refresh_rates(&d.device_name, d.width, d.height);
                if let Some(hz) = rates.into_iter().filter(|&h| h < d.current_hz && h >= 50).max() {
                    let t = Tweak::DisplayRefresh { device: d.device_name, monitor: d.monitor, width: d.width, height: d.height };
                    cases.push((t, TweakValue::Dword(hz)));
                }
            }
        }
    }
    if has("--admin") {
        let hags = read(&Tweak::Hags).ok();
        cases.push((Tweak::Hags, TweakValue::Dword(if hags == Some(TweakValue::Dword(2)) { 1 } else { 2 })));
        let mi = read(&Tweak::MemoryIntegrity).ok();
        cases.push((Tweak::MemoryIntegrity, TweakValue::Dword(if mi == Some(TweakValue::Dword(1)) { 0 } else { 1 })));
        if let Some(e) = framewise_checks::collect::startup_entries().into_iter().find(|e| e.source == Some(StartupSource::MachineRun)) {
            let t = Tweak::StartupEntry { source: StartupSource::MachineRun, name: e.value_name };
            cases.push((t, TweakValue::Bytes(startup_disabled_bytes(framewise_checks::system::filetime_now()))));
        }
    }

    let same = |t: &Tweak, a: &TweakValue, b: &TweakValue| match (t, a, b) {
        (Tweak::PowerScheme | Tweak::PowerMode, TweakValue::Text(x), TweakValue::Text(y)) => x.eq_ignore_ascii_case(y),
        _ => a == b,
    };
    let settle = |t: &Tweak, want: &TweakValue| {
        for _ in 0..10 {
            if read(t).is_ok_and(|v| same(t, &v, want)) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        false
    };

    let mut failed = 0;
    if has("--admin") {
        // Not a round trip (Windows owns restore points), but it only works elevated, so check it here.
        match framewise_checks::system::create_restore_point("FrameWise round-trip test") {
            Ok(rp) => println!(
                "PASS Restore point: sequence {} \"{}\"{}",
                rp.sequence,
                rp.description,
                if rp.reused { " (reused: Windows already made one in the last 24 hours)" } else { " (new)" }
            ),
            Err((code, msg)) if code == "restore_point_off" => println!("SKIP Restore point: System Restore is off ({msg})"),
            Err((code, msg)) => {
                failed += 1;
                println!("FAIL Restore point: {code}: {msg}");
            }
        }
        println!("Memory Integrity locked by UEFI: {}", framewise_checks::system::memory_integrity_locked());
    }
    for (t, new) in cases {
        let name = t.title();
        let orig = match read(&t) {
            Ok(v) => v,
            Err(e) => {
                println!("SKIP {name}: can't read ({e})");
                continue;
            }
        };
        let applied = write(&t, &new).map_err(|e| e.to_string()).and_then(|_| if settle(&t, &new) { Ok(()) } else { Err("didn't verify".into()) });
        // Always put the original back, even if applying failed half-way.
        let restored = write(&t, &orig).map_err(|e| e.to_string()).and_then(|_| if settle(&t, &orig) { Ok(()) } else { Err("didn't verify".into()) });
        let exact = read(&t).map(|v| v == orig || same(&t, &v, &orig)).unwrap_or(false);
        match (applied, restored, exact) {
            (Ok(()), Ok(()), true) => println!("PASS {name}: {} → {} → {}", t.describe(&orig), t.describe(&new), t.describe(&orig)),
            (a, r, e) => {
                failed += 1;
                println!("FAIL {name}: apply {a:?}, undo {r:?}, original back exactly: {e} (original {orig:?})");
            }
        }
    }
    if failed > 0 {
        std::process::exit(1);
    }
}

#[cfg(not(windows))]
fn main() {}

// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `framewise.exe --helper ...` is the elevated helper (see src/elevate.rs): no window, no
    // WebView, just the allowlisted request loop.
    let args: Vec<String> = std::env::args().collect();
    // Used by the installer before updating or uninstalling: stop the watcher so its exe isn't locked.
    if args.get(1).map(String::as_str) == Some("--stop-watcher") {
        std::process::exit(framewise_lib::stop_watcher());
    }
    // Used on uninstall: stop the watcher and remove its start-with-Windows entry.
    if args.get(1).map(String::as_str) == Some("--uninstall-cleanup") {
        std::process::exit(framewise_lib::uninstall_cleanup());
    }
    if args.get(1).map(String::as_str) == Some("--watcher") {
        std::process::exit(framewise_lib::watcher_main());
    }
    if args.get(1).map(String::as_str) == Some("--helper") {
        std::process::exit(framewise_lib::helper_main(&args));
    }
    // Debug builds only: exercise the pipe and helper protocol without elevation.
    #[cfg(debug_assertions)]
    if args.get(1).map(String::as_str) == Some("--selftest-helper") {
        std::process::exit(framewise_lib::helper_selftest());
    }
    #[cfg(debug_assertions)]
    if args.get(1).map(String::as_str) == Some("--capture-test") {
        let secs = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(10);
        std::process::exit(framewise_lib::capture_selftest(secs));
    }
    framewise_lib::run()
}

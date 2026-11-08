//! Lists the games FrameWise detects for profiles. Read-only.
#[cfg(windows)]
fn main() {
    for g in framewise_checks::library::detect_games() {
        println!("{:<40} {:<12} {:<28} {}", g.name, g.source.label(), g.id, g.exe.as_deref().or(g.install_dir.as_deref()).unwrap_or(""));
    }
}
#[cfg(not(windows))]
fn main() {}
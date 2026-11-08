//! Prints live sensor readings once a second for a few seconds. Read-only.
#[cfg(windows)]
fn main() {
    let mut s = framewise_checks::sensors::Sensors::new();
    println!("NVML: {}", s.has_nvml());
    let n: u32 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(3);
    for _ in 0..n {
        std::thread::sleep(std::time::Duration::from_secs(1));
        println!("{:?}", s.sample());
    }
}
#[cfg(not(windows))]
fn main() {}
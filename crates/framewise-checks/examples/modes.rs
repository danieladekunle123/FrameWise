//! Diagnostic: every display's current mode and the modes Windows lists for it. Read-only.
#[cfg(windows)]
fn main() {
    use windows::core::HSTRING;
    use windows::Win32::Graphics::Gdi::{
        EnumDisplaySettingsExW, EnumDisplaySettingsW, DEVMODEW, EDS_RAWMODE, ENUM_CURRENT_SETTINGS, ENUM_DISPLAY_SETTINGS_FLAGS,
        ENUM_DISPLAY_SETTINGS_MODE, ENUM_REGISTRY_SETTINGS,
    };
    for d in framewise_checks::collect::collect_displays().unwrap_or_default() {
        println!("{d:?}");
        let name = HSTRING::from(d.device_name.as_str());
        for (label, which) in [("current", ENUM_CURRENT_SETTINGS), ("registry", ENUM_REGISTRY_SETTINGS)] {
            let mut cur = DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() };
            let ok = unsafe { EnumDisplaySettingsW(&name, which, &mut cur) }.as_bool();
            println!("  {label} ok={ok} {}x{} {}bpp {}Hz fields {:#x}", cur.dmPelsWidth, cur.dmPelsHeight, cur.dmBitsPerPel, cur.dmDisplayFrequency, cur.dmFields.0);
        }
        for (label, flags) in [("normal", ENUM_DISPLAY_SETTINGS_FLAGS(0)), ("raw", EDS_RAWMODE)] {
            let mut rates = Vec::new();
            let mut n = 0;
            for m in 0..5000u32 {
                let mut dm = DEVMODEW { dmSize: std::mem::size_of::<DEVMODEW>() as u16, ..Default::default() };
                if !unsafe { EnumDisplaySettingsExW(&name, ENUM_DISPLAY_SETTINGS_MODE(m), &mut dm, flags) }.as_bool() {
                    break;
                }
                n += 1;
                if dm.dmPelsWidth == d.width && dm.dmPelsHeight == d.height && dm.dmBitsPerPel == 32 {
                    rates.push(dm.dmDisplayFrequency);
                }
            }
            rates.sort_unstable();
            rates.dedup();
            println!("  {label}: {n} modes; at {}x{} 32bpp: {rates:?}", d.width, d.height);
        }
    }
}
#[cfg(not(windows))]
fn main() {}

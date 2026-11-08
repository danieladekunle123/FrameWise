//! Keeps the interface light while you're not looking at it.
//!
//! When the FrameWise window loses focus (minimised, or behind a game), WebView2 is asked to use
//! its Low memory target, which trims caches and working set it doesn't need. When the window is
//! focused again it goes back to Normal. This is Microsoft's documented use of
//! `ICoreWebView2_19::MemoryUsageTargetLevel` for inactive apps, and it only affects FrameWise's own
//! processes, never the rest of the system (that would be a RAM cleaner, which FrameWise doesn't do).

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{WebviewWindow, WindowEvent};

/// True while the Low target is applied.
pub static BACKGROUND_TRIM: AtomicBool = AtomicBool::new(false);

fn set_low(window: &WebviewWindow, low: bool) {
    if BACKGROUND_TRIM.swap(low, Ordering::SeqCst) == low {
        return;
    }
    let _ = window.with_webview(move |wv| {
        use webview2_com::Microsoft::Web::WebView2::Win32::{
            ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        };
        use windows_core::Interface;
        let level = if low { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW } else { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL };
        // SAFETY: COM calls on the controller Tauri owns, made on the webview's thread by with_webview.
        // Older WebView2 runtimes without ICoreWebView2_19 simply skip this.
        unsafe {
            if let Ok(core) = wv.controller().CoreWebView2() {
                if let Ok(core19) = core.cast::<ICoreWebView2_19>() {
                    let _ = core19.SetMemoryUsageTargetLevel(level);
                }
            }
        }
    });
}

/// Applies Low when the window loses focus and Normal when it regains it.
///
/// For footprint tests, `FRAMEWISE_MEMORY_TARGET=low|normal` pins the level instead
/// (see scripts/measure-footprint.ps1), since automation can't reliably move window focus.
pub fn watch(window: &WebviewWindow) {
    if let Ok(pinned) = std::env::var("FRAMEWISE_MEMORY_TARGET") {
        // Start from the opposite state so the first call always applies.
        let low = pinned.eq_ignore_ascii_case("low");
        BACKGROUND_TRIM.store(!low, Ordering::SeqCst);
        set_low(window, low);
        return;
    }
    let w = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::Focused(focused) = event {
            set_low(&w, !focused);
        }
    });
    // A window launched in the background never receives a focus event, so start from its real state.
    if !window.is_focused().unwrap_or(true) {
        set_low(window, true);
    }
}

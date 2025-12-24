//! X11 thread initialization for Linux systems.
//!
//! This module handles calling XInitThreads() before any X11 operations
//! to prevent crashes with the error:
//! "[xcb] Most likely this is a multi-threaded client and XInitThreads has not
//! been called"

/// Initializes X11 for multi-threaded use on Linux X11 systems.
///
/// This function must be called at the very start of main(), before any
/// other code that might interact with X11 (including Tauri/wry
/// initialization).
///
/// On Wayland or non-Linux systems, this function does nothing.
/// If libX11 is not available, this function continues gracefully.
#[cfg(target_os = "linux")]
pub fn init_x11_threads() {
    // Skip if running on Wayland
    if is_wayland() {
        return;
    }

    // Use dlopen to dynamically load libX11
    // This avoids requiring libX11-dev at compile time and handles
    // systems where X11 is not installed
    unsafe {
        let lib = libc::dlopen(c"libX11.so.6".as_ptr(), libc::RTLD_LAZY);

        let lib = if lib.is_null() {
            // Try without version suffix
            let lib_fallback = libc::dlopen(c"libX11.so".as_ptr(), libc::RTLD_LAZY);
            if lib_fallback.is_null() {
                return;
            }
            lib_fallback
        } else {
            lib
        };

        let sym = libc::dlsym(lib, c"XInitThreads".as_ptr());
        if !sym.is_null() {
            let init_fn: unsafe extern "C" fn() -> libc::c_int = std::mem::transmute(sym);
            init_fn();
        }
        // Note: We intentionally do NOT call dlclose here.
        // libX11 needs to remain loaded for the entire application lifetime.
    }
}

#[cfg(target_os = "linux")]
fn is_wayland() -> bool {
    std::env::var("WAYLAND_DISPLAY").is_ok()
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland")
}

/// No-op on non-Linux platforms.
#[cfg(not(target_os = "linux"))]
pub fn init_x11_threads() {
    // Nothing to do on non-Linux platforms
}

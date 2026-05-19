// Tauri command handlers are registered at runtime via the
// `tauri::generate_handler!` macro in `main.rs`. The compiler cannot see
// those references, so every command function gets a false-positive
// `dead_code` warning. The same applies to types used only inside command
// payloads (deserialized from the JS side) and to plugin-installed state.
//
// The `kftray-tauri` library exists only to back the `cdylib`/`staticlib`
// targets used by Tauri's mobile build pipeline; it is never consumed as a
// regular Rust dependency, so a blanket allow here is the precise fix.
#![allow(dead_code)]

pub mod commands;
pub mod glibc_detector;
pub mod init_check;
pub mod mcp;
pub mod shortcuts;
pub mod tray;
#[cfg(target_os = "linux")]
pub mod tray_linux;
#[cfg(target_os = "windows")]
pub mod tray_theme;
pub mod validation;
pub mod window;
pub mod window_size;

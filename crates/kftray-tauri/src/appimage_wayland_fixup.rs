//! AppImage + Wayland compatibility fixup.
//!
//! When running as an AppImage on a Wayland session, the bundled
//! `libwayland-client.so` may be older than the host compositor's protocol
//! version. This causes WebKitGTK's EGL initialization to fail silently,
//! producing a blank window.
//!
//! The fix: detect this situation at startup and re-exec with LD_PRELOAD
//! pointing to the host's libwayland-client, so the dynamic linker prefers
//! the host copy over the bundled one.

use std::env;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Command;

/// Guard variable to prevent infinite re-exec loops.
const REEXEC_GUARD: &str = "_KFTRAY_WAYLAND_REEXEC";

/// Candidate paths for the host's libwayland-client, ordered by distro
/// convention:
/// - Debian/Ubuntu: /usr/lib/x86_64-linux-gnu/ (or aarch64-linux-gnu)
/// - Fedora/RHEL: /usr/lib64/
/// - Arch: /usr/lib/
const CANDIDATE_PATHS: &[&str] = &[
    #[cfg(target_arch = "x86_64")]
    "/usr/lib/x86_64-linux-gnu/libwayland-client.so.0",
    #[cfg(target_arch = "aarch64")]
    "/usr/lib/aarch64-linux-gnu/libwayland-client.so.0",
    "/usr/lib64/libwayland-client.so.0",
    "/usr/lib/libwayland-client.so.0",
];

/// Re-execs the current process with LD_PRELOAD if all conditions are met:
/// 1. Running inside an AppImage (APPIMAGE env var set)
/// 2. Session is Wayland
/// 3. Haven't already re-exec'd (guard var absent)
/// 4. A suitable host libwayland-client exists
///
/// If any condition fails, returns normally and startup continues as usual.
pub fn maybe_reexec() {
    if env::var_os(REEXEC_GUARD).is_some() {
        return;
    }

    if env::var_os("APPIMAGE").is_none() {
        return;
    }

    if !is_wayland_session() {
        return;
    }

    let Some(host_lib) = find_host_libwayland() else {
        return;
    };

    // Build LD_PRELOAD value, preserving any existing entries.
    let preload = match env::var_os("LD_PRELOAD") {
        Some(existing) => {
            let mut val = host_lib.to_string();
            let existing_str = existing.to_string_lossy();
            if !existing_str.is_empty() {
                val.push(':');
                val.push_str(&existing_str);
            }
            val
        }
        None => host_lib.to_string(),
    };

    let exe = match env::current_exe() {
        Ok(p) => p,
        Err(_) => return,
    };

    let args: Vec<String> = env::args().skip(1).collect();

    // exec() replaces the current process — no parent left behind.
    let err = Command::new(exe)
        .args(&args)
        .env(REEXEC_GUARD, "1")
        .env("LD_PRELOAD", &preload)
        .exec();

    // exec() only returns on failure. Log and continue normally.
    eprintln!("kftray: wayland fixup re-exec failed: {err}");
}

fn is_wayland_session() -> bool {
    env::var_os("WAYLAND_DISPLAY").is_some()
        || env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland")
}

fn find_host_libwayland() -> Option<&'static str> {
    CANDIDATE_PATHS
        .iter()
        .find(|path| is_valid_elf(path))
        .copied()
}

/// Verify the file exists and is a valid ELF of the correct architecture.
/// This guards against picking up a 32-bit library on a 64-bit system.
fn is_valid_elf(path: &str) -> bool {
    let p = Path::new(path);
    if !p.is_file() {
        return false;
    }

    // Read the ELF header (first 20 bytes are enough for magic + class + arch).
    let Ok(data) = std::fs::read(p) else {
        return false;
    };

    if data.len() < 20 {
        return false;
    }

    // ELF magic: 0x7f 'E' 'L' 'F'
    if &data[0..4] != b"\x7fELF" {
        return false;
    }

    // EI_CLASS: 1 = 32-bit, 2 = 64-bit
    let elf_class = data[4];
    let expected_class: u8 = if cfg!(target_pointer_width = "64") {
        2
    } else {
        1
    };

    elf_class == expected_class
}

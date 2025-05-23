use std::path::{
    Path,
    PathBuf,
};
use std::process::Command;

use crate::error::HelperError;

pub fn find_helper_binary() -> Result<PathBuf, HelperError> {
    let helper_name = if cfg!(target_os = "windows") {
        "kftray-helper.exe"
    } else {
        "kftray-helper"
    };

    let current_exe = std::env::current_exe().map_err(|e| {
        HelperError::PlatformService(format!("Failed to get current executable path: {e}"))
    })?;

    let exe_dir = current_exe.parent().ok_or_else(|| {
        HelperError::PlatformService("Failed to get current executable directory".into())
    })?;

    let mut target_triple = String::new();
    if let Ok(output) = Command::new("rustc").args(["-Vv"]).output() {
        let output_str = String::from_utf8_lossy(&output.stdout);
        if let Some(line) = output_str.lines().find(|line| line.starts_with("host:")) {
            if let Some(triple) = line.split_whitespace().nth(1) {
                target_triple = triple.to_string();
            }
        }
    }

    #[cfg(debug_assertions)]
    {
        let workspace_root = find_workspace_root(&std::env::current_dir().unwrap_or_default())?;

        let tauri_bin_dir = workspace_root
            .join("crates")
            .join("kftray-tauri")
            .join("bin");

        if !target_triple.is_empty() {
            let sidecar_name = format!(
                "kftray-helper-{}{}",
                target_triple,
                if cfg!(target_os = "windows") {
                    ".exe"
                } else {
                    ""
                }
            );

            let tauri_sidecar_path = tauri_bin_dir.join(&sidecar_name);
            if tauri_sidecar_path.exists() {
                return Ok(tauri_sidecar_path);
            }
        }

        let tauri_helper_path = tauri_bin_dir.join(helper_name);
        if tauri_helper_path.exists() {
            return Ok(tauri_helper_path);
        }
    }

    if !target_triple.is_empty() {
        let sidecar_name = format!(
            "kftray-helper-{}{}",
            target_triple,
            if cfg!(target_os = "windows") {
                ".exe"
            } else {
                ""
            }
        );

        let resources_dir = exe_dir.join("resources");
        if resources_dir.exists() {
            let sidecar_path = resources_dir.join(&sidecar_name);
            if sidecar_path.exists() {
                return Ok(sidecar_path);
            }

            let resources_bin_dir = resources_dir.join("bin");
            if resources_bin_dir.exists() {
                let resources_bin_path = resources_bin_dir.join(&sidecar_name);
                if resources_bin_path.exists() {
                    return Ok(resources_bin_path);
                }
            }
        }

        let bin_dir = exe_dir.join("bin");
        if bin_dir.exists() {
            let sidecar_path = bin_dir.join(&sidecar_name);
            if sidecar_path.exists() {
                return Ok(sidecar_path);
            }
        }
    }

    let helper_path = exe_dir.join(helper_name);
    if helper_path.exists() {
        return Ok(helper_path);
    }

    let current_dir = std::env::current_dir().map_err(|e| {
        HelperError::PlatformService(format!("Failed to get current directory: {e}"))
    })?;

    let workspace_root = find_workspace_root(&current_dir)?;

    let debug_path = workspace_root
        .join("target")
        .join("debug")
        .join(helper_name);
    if debug_path.exists() {
        return Ok(debug_path);
    }

    let release_path = workspace_root
        .join("target")
        .join("release")
        .join(helper_name);
    if release_path.exists() {
        return Ok(release_path);
    }

    Err(HelperError::PlatformService(
        "Helper binary not found. Checked standard locations and sidecar locations.".to_string(),
    ))
}

pub fn find_workspace_root(start_dir: &Path) -> Result<PathBuf, HelperError> {
    let mut current = start_dir.to_path_buf();

    loop {
        let cargo_toml = current.join("Cargo.toml");
        if cargo_toml.exists() {
            return Ok(current);
        }

        if !current.pop() {
            return Err(HelperError::PlatformService(
                "Could not find workspace root directory".into(),
            ));
        }
    }
}

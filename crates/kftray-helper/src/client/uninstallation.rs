use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use crate::error::HelperError;

pub fn uninstall_helper(helper_path: &PathBuf) -> Result<(), HelperError> {
    #[cfg(target_os = "macos")]
    {
        let output = Command::new(helper_path).args(["uninstall"]).output();

        if let Err(_e) = output {
            // Escape single quotes in the path
            let escaped_path = helper_path.to_string_lossy().replace("'", "'\\''");
            let script = format!(
                r#"do shell script "'{0}' uninstall" with administrator privileges"#,
                escaped_path
            );

            let output = Command::new("osascript")
                .args(["-e", &script])
                .output()
                .map_err(|e| {
                    HelperError::PlatformService(format!(
                        "Failed to uninstall helper with admin privileges: {}",
                        e
                    ))
                })?;

            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                return Err(HelperError::PlatformService(format!(
                    "Failed to uninstall helper with admin privileges: {}",
                    error
                )));
            }
        } else if let Ok(output) = output {
            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                return Err(HelperError::PlatformService(format!(
                    "Failed to uninstall helper: {}",
                    error
                )));
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let output = Command::new("pkexec")
            .args([helper_path.to_string_lossy().as_ref(), "uninstall"])
            .output();

        if let Err(_) = output {
            let output = Command::new("sudo")
                .args([helper_path.to_string_lossy().as_ref(), "uninstall"])
                .output()
                .map_err(|e| {
                    HelperError::PlatformService(format!(
                        "Failed to uninstall helper with sudo: {}",
                        e
                    ))
                })?;

            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                return Err(HelperError::PlatformService(format!(
                    "Failed to uninstall helper with sudo: {}",
                    error
                )));
            }
        } else if let Ok(output) = output {
            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                return Err(HelperError::PlatformService(format!(
                    "Failed to uninstall helper with pkexec: {}",
                    error
                )));
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        let output = Command::new("powershell")
            .args([
                "-Command",
                &format!(
                    "Start-Process -FilePath \"{}\" -ArgumentList \"uninstall\" -Verb RunAs -Wait",
                    helper_path.to_string_lossy()
                ),
            ])
            .output()
            .map_err(|e| {
                HelperError::PlatformService(format!(
                    "Failed to uninstall helper with elevation: {}",
                    e
                ))
            })?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(HelperError::PlatformService(format!(
                "Failed to uninstall helper with elevation: {}",
                error
            )));
        }
    }

    std::thread::sleep(Duration::from_secs(2));
    Ok(())
}

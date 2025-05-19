use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use crate::error::HelperError;

pub fn install_helper(helper_path: &PathBuf) -> Result<(), HelperError> {
    #[cfg(target_os = "macos")]
    {
        let output = Command::new(helper_path).args(["install"]).output();

        if let Err(_e) = output {
            let script = format!(
                r#"do shell script "{} install" with administrator privileges"#,
                helper_path.to_string_lossy()
            );

            let output = Command::new("osascript")
                .args(["-e", &script])
                .output()
                .map_err(|e| {
                    HelperError::PlatformService(format!(
                        "Failed to install helper with admin privileges: {}",
                        e
                    ))
                })?;

            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                return Err(HelperError::PlatformService(format!(
                    "Failed to install helper with admin privileges: {}",
                    error
                )));
            }
        } else if let Ok(output) = output {
            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                return Err(HelperError::PlatformService(format!(
                    "Failed to install helper: {}",
                    error
                )));
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        let output = Command::new("pkexec")
            .args([helper_path.to_string_lossy().as_ref(), "install"])
            .output();

        if let Err(_) = output {
            let output = Command::new("sudo")
                .args([helper_path.to_string_lossy().as_ref(), "install"])
                .output()
                .map_err(|e| {
                    HelperError::PlatformService(format!(
                        "Failed to install helper with sudo: {}",
                        e
                    ))
                })?;

            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                return Err(HelperError::PlatformService(format!(
                    "Failed to install helper with sudo: {}",
                    error
                )));
            }
        } else if let Ok(output) = output {
            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                return Err(HelperError::PlatformService(format!(
                    "Failed to install helper with pkexec: {}",
                    error
                )));
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        let args = format!("\"{}\" install", helper_path.to_string_lossy());

        let output = Command::new("powershell")
            .args([
                "-Command",
                &format!(
                    "Start-Process -FilePath \"{}\" -ArgumentList \"install\" -Verb RunAs -Wait",
                    helper_path.to_string_lossy()
                ),
            ])
            .output()
            .map_err(|e| {
                HelperError::PlatformService(format!(
                    "Failed to install helper with elevation: {}",
                    e
                ))
            })?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(HelperError::PlatformService(format!(
                "Failed to install helper with elevation: {}",
                error
            )));
        }
    }

    std::thread::sleep(Duration::from_secs(2));
    Ok(())
}

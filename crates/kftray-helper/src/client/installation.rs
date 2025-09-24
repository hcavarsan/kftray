use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::error::HelperError;

pub fn install_helper(helper_path: &Path) -> Result<(), HelperError> {
    #[cfg(target_os = "macos")]
    {
        let output = Command::new(helper_path).args(["install"]).output();

        if let Err(_e) = output {
            let escaped_path = helper_path.to_string_lossy().replace("\"", "\\\"");
            let script = format!(
                r#"do shell script "{escaped_path} install" with administrator privileges"#
            );

            let output = Command::new("osascript")
                .args(["-e", &script])
                .output()
                .map_err(|e| {
                    HelperError::PlatformService(format!(
                        "Failed to install helper with admin privileges: {e}"
                    ))
                })?;

            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                return Err(HelperError::PlatformService(format!(
                    "Failed to install helper with admin privileges: {error}"
                )));
            }
        } else if let Ok(output) = output
            && !output.status.success()
        {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(HelperError::PlatformService(format!(
                "Failed to install helper: {error}"
            )));
        }
    }

    #[cfg(target_os = "linux")]
    {
        let helper_path_str = helper_path.to_string_lossy();
        let final_helper_path = if helper_path_str.contains("/tmp/.mount_") {
            let temp_dir = std::env::temp_dir();
            let extracted_helper = temp_dir.join("kftray-helper-install");

            std::fs::copy(helper_path, &extracted_helper).map_err(|e| {
                HelperError::PlatformService(format!("Failed to copy helper from AppImage: {}", e))
            })?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&extracted_helper)
                    .map_err(|e| {
                        HelperError::PlatformService(format!(
                            "Failed to get helper permissions: {}",
                            e
                        ))
                    })?
                    .permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&extracted_helper, perms).map_err(|e| {
                    HelperError::PlatformService(format!("Failed to set helper permissions: {}", e))
                })?;
            }

            extracted_helper
        } else {
            helper_path.to_path_buf()
        };

        if std::env::var("RUST_LOG").is_ok() {
            println!("Installing helper via pkexec...");
        }
        let output = Command::new("/usr/bin/pkexec")
            .args([final_helper_path.to_string_lossy().as_ref(), "install"])
            .output();

        if output.is_err() {
            if std::env::var("RUST_LOG").is_ok() {
                println!("pkexec failed, trying sudo...");
            }
            let output = Command::new("sudo")
                .args([final_helper_path.to_string_lossy().as_ref(), "install"])
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
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            if std::env::var("RUST_LOG").is_ok() {
                if !stdout.is_empty() {
                    println!("Helper output:\n{}", stdout);
                }
            }

            if !output.status.success() {
                if std::env::var("RUST_LOG").is_ok() && !stderr.is_empty() {
                    println!("Helper error:\n{}", stderr);
                }
                return Err(HelperError::PlatformService(format!(
                    "Failed to install helper with pkexec: {}",
                    stderr
                )));
            }
        }

        if helper_path_str.contains("/tmp/.mount_") {
            let _ = std::fs::remove_file(std::env::temp_dir().join("kftray-helper-install"));
        }
    }

    #[cfg(target_os = "windows")]
    {
        let _args = format!("\"{}\" install", helper_path.to_string_lossy());

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

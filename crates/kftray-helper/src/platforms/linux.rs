use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::Command,
};

use kftray_commons::utils::config_dir;
use log::warn;

use crate::error::HelperError;

macro_rules! debug_println {
    ($($arg:tt)*) => {
        if std::env::var("RUST_LOG").is_ok() {
            println!($($arg)*);
        }
    };
}

const SYSTEMD_SERVICE_TEMPLATE: &str = r#"[Unit]
Description=KFTray privileged helper service
After=network.target

[Service]
Type=simple
ExecStart={{HELPER_PATH}} service
Restart=on-failure
RestartSec=5
StandardOutput=file:{{LOG_DIR}}/kftray-helper.log
StandardError=file:{{LOG_DIR}}/kftray-helper.err
# Environment variables for consistent socket path location
Environment="CONFIG_DIR={{LOG_DIR}}"
Environment="KFTRAY_CONFIG={{LOG_DIR}}"
Environment="SOCKET_PATH={{LOG_DIR}}/{{SOCKET_FILENAME}}"
Environment="USER={{CURRENT_USER}}"
Environment="HOME={{HOME_DIR}}"

[Install]
WantedBy=default.target
"#;

const POLKIT_POLICY_TEMPLATE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE policyconfig PUBLIC
 "-//freedesktop//DTD PolicyKit Policy Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/PolicyKit/1/policyconfig.dtd">
<policyconfig>
  <action id="com.kftray.helper.network">
    <description>KFTray network configuration</description>
    <message>Authentication is required to configure network interfaces</message>
    <defaults>
      <allow_any>auth_admin</allow_any>
      <allow_inactive>auth_admin</allow_inactive>
      <allow_active>auth_admin_keep</allow_active>
    </defaults>
    <annotate key="org.freedesktop.policykit.exec.path">{{HELPER_PATH}}</annotate>
  </action>
</policyconfig>
"#;

pub fn install_service(service_name: &str) -> Result<(), HelperError> {
    let helper_path = std::env::current_exe().map_err(|e| {
        HelperError::PlatformService(format!("Failed to get current executable path: {}", e))
    })?;
    let helper_path_str = helper_path.to_string_lossy();

    let config_dir_path = match config_dir::get_config_dir() {
        Ok(path) => path,
        Err(_) => PathBuf::from("/tmp"),
    };

    if !config_dir_path.exists() {
        debug_println!("Creating config directory: {}", config_dir_path.display());
        fs::create_dir_all(&config_dir_path).map_err(|e| {
            HelperError::PlatformService(format!("Failed to create config directory: {}", e))
        })?;
    }

    let current_user = std::env::var("USER").unwrap_or_else(|_| "nobody".to_string());
    let home_dir = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());

    let socket_filename = crate::communication::SOCKET_FILENAME;

    let service_content = SYSTEMD_SERVICE_TEMPLATE
        .replace("{{HELPER_PATH}}", &helper_path_str)
        .replace("{{LOG_DIR}}", &config_dir_path.to_string_lossy())
        .replace("{{SOCKET_FILENAME}}", socket_filename)
        .replace("{{CURRENT_USER}}", &current_user)
        .replace("{{HOME_DIR}}", &home_dir);

    let policy_content = POLKIT_POLICY_TEMPLATE.replace("{{HELPER_PATH}}", &helper_path_str);

    let service_path = get_systemd_service_path(service_name)?;
    if let Some(parent) = service_path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            HelperError::PlatformService(format!("Failed to create systemd directory: {}", e))
        })?;
    }

    let mut file = fs::File::create(&service_path).map_err(|e| {
        HelperError::PlatformService(format!("Failed to create systemd service file: {}", e))
    })?;
    file.write_all(service_content.as_bytes()).map_err(|e| {
        HelperError::PlatformService(format!("Failed to write systemd service file: {}", e))
    })?;

    let policy_path = get_polkit_policy_path(service_name)?;
    if let Some(parent) = policy_path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            HelperError::PlatformService(format!("Failed to create polkit directory: {}", e))
        })?;
    }

    let mut file = fs::File::create(&policy_path).map_err(|e| {
        HelperError::PlatformService(format!("Failed to create polkit policy file: {}", e))
    })?;
    file.write_all(policy_content.as_bytes()).map_err(|e| {
        HelperError::PlatformService(format!("Failed to write polkit policy file: {}", e))
    })?;

    let systemctl_check = Command::new("systemctl").args(["--version"]).output();

    if systemctl_check.is_ok() && systemctl_check.unwrap().status.success() {
        let dbus_check = std::env::var("DBUS_SESSION_BUS_ADDRESS");
        let xdg_runtime = std::env::var("XDG_RUNTIME_DIR");

        if dbus_check.is_err() || xdg_runtime.is_err() {
            debug_println!("Warning: DBus session not available in pkexec environment");
            debug_println!(
                "The systemd service has been installed but needs to be started manually"
            );
            debug_println!("");
            debug_println!("Please run the following commands as your regular user:");
            debug_println!("  systemctl --user daemon-reload");
            debug_println!("  systemctl --user enable --now {}.service", service_name);
            debug_println!("");
            debug_println!("Alternatively, starting helper in standalone mode...");

            let log_path = config_dir_path.join(format!("{}-standalone.log", service_name));
            let log_file = std::fs::File::create(&log_path).map_err(|e| {
                HelperError::PlatformService(format!("Failed to create log file: {}", e))
            })?;

            let mut child = Command::new(&helper_path)
                .arg("service")
                .stdout(std::process::Stdio::from(log_file.try_clone().unwrap()))
                .stderr(std::process::Stdio::from(log_file))
                .spawn()
                .map_err(|e| {
                    HelperError::PlatformService(format!("Failed to start helper process: {}", e))
                })?;

            debug_println!("Helper started in standalone mode");
            debug_println!("Process ID: {}", child.id());
            debug_println!("Log file: {}", log_path.display());

            std::thread::sleep(std::time::Duration::from_millis(500));

            match child.try_wait() {
                Ok(Some(status)) => {
                    return Err(HelperError::PlatformService(format!(
                        "Helper process exited immediately with status: {}. Check log file at: {}",
                        status,
                        log_path.display()
                    )));
                }
                Ok(None) => {
                    debug_println!("Helper process is running");
                }
                Err(e) => {
                    warn!("Failed to check helper process status: {}", e);
                }
            }
        } else {
            let output = Command::new("systemctl")
                .args(["--user", "daemon-reload"])
                .output()
                .map_err(|e| {
                    HelperError::PlatformService(format!("Failed to reload systemd daemon: {}", e))
                })?;

            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                if error.contains("Failed to connect to bus") || error.contains("No medium found") {
                    debug_println!("Warning: Cannot connect to systemd user session from pkexec");
                    debug_println!(
                        "The systemd service has been installed but needs to be started manually"
                    );
                    debug_println!("");
                    debug_println!("Please run the following commands as your regular user:");
                    debug_println!("  systemctl --user daemon-reload");
                    debug_println!("  systemctl --user enable --now {}.service", service_name);
                    debug_println!("");
                    debug_println!("Starting helper in standalone mode for now...");

                    let log_path = config_dir_path.join(format!("{}-standalone.log", service_name));
                    let log_file = std::fs::File::create(&log_path).map_err(|e| {
                        HelperError::PlatformService(format!("Failed to create log file: {}", e))
                    })?;

                    let mut child = Command::new(&helper_path)
                        .arg("service")
                        .stdout(std::process::Stdio::from(log_file.try_clone().unwrap()))
                        .stderr(std::process::Stdio::from(log_file))
                        .spawn()
                        .map_err(|e| {
                            HelperError::PlatformService(format!(
                                "Failed to start helper process: {}",
                                e
                            ))
                        })?;

                    debug_println!("Helper started in standalone mode");
                    debug_println!("Process ID: {}", child.id());
                    debug_println!("Log file: {}", log_path.display());

                    std::thread::sleep(std::time::Duration::from_millis(500));

                    match child.try_wait() {
                        Ok(Some(status)) => {
                            return Err(HelperError::PlatformService(format!(
                                "Helper process exited immediately with status: {}. Check log file at: {}",
                                status,
                                log_path.display()
                            )));
                        }
                        Ok(None) => {
                            debug_println!("Helper process is running");
                        }
                        Err(e) => {
                            warn!("Failed to check helper process status: {}", e);
                        }
                    }

                    return Ok(());
                }
                return Err(HelperError::PlatformService(format!(
                    "Failed to reload systemd daemon: {}",
                    error
                )));
            }

            let output = Command::new("systemctl")
                .args([
                    "--user",
                    "enable",
                    "--now",
                    &format!("{}.service", service_name),
                ])
                .output()
                .map_err(|e| {
                    HelperError::PlatformService(format!("Failed to enable systemd service: {}", e))
                })?;

            if !output.status.success() {
                let error = String::from_utf8_lossy(&output.stderr);
                return Err(HelperError::PlatformService(format!(
                    "Failed to enable systemd service: {}",
                    error
                )));
            }
        }
    } else {
        debug_println!("Systemd not available, starting helper in standalone mode");

        let log_path = config_dir_path.join(format!("{}.log", service_name));
        let _pid_file = config_dir_path.join(format!("{}.pid", service_name));

        let log_file = std::fs::File::create(&log_path).map_err(|e| {
            HelperError::PlatformService(format!("Failed to create log file: {}", e))
        })?;

        let mut child = Command::new(&helper_path)
            .arg("service")
            .stdout(std::process::Stdio::from(log_file.try_clone().unwrap()))
            .stderr(std::process::Stdio::from(log_file))
            .spawn()
            .map_err(|e| {
                HelperError::PlatformService(format!("Failed to start helper process: {}", e))
            })?;

        debug_println!("Helper started in standalone mode");
        debug_println!("Process ID: {}", child.id());
        debug_println!("Note: The helper will need to be manually started after system reboot");
        debug_println!("Log file: {}", log_path.display());

        std::thread::sleep(std::time::Duration::from_millis(500));

        match child.try_wait() {
            Ok(Some(status)) => {
                return Err(HelperError::PlatformService(format!(
                    "Helper process exited immediately with status: {}. Check log file at: {}",
                    status,
                    log_path.display()
                )));
            }
            Ok(None) => {
                debug_println!("Helper process is running");
            }
            Err(e) => {
                warn!("Failed to check helper process status: {}", e);
            }
        }
    }

    Ok(())
}

pub fn uninstall_service(service_name: &str) -> Result<(), HelperError> {
    let systemctl_check = Command::new("systemctl").args(["--version"]).output();

    if systemctl_check.is_ok() && systemctl_check.unwrap().status.success() {
        let output = Command::new("systemctl")
            .args([
                "--user",
                "disable",
                "--now",
                &format!("{}.service", service_name),
            ])
            .output();

        if let Err(e) = output {
            debug_println!("Warning: Failed to disable systemd service: {}", e);
        } else if let Ok(output) = output
            && !output.status.success()
        {
            let error = String::from_utf8_lossy(&output.stderr);
            debug_println!("Warning: Failed to disable systemd service: {}", error);
        }
    } else {
        debug_println!("Systemd not available, looking for standalone helper process");

        let socket_path = crate::communication::get_default_socket_path()?;
        if socket_path.exists() {
            debug_println!("Helper socket found, attempting to stop helper gracefully");

            match crate::HelperClient::new("com.kftray.app".to_string()) {
                Ok(client) => {
                    if client.is_helper_available() {
                        debug_println!("Sending stop command to helper");
                        match client.stop_service() {
                            Ok(_) => {
                                debug_println!("Stop command sent successfully");
                                std::thread::sleep(std::time::Duration::from_millis(1000));
                            }
                            Err(e) => {
                                debug_println!("Failed to send stop command: {}", e);
                            }
                        }
                    }
                }
                Err(e) => {
                    debug_println!("Failed to create helper client: {}", e);
                }
            }

            if let Err(e) = fs::remove_file(&socket_path) {
                debug_println!("Failed to remove socket file: {}", e);
            } else {
                debug_println!("Socket file removed");
            }
        }

        debug_println!("Standalone helper cleanup completed");
    }

    let service_path = get_systemd_service_path(service_name)?;
    if service_path.exists() {
        fs::remove_file(&service_path).map_err(|e| {
            HelperError::PlatformService(format!("Failed to remove systemd service file: {}", e))
        })?;
    }

    let policy_path = get_polkit_policy_path(service_name)?;
    if policy_path.exists() {
        fs::remove_file(&policy_path).map_err(|e| {
            HelperError::PlatformService(format!("Failed to remove polkit policy file: {}", e))
        })?;
    }

    let systemctl_check = Command::new("systemctl").args(["--version"]).output();

    if systemctl_check.is_ok() && systemctl_check.unwrap().status.success() {
        let _ = Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .output();
    }

    Ok(())
}

pub fn run_service() -> Result<(), HelperError> {
    debug_println!("Starting helper service on Linux...");

    if tokio::runtime::Handle::try_current().is_ok() {
        debug_println!("Using existing tokio runtime");
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                let (pool_manager, network_manager, socket_path) =
                    super::common::initialize_components().await?;

                super::common::run_communication_server(pool_manager, network_manager, socket_path)
                    .await
            })
        })
    } else {
        match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => {
                debug_println!("Successfully created tokio runtime");
                runtime.block_on(async {
                    let (pool_manager, network_manager, socket_path) =
                        super::common::initialize_components().await?;

                    super::common::run_communication_server(
                        pool_manager,
                        network_manager,
                        socket_path,
                    )
                    .await
                })
            }
            Err(e) => {
                debug_println!("Failed to build tokio runtime: {}", e);
                Err(HelperError::PlatformService(format!(
                    "Failed to build tokio runtime: {}",
                    e
                )))
            }
        }
    }
}

fn get_systemd_service_path(service_name: &str) -> Result<PathBuf, HelperError> {
    let home_dir = dirs::home_dir()
        .ok_or_else(|| HelperError::PlatformService("Could not determine home directory".into()))?;

    let service_path = home_dir
        .join(".config/systemd/user")
        .join(format!("{}.service", service_name));

    Ok(service_path)
}

fn get_polkit_policy_path(service_name: &str) -> Result<PathBuf, HelperError> {
    let home_dir = dirs::home_dir()
        .ok_or_else(|| HelperError::PlatformService("Could not determine home directory".into()))?;

    let policy_path = home_dir
        .join(".local/share/polkit-1/actions")
        .join(format!("com.kftray.{}.policy", service_name));

    Ok(policy_path)
}

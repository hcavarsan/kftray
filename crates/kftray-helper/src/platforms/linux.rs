use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::Command,
};

use crate::{
    address_pool::AddressPoolManager,
    communication::{
        get_default_socket_path,
        start_communication_server,
    },
    error::HelperError,
    network::NetworkConfigManager,
};

const SYSTEMD_SERVICE_TEMPLATE: &str = r#"[Unit]
Description=KFTray privileged helper service
After=network.target

[Service]
Type=simple
ExecStart={{HELPER_PATH}} service
Restart=on-failure
RestartSec=5

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

    let service_content = SYSTEMD_SERVICE_TEMPLATE.replace("{{HELPER_PATH}}", &helper_path_str);

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

    let output = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output()
        .map_err(|e| {
            HelperError::PlatformService(format!("Failed to reload systemd daemon: {}", e))
        })?;

    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
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

    Ok(())
}

pub fn uninstall_service(service_name: &str) -> Result<(), HelperError> {
    let output = Command::new("systemctl")
        .args([
            "--user",
            "disable",
            "--now",
            &format!("{}.service", service_name),
        ])
        .output();

    if let Err(e) = output {
        eprintln!("Warning: Failed to disable systemd service: {}", e);
    } else if let Ok(output) = output {
        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            eprintln!("Warning: Failed to disable systemd service: {}", error);
        }
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

    let _ = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();

    Ok(())
}

pub fn run_service() -> Result<(), HelperError> {
    println!("Starting helper service on Linux...");

    if tokio::runtime::Handle::try_current().is_ok() {
        println!("Using existing tokio runtime");
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
                println!("Successfully created tokio runtime");
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
                eprintln!("Failed to build tokio runtime: {}", e);
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

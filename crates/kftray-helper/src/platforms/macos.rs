use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::Command,
};

use kftray_commons::utils::config_dir;

use crate::{
    communication::{
        get_default_socket_path,
        SOCKET_FILENAME,
    },
    error::HelperError,
};

const LAUNCHD_PLIST_TEMPLATE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{{SERVICE_NAME}}</string>
    <key>MachServices</key>
    <dict>
        <key>{{SERVICE_NAME}}</key>
        <true/>
    </dict>
    <key>ProgramArguments</key>
    <array>
        <string>{{HELPER_PATH}}</string>
        <string>service</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{{CONFIG_DIR}}/{{SERVICE_NAME}}.log</string>
    <key>StandardErrorPath</key>
    <string>{{CONFIG_DIR}}/{{SERVICE_NAME}}.err</string>
    <!-- Environment variables - CRITICAL for socket path location -->
    <key>EnvironmentVariables</key>
    <dict>
        <key>CONFIG_DIR</key>
        <string>{{CONFIG_DIR}}</string>
        <key>HOME</key>
        <string>{{HOME_DIR}}</string>
        <key>KFTRAY_CONFIG</key>
        <string>{{CONFIG_DIR}}</string>
        <key>SOCKET_PATH</key>
        <string>{{CONFIG_DIR}}/{{SOCKET_FILENAME}}</string>
        <key>USER</key>
        <string>{{CURRENT_USER}}</string>
    </dict>
    <key>ProcessType</key>
    <string>Interactive</string>
    <key>AbandonProcessGroup</key>
    <true/>
</dict>
</plist>"#;

pub fn install_service(_: &str) -> Result<(), HelperError> {
    let helper_path = std::env::current_exe().map_err(|e| {
        HelperError::PlatformService(format!("Failed to get current executable path: {e}"))
    })?;

    let full_service_name = "com.hcavarsan.kftray.helper";

    if !helper_path.exists() {
        return Err(HelperError::PlatformService(format!(
            "Helper binary not found at path: {}",
            helper_path.display()
        )));
    }

    println!("Installing privileged helper service: {full_service_name}");

    let config_dir_path = match config_dir::get_config_dir() {
        Ok(path) => path,
        Err(_) => PathBuf::from("/tmp"),
    };

    if !config_dir_path.exists() {
        println!("Creating config directory: {}", config_dir_path.display());
        fs::create_dir_all(&config_dir_path).map_err(|e| {
            HelperError::PlatformService(format!("Failed to create config directory: {e}"))
        })?;
    }

    let current_user = std::env::var("USER").unwrap_or_else(|_| "nobody".to_string());
    let current_group = current_user.clone();
    let home_dir = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());

    let helper_install_path = "/Library/PrivilegedHelperTools/com.hcavarsan.kftray.helper";
    let plist_content = LAUNCHD_PLIST_TEMPLATE
        .replace("{{SERVICE_NAME}}", full_service_name)
        .replace("{{HELPER_PATH}}", helper_install_path)
        .replace("{{CONFIG_DIR}}", &config_dir_path.to_string_lossy())
        .replace("{{CURRENT_USER}}", &current_user)
        .replace("{{CURRENT_GROUP}}", &current_group)
        .replace("{{HOME_DIR}}", &home_dir)
        .replace("{{SOCKET_FILENAME}}", SOCKET_FILENAME);

    let tmp_plist_path = format!("/tmp/{full_service_name}.plist");
    let mut tmp_file = fs::File::create(&tmp_plist_path).map_err(|e| {
        HelperError::PlatformService(format!("Failed to create temp plist file: {e}"))
    })?;
    tmp_file.write_all(plist_content.as_bytes()).map_err(|e| {
        HelperError::PlatformService(format!("Failed to write temp plist file: {e}"))
    })?;

    let socket_path = get_default_socket_path()?;

    let config_log_dir = config_dir_path.clone();
    let install_script = format!(
        r#"do shell script "
mkdir -p '{0}' &&
mkdir -p /Library/PrivilegedHelperTools &&
cp '{1}' '{2}' &&
chown root:wheel '{2}' &&
chmod 544 '{2}' &&
cp '{3}' '/Library/LaunchDaemons/{4}.plist' &&
chown root:wheel '/Library/LaunchDaemons/{4}.plist' &&
chmod 644 '/Library/LaunchDaemons/{4}.plist' &&
launchctl unload '/Library/LaunchDaemons/{4}.plist' 2>/dev/null || true &&
launchctl load -w '/Library/LaunchDaemons/{4}.plist' &&
sleep 2 &&
(launchctl list '{4}' | grep -v 'Could not find service' || echo 'Service not found yet, may be starting')
" with administrator privileges"#,
        config_log_dir.display(),
        helper_path.display(),
        helper_install_path,
        tmp_plist_path,
        full_service_name
    );

    println!("Requesting administrator privileges (one-time prompt)");
    let osa_output = Command::new("osascript")
        .args(["-e", &install_script])
        .output()
        .map_err(|e| {
            HelperError::PlatformService(format!("Failed to run installation script: {e}"))
        })?;

    if osa_output.status.success() {
        println!("Installation completed successfully:");

        let output = String::from_utf8_lossy(&osa_output.stdout);
        for line in output.lines() {
            println!("  {line}");
        }
    } else {
        let error = String::from_utf8_lossy(&osa_output.stderr);
        return Err(HelperError::PlatformService(format!(
            "Failed to install helper service: {error}"
        )));
    }

    if socket_path.exists() {
        println!("Verified: Socket file exists at {}", socket_path.display());
    } else {
        println!(
            "Warning: Socket file was not created at {}",
            socket_path.display()
        );
        println!("You may need to restart the service manually");
    }

    Ok(())
}

pub fn uninstall_service(_: &str) -> Result<(), HelperError> {
    let full_service_name = "com.hcavarsan.kftray.helper";

    let daemon_plist_path =
        PathBuf::from("/Library/LaunchDaemons").join(format!("{full_service_name}.plist"));

    println!(
        "Uninstalling system daemon from: {}",
        daemon_plist_path.display()
    );

    let config_dir_path = match config_dir::get_config_dir() {
        Ok(path) => path,
        Err(_) => PathBuf::from("/tmp"),
    };

    let user_socket_path = match config_dir::get_config_dir() {
        Ok(mut path) => {
            path.push(SOCKET_FILENAME);
            path
        }
        Err(_) => PathBuf::from(format!("/tmp/{SOCKET_FILENAME}")),
    };

    let tmp_socket_path = PathBuf::from(format!("/tmp/{SOCKET_FILENAME}"));

    let cmd_script = format!(
        r#"do shell script "
launchctl unload -w '{0}' 2>/dev/null || true &&
rm '{0}' 2>/dev/null || true &&
rm '/Library/PrivilegedHelperTools/com.hcavarsan.kftray.helper' 2>/dev/null || true &&
rm /usr/local/bin/kftray-helper 2>/dev/null || true &&
rm '{1}' 2>/dev/null || true &&
rm '{2}' 2>/dev/null || true &&
rm '{3}/{4}.log' 2>/dev/null || true &&
rm '{3}/{4}.err' 2>/dev/null || true
" with administrator privileges"#,
        daemon_plist_path.display(),
        user_socket_path.display(),
        tmp_socket_path.display(),
        config_dir_path.display(),
        full_service_name
    );

    println!("Requesting administrator privileges to uninstall helper");
    let osa_output = Command::new("osascript").args(["-e", &cmd_script]).output();

    match osa_output {
        Ok(output) if output.status.success() => {
            println!("Successfully uninstalled helper service");

            let out = String::from_utf8_lossy(&output.stdout);
            for line in out.lines() {
                println!("  {line}");
            }
        }
        Ok(output) => {
            let error = String::from_utf8_lossy(&output.stderr);
            println!("Warning: Some parts of uninstallation might have failed: {error}");

            println!("Attempting to clean up some files directly");
            self_cleanup_files(full_service_name, &user_socket_path, &tmp_socket_path);
        }
        Err(e) => {
            println!("Warning: Failed to run uninstallation script: {e}");

            println!("Attempting to clean up some files directly");
            self_cleanup_files(full_service_name, &user_socket_path, &tmp_socket_path);
        }
    }

    Ok(())
}

fn self_cleanup_files(_: &str, user_socket: &PathBuf, tmp_socket: &PathBuf) {
    let full_service_name = "com.hcavarsan.kftray.helper";

    for socket in &[user_socket, tmp_socket] {
        if socket.exists() {
            println!("Removing socket file at: {}", socket.display());
            if let Err(e) = fs::remove_file(socket) {
                println!("  Failed to remove socket: {e}");
            } else {
                println!("  Successfully removed socket");
            }
        }
    }

    if let Ok(config_dir) = config_dir::get_config_dir() {
        let log_file = config_dir.join(format!("{full_service_name}.log"));
        let err_file = config_dir.join(format!("{full_service_name}.err"));

        for file in &[log_file, err_file] {
            if file.exists() {
                println!("Removing log file at: {}", file.display());
                if let Err(e) = fs::remove_file(file) {
                    println!("  Failed to remove log file: {e}");
                } else {
                    println!("  Successfully removed log file");
                }
            }
        }
    }
}

pub fn run_service() -> Result<(), HelperError> {
    println!("Starting helper service on macOS...");

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
                eprintln!("Failed to build tokio runtime: {e}");
                Err(HelperError::PlatformService(format!(
                    "Failed to build tokio runtime: {e}"
                )))
            }
        }
    }
}

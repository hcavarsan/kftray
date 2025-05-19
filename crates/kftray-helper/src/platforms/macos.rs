use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::Command,
};

use crate::{
    communication::get_default_socket_path,
    error::HelperError,
};

const LAUNCHD_PLIST_TEMPLATE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{{SERVICE_NAME}}</string>
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
    <string>/tmp/{{SERVICE_NAME}}.log</string>
    <key>StandardErrorPath</key>
    <string>/tmp/{{SERVICE_NAME}}.err</string>
    <!-- Run as root -->
    <key>UserName</key>
    <string>root</string>
    <key>GroupName</key>
    <string>wheel</string>
    <!-- Socket permissions -->
    <key>Umask</key>
    <integer>0</integer>
    <!-- Give all necessary permissions -->
    <key>ProcessType</key>
    <string>Interactive</string>
    <key>AbandonProcessGroup</key>
    <true/>
</dict>
</plist>"#;

pub fn install_service(service_name: &str) -> Result<(), HelperError> {
    let helper_path = std::env::current_exe().map_err(|e| {
        HelperError::PlatformService(format!("Failed to get current executable path: {}", e))
    })?;

    if !helper_path.exists() {
        return Err(HelperError::PlatformService(format!(
            "Helper binary not found at path: {}",
            helper_path.display()
        )));
    }

    println!("Installing privileged helper service: {}", service_name);

    let plist_content = LAUNCHD_PLIST_TEMPLATE
        .replace("{{SERVICE_NAME}}", service_name)
        .replace("{{HELPER_PATH}}", "/usr/local/bin/kftray-helper");

    let tmp_plist_path = format!("/tmp/{}.plist", service_name);
    let mut tmp_file = fs::File::create(&tmp_plist_path).map_err(|e| {
        HelperError::PlatformService(format!("Failed to create temp plist file: {}", e))
    })?;
    tmp_file.write_all(plist_content.as_bytes()).map_err(|e| {
        HelperError::PlatformService(format!("Failed to write temp plist file: {}", e))
    })?;

    let socket_path = get_default_socket_path()?;

    let install_script = format!(
        r#"do shell script "
# Copy helper binary and set permissions
cp '{}' /usr/local/bin/kftray-helper
chmod +x /usr/local/bin/kftray-helper

# Install and load launchd daemon
mv '{}' '/Library/LaunchDaemons/{}.plist'
chown root:wheel '/Library/LaunchDaemons/{}.plist'
launchctl unload '/Library/LaunchDaemons/{}.plist' 2>/dev/null || true
launchctl load -w '/Library/LaunchDaemons/{}.plist'

# Wait for daemon to start
sleep 2

# Check if service is running
launchctl list {} | grep -v 'Could not find service' || echo 'Service not found yet, may be starting'

# Wait for socket to be created
for i in 1 2 3 4 5; do
  if [ -e '{}' ]; then
    echo 'Socket file exists, setting permissions'
    # Restrict to the kftray group (or the invoking user) – change as appropriate
    chown root:wheel '{}'
    chmod 660 '{}'
    ls -la '{}'
    break
  else
    echo 'Waiting for socket file to be created...'
    sleep 1
  fi
done
" with administrator privileges"#,
        helper_path.display(),
        tmp_plist_path,
        service_name,
        service_name,
        service_name,
        service_name,
        service_name,
        socket_path.display(),
        socket_path.display(),
        socket_path.display(),
        socket_path.display()
    );

    println!("Requesting administrator privileges (one-time prompt)");
    let osa_output = Command::new("osascript")
        .args(["-e", &install_script])
        .output()
        .map_err(|e| {
            HelperError::PlatformService(format!("Failed to run installation script: {}", e))
        })?;

    if osa_output.status.success() {
        println!("Installation completed successfully:");

        let output = String::from_utf8_lossy(&osa_output.stdout);
        for line in output.lines() {
            println!("  {}", line);
        }
    } else {
        let error = String::from_utf8_lossy(&osa_output.stderr);
        return Err(HelperError::PlatformService(format!(
            "Failed to install helper service: {}",
            error
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

pub fn uninstall_service(service_name: &str) -> Result<(), HelperError> {
    let daemon_plist_path =
        PathBuf::from("/Library/LaunchDaemons").join(format!("{}.plist", service_name));

    println!(
        "Uninstalling system daemon from: {}",
        daemon_plist_path.display()
    );

    let socket_path = get_default_socket_path()?;

    let cmd_script = format!(
        r#"do shell script "
launchctl unload -w {} 2>/dev/null || true
rm {} 2>/dev/null || true
rm /usr/local/bin/kftray-helper 2>/dev/null || true
rm {} 2>/dev/null || true
" with administrator privileges"#,
        daemon_plist_path.display(),
        daemon_plist_path.display(),
        socket_path.display()
    );

    println!("Requesting administrator privileges to uninstall helper");
    let osa_output = Command::new("osascript").args(["-e", &cmd_script]).output();

    match osa_output {
        Ok(output) if output.status.success() => {
            println!("Successfully uninstalled helper service");
        }
        Ok(output) => {
            let error = String::from_utf8_lossy(&output.stderr);
            println!(
                "Warning: Some parts of uninstallation might have failed: {}",
                error
            );
        }
        Err(e) => {
            println!("Warning: Failed to run uninstallation script: {}", e);
        }
    }

    if socket_path.exists() {
        println!("Socket file still exists, trying direct removal");
        let _ = fs::remove_file(&socket_path);
    }

    Ok(())
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
                eprintln!("Failed to build tokio runtime: {}", e);
                Err(HelperError::PlatformService(format!(
                    "Failed to build tokio runtime: {}",
                    e
                )))
            }
        }
    }
}

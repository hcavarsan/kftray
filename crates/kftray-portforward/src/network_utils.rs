use std::net::IpAddr;
use std::process::Command;
use std::str::FromStr;

use anyhow::{
    anyhow,
    Result,
};
use tracing::{
    debug,
    error,
    info,
    warn,
};

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn execute_command(cmd: &str, args: &[&str]) -> anyhow::Result<()> {
    let status = Command::new(cmd)
        .args(args)
        .status()
        .map_err(|e| anyhow!("failed to spawn `{}`: {}", cmd, e))?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("`{}` exited with {}", cmd, status))
    }
}

pub fn is_loopback_address(addr: &str) -> bool {
    if let Ok(ip) = IpAddr::from_str(addr) {
        return ip.is_loopback();
    }
    false
}

pub fn is_default_loopback_address(addr: &str) -> bool {
    addr == "127.0.0.1"
}

pub fn is_custom_loopback_address(addr: &str) -> bool {
    is_loopback_address(addr) && !is_default_loopback_address(addr)
}

pub async fn ensure_loopback_address(addr: &str) -> Result<()> {
    if !is_loopback_address(addr) {
        return Ok(());
    }

    if addr == "127.0.0.1" {
        return Ok(());
    }

    debug!("Ensuring loopback address {} is configured", addr);

    if is_address_accessible(addr).await {
        debug!("Loopback address {} is already accessible", addr);
        return Ok(());
    }

    info!("Configuring loopback address: {}", addr);

    let helper_result = configure_loopback_with_helper(addr).await;

    if helper_result.is_ok() {
        debug!(
            "Successfully configured loopback address via helper: {}",
            addr
        );

        if is_address_accessible(addr).await {
            debug!("Verified loopback address {} is now accessible", addr);
            return Ok(());
        } else {
            warn!(
                "Helper claimed success but address {} is still not accessible",
                addr
            );
        }
    } else if let Err(e) = helper_result {
        warn!("Failed to configure loopback address with helper: {}", e);
    }

    debug!("Falling back to traditional methods for configuring loopback address");

    #[cfg(target_os = "macos")]
    {
        info!("Using macOS-specific method for loopback configuration");
        configure_loopback_macos(addr)?;
    }

    #[cfg(target_os = "linux")]
    {
        info!("Using Linux-specific method for loopback configuration");
        configure_loopback_linux(addr)?;
    }

    #[cfg(target_os = "windows")]
    {
        info!("Using Windows-specific method for loopback configuration");
        configure_loopback_windows(addr)?;
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows",)))]
    {
        return Err(anyhow!(
            "Loopback address configuration not supported on this platform"
        ));
    }

    if is_address_accessible(addr).await {
        debug!("Successfully configured loopback address: {}", addr);
        Ok(())
    } else {
        Err(anyhow!(
            "Failed to configure loopback address {} after all attempts",
            addr
        ))
    }
}

async fn configure_loopback_with_helper(addr: &str) -> Result<()> {
    debug!("Checking if helper is available");

    let app_id = "com.kftray.app".to_string();

    let socket_path = kftray_helper::communication::get_default_socket_path()?;

    if !kftray_helper::client::socket_comm::is_socket_available(&socket_path) {
        return Err(anyhow!("Helper service is not available"));
    }

    debug!("Helper service is available, proceeding with configuration");

    let command = kftray_helper::messages::RequestCommand::Network(
        kftray_helper::messages::NetworkCommand::Add {
            address: addr.to_string(),
        },
    );

    match kftray_helper::client::socket_comm::send_request(&socket_path, &app_id, command) {
        Ok(_) => {
            debug!(
                "Successfully configured loopback address with helper: {}",
                addr
            );
            Ok(())
        }
        Err(e) => {
            error!("Helper failed to add loopback address: {}", e);
            Err(anyhow!(
                "Helper failed to configure loopback address: {}",
                e
            ))
        }
    }
}

async fn remove_loopback_with_helper(addr: &str) -> Result<()> {
    debug!("Checking if helper is available for address removal");

    let app_id = "com.kftray.app".to_string();

    let socket_path = kftray_helper::communication::get_default_socket_path()?;

    if !kftray_helper::client::socket_comm::is_socket_available(&socket_path) {
        return Err(anyhow!("Helper service is not available"));
    }

    debug!("Helper service is available, proceeding with address removal");

    let command = kftray_helper::messages::RequestCommand::Network(
        kftray_helper::messages::NetworkCommand::Remove {
            address: addr.to_string(),
        },
    );

    match kftray_helper::client::socket_comm::send_request(&socket_path, &app_id, command) {
        Ok(_) => {
            debug!(
                "Successfully removed loopback address with helper: {}",
                addr
            );
            Ok(())
        }
        Err(e) => {
            error!("Helper failed to remove loopback address: {}", e);
            Err(anyhow!("Helper failed to remove loopback address: {}", e))
        }
    }
}

pub async fn remove_loopback_address(addr: &str) -> Result<()> {
    if !is_loopback_address(addr) {
        return Ok(());
    }

    if addr == "127.0.0.1" {
        return Ok(());
    }

    debug!("Removing loopback address: {}", addr);

    let helper_result = remove_loopback_with_helper(addr).await;

    if helper_result.is_ok() {
        debug!("Successfully removed loopback address via helper: {}", addr);
        return Ok(());
    } else if let Err(e) = helper_result {
        warn!("Failed to remove loopback address with helper: {}", e);
    }

    debug!("Falling back to traditional methods for removing loopback address");

    #[cfg(target_os = "macos")]
    {
        info!("Using macOS-specific method for loopback removal");

        let check_output = Command::new("ifconfig")
            .args(["lo0"])
            .output()
            .map_err(|e| anyhow!("Failed to check loopback interface: {}", e))?;

        let output_str = String::from_utf8_lossy(&check_output.stdout);
        if !output_str.contains(addr) {
            debug!(
                "Loopback address {} is not configured on lo0, no removal needed",
                addr
            );
            return Ok(());
        }

        debug!("Trying to remove loopback address alias with osascript");
        let script = format!(
            r#"do shell script "ifconfig lo0 -alias {addr}" with administrator privileges"#
        );

        let result = Command::new("osascript").args(["-e", &script]).output();

        match result {
            Ok(output) if output.status.success() => {
                debug!(
                    "Successfully removed loopback address using osascript with admin privileges"
                );
                Ok(())
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                debug!(
                    "osascript command failed - stdout: {}, stderr: {}",
                    stdout, stderr
                );

                if stderr.contains("Can't assign requested address")
                    || stderr.contains("SIOCDIFADDR")
                {
                    warn!(
                        "Loopback address {} might already be removed or not exist",
                        addr
                    );
                    return Ok(());
                }

                if stderr.contains("User cancelled")
                    || stderr.contains("user cancelled")
                    || stderr.contains("cancelled")
                    || stderr.contains("User canceled")
                    || stderr.contains("canceled")
                    || stderr.contains("(-128)")
                    || output.status.code() == Some(1)
                {
                    Err(anyhow!("User cancelled loopback address removal"))
                } else {
                    Err(anyhow!("Failed to remove loopback address: {}", stderr))
                }
            }
            Err(e) => {
                debug!("Failed to execute osascript command: {}", e);
                Err(anyhow!("Failed to execute osascript: {}", e))
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        info!("Using Linux-specific method for loopback removal");
        if unsafe { libc::geteuid() } == 0 {
            execute_command("ip", &["addr", "del", addr, "dev", "lo"])?;
        } else {
            execute_command("sudo", &["ip", "addr", "del", addr, "dev", "lo"])?;
        }
        Ok(())
    }

    #[cfg(target_os = "windows")]
    {
        info!("Using Windows-specific method for loopback removal");
        Ok(())
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows",)))]
    {
        Err(anyhow!(
            "Loopback address removal not supported on this platform"
        ))
    }
}

pub async fn is_address_accessible(addr: &str) -> bool {
    let socket_addr = format!("{addr}:0");
    tokio::net::TcpListener::bind(socket_addr).await.is_ok()
}

#[cfg(target_os = "macos")]
fn configure_loopback_macos(addr: &str) -> Result<()> {
    let check_output = Command::new("ifconfig")
        .args(["lo0"])
        .output()
        .map_err(|e| anyhow!("Failed to check loopback interface: {}", e))?;

    let output_str = String::from_utf8_lossy(&check_output.stdout);
    if output_str.contains(addr) {
        debug!("Loopback address {} is already configured on lo0", addr);
        return Ok(());
    }

    debug!("Trying to add loopback address alias with osascript");
    let script =
        format!(r#"do shell script "ifconfig lo0 alias {addr}" with administrator privileges"#);

    let result = Command::new("osascript").args(["-e", &script]).output();

    match result {
        Ok(output) if output.status.success() => {
            debug!("Successfully added loopback address using osascript with admin privileges");
            Ok(())
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            debug!(
                "osascript command failed with exit code {}: {}",
                output.status.code().unwrap_or(-1),
                stderr
            );

            if stderr.contains("User cancelled")
                || stderr.contains("user cancelled")
                || stderr.contains("cancelled")
                || stderr.contains("User canceled")
                || stderr.contains("canceled")
                || stderr.contains("(-128)")
                || output.status.code() == Some(1)
            {
                // Common exit code for user cancellation
                Err(anyhow!("User cancelled loopback address configuration"))
            } else {
                Err(anyhow!("Failed to configure loopback address: {}", stderr))
            }
        }
        Err(e) => Err(anyhow!("Failed to execute osascript: {}", e)),
    }
}

#[cfg(target_os = "linux")]
fn configure_loopback_linux(addr: &str) -> Result<()> {
    if unsafe { libc::geteuid() } == 0 {
        execute_command("ip", &["addr", "add", addr, "dev", "lo"])?;

        execute_command(
            "ip",
            &["route", "add", &format!("{}/32", addr), "dev", "lo"],
        )
        .or_else(|e| {
            debug!("Route might already exist: {}", e);
            Ok(())
        })
    } else {
        execute_command("pkexec", &["ip", "addr", "add", addr, "dev", "lo"])
            .or_else(|_| execute_command("sudo", &["ip", "addr", "add", addr, "dev", "lo"]))?;

        execute_command(
            "pkexec",
            &["ip", "route", "add", &format!("{}/32", addr), "dev", "lo"],
        )
        .or_else(|_| {
            execute_command(
                "sudo",
                &["ip", "route", "add", &format!("{}/32", addr), "dev", "lo"],
            )
        })
        .or_else(|e| {
            debug!("Route might already exist: {}", e);
            Ok(())
        })
    }
}

#[cfg(target_os = "windows")]
fn configure_loopback_windows(addr: &str) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_loopback_address() {
        assert!(is_loopback_address("127.0.0.1"));
        assert!(is_loopback_address("127.0.0.2"));
        assert!(is_loopback_address("127.255.255.255"));
        assert!(!is_loopback_address("192.168.1.1"));
        assert!(!is_loopback_address("10.0.0.1"));
        assert!(!is_loopback_address("invalid-ip"));
    }

    #[test]
    fn test_is_default_loopback_address() {
        assert!(is_default_loopback_address("127.0.0.1"));
        assert!(!is_default_loopback_address("127.0.0.2"));
        assert!(!is_default_loopback_address("127.255.255.255"));
        assert!(!is_default_loopback_address("192.168.1.1"));
        assert!(!is_default_loopback_address("10.0.0.1"));
        assert!(!is_default_loopback_address("invalid-ip"));
    }

    #[test]
    fn test_is_custom_loopback_address() {
        assert!(!is_custom_loopback_address("127.0.0.1")); // Default loopback
        assert!(is_custom_loopback_address("127.0.0.2")); // Custom loopback
        assert!(is_custom_loopback_address("127.0.0.5")); // Custom loopback
        assert!(is_custom_loopback_address("127.255.255.255")); // Custom loopback
        assert!(!is_custom_loopback_address("192.168.1.1")); // Not loopback
        assert!(!is_custom_loopback_address("10.0.0.1")); // Not loopback
        assert!(!is_custom_loopback_address("invalid-ip")); // Invalid IP
    }

    #[tokio::test]
    async fn test_ensure_loopback_address_non_loopback() {
        assert!(ensure_loopback_address("192.168.1.1").await.is_ok());
    }

    #[tokio::test]
    async fn test_ensure_loopback_address_default() {
        assert!(ensure_loopback_address("127.0.0.1").await.is_ok());
    }

    #[cfg(test)]
    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn test_configure_loopback_windows() {
        assert!(configure_loopback_windows("127.0.0.2").is_ok());
    }
}

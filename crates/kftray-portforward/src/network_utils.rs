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
};

pub fn is_loopback_address(addr: &str) -> bool {
    if let Ok(ip) = IpAddr::from_str(addr) {
        return ip.is_loopback();
    }
    false
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

    #[cfg(target_os = "macos")]
    {
        // Try without admin privileges first
        let result = execute_command("ifconfig", &["lo0", "alias", addr, "up"]);
        if result.is_err() {
            debug!("Failed to configure loopback without admin privileges, trying with privileges");
            configure_loopback_macos(addr)?;
        }
    }

    #[cfg(target_os = "linux")]
    {
        configure_loopback_linux(addr)?;
    }

    #[cfg(target_os = "windows")]
    {
        configure_loopback_windows(addr)?;
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows",)))]
    {
        return Err(anyhow!(
            "Loopback address configuration not supported on this platform"
        ));
    }

    debug!("Successfully configured loopback address: {}", addr);
    Ok(())
}

pub async fn remove_loopback_address(addr: &str) -> Result<()> {
    if !is_loopback_address(addr) {
        return Ok(());
    }

    if addr == "127.0.0.1" {
        return Ok(());
    }

    debug!("Removing loopback address {}", addr);

    #[cfg(target_os = "macos")]
    {
        let result = execute_command("ifconfig", &["lo0", "-alias", addr]);
        if result.is_err() {
            debug!("Failed to remove loopback without admin privileges, trying with privileges");
            remove_loopback_macos(addr)?;
        }
    }

    #[cfg(target_os = "linux")]
    {
        remove_loopback_linux(addr)?;
    }

    #[cfg(target_os = "windows")]
    {
        Ok(())
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows",)))]
    {
        return Err(anyhow!(
            "Loopback address removal not supported on this platform"
        ));
    }

    debug!("Successfully removed loopback address: {}", addr);
    Ok(())
}

async fn is_address_accessible(addr: &str) -> bool {
    let socket_addr = format!("{}:0", addr);
    tokio::net::TcpListener::bind(socket_addr).await.is_ok()
}

#[cfg(target_os = "macos")]
fn configure_loopback_macos(addr: &str) -> Result<()> {
    if std::env::var("SUDO_USER").is_ok() {
        execute_command("ifconfig", &["lo0", "alias", addr, "up"])
    } else {
        info!("Admin privileges required to configure loopback address");
        let script = format!(
            r#"do shell script "ifconfig lo0 alias {} up" with administrator privileges"#,
            addr
        );
        execute_command("osascript", &["-e", &script])
    }
}

#[cfg(target_os = "macos")]
fn remove_loopback_macos(addr: &str) -> Result<()> {
    if std::env::var("SUDO_USER").is_ok() {
        execute_command("ifconfig", &["lo0", "-alias", addr])
    } else {
        info!("Admin privileges required to remove loopback address");
        let script = format!(
            r#"do shell script "ifconfig lo0 -alias {}" with administrator privileges"#,
            addr
        );
        execute_command("osascript", &["-e", &script])
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
        // Try without admin privileges first
        let result = execute_command("ip", &["addr", "add", addr, "dev", "lo"]);
        if result.is_err() {
            debug!("Failed to configure loopback without admin privileges, trying with privileges");
            execute_command("pkexec", &["ip", "addr", "add", addr, "dev", "lo"])
                .or_else(|_| execute_command("sudo", &["ip", "addr", "add", addr, "dev", "lo"]))?;
        }

        let route_result = execute_command(
            "ip",
            &["route", "add", &format!("{}/32", addr), "dev", "lo"],
        );
        if route_result.is_err() {
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
            })?;
        }
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn remove_loopback_linux(addr: &str) -> Result<()> {
    if unsafe { libc::geteuid() } == 0 {
        execute_command("ip", &["route", "del", &format!("{}/32", addr), "dev", "lo"])
            .or_else(|e| {
                debug!("Route might not exist: {}", e);
                Ok(())
            })?;

        execute_command("ip", &["addr", "del", addr, "dev", "lo"])
    } else {
        // Try without admin privileges first
        let route_result = execute_command(
            "ip",
            &["route", "del", &format!("{}/32", addr), "dev", "lo"],
        );
        if route_result.is_err() {
            execute_command(
                "pkexec",
                &["ip", "route", "del", &format!("{}/32", addr), "dev", "lo"],
            )
            .or_else(|_| {
                execute_command(
                    "sudo",
                    &["ip", "route", "del", &format!("{}/32", addr), "dev", "lo"],
                )
            })
            .or_else(|e| {
                debug!("Route might not exist: {}", e);
                Ok(())
            })?;
        }

        let addr_result = execute_command("ip", &["addr", "del", addr, "dev", "lo"]);
        if addr_result.is_err() {
            execute_command("pkexec", &["ip", "addr", "del", addr, "dev", "lo"])
                .or_else(|_| execute_command("sudo", &["ip", "addr", "del", addr, "dev", "lo"]))?;
        }
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn configure_loopback_windows(addr: &str) -> Result<()> {
    // Windows generally doesn't require explicit configuration for loopback
    // addresses as it has a more permissive loopback interface by default.
    Ok(())
}

fn execute_command(command: &str, args: &[&str]) -> Result<()> {
    debug!("Executing command: {} {:?}", command, args);

    let output = Command::new(command)
        .args(args)
        .output()
        .map_err(|e| anyhow!("Failed to execute command: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!("Command failed: {}", stderr);
        return Err(anyhow!("Command failed: {}", stderr));
    }

    Ok(())
}

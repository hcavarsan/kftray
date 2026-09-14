use std::net::IpAddr;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::process::Command;
use std::str::FromStr;

use anyhow::{
    Result,
    anyhow,
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

/// Outcome of attempting to release a loopback alias.
///
/// A caller cannot treat every non-removal the same way: one where the
/// helper is missing or `sudo` needs a password will not resolve itself by
/// retrying, so it must be surfaced once and recorded as unsatisfiable here,
/// rather than kept as a cleanup that is retried forever.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoopbackRelease {
    /// The alias was configured and has been removed.
    Released,
    /// Nothing was configured for this address; there was nothing to remove.
    AlreadyAbsent,
    /// The alias is still configured, but this process cannot escalate the
    /// privileges removal needs right now (helper unavailable, `sudo -n`
    /// needs a password). Retrying without a privilege change will not help.
    PrivilegeUnavailable(String),
    /// The alias is still configured and removal failed for a reason a later
    /// attempt might resolve.
    Failed(String),
}

/// Remove loopback address. Only uses helper service - skips osascript fallback
/// to avoid blocking on user interaction during stop operations.
pub async fn remove_loopback_address(addr: &str) -> Result<LoopbackRelease> {
    if !is_loopback_address(addr) {
        return Ok(LoopbackRelease::AlreadyAbsent);
    }

    if addr == "127.0.0.1" {
        return Ok(LoopbackRelease::AlreadyAbsent);
    }

    debug!("Removing loopback address: {}", addr);

    // Try helper service first (non-blocking)
    let helper_result = remove_loopback_with_helper(addr).await;

    if helper_result.is_ok() {
        debug!("Successfully removed loopback address via helper: {}", addr);
        return Ok(LoopbackRelease::Released);
    } else if let Err(e) = &helper_result {
        warn!("Failed to remove loopback address with helper: {}", e);
    }

    // Skip osascript fallback on macOS - it blocks waiting for user interaction
    // which can hang stop operations indefinitely.
    #[cfg(target_os = "macos")]
    {
        // An alias that is not there needs no removal. Without this check a
        // machine with no helper reports every stop as leaving an alias
        // behind, and the cleanup record that failure keeps would block
        // deleting the configuration for the rest of the session.
        //
        // Only a successful query proves absence: a failed one must stay an
        // error, or a broken `ifconfig` would read as confirmed cleanup.
        if !macos_alias_exists(addr)? {
            debug!("No loopback alias for {addr}; nothing to remove");

            return Ok(LoopbackRelease::AlreadyAbsent);
        }

        // The helper is the only non-blocking removal path on macOS, so its
        // absence is a privilege problem, not a transient one: nothing this
        // process retries on its own will remove the alias.
        warn!(
            "Could not remove loopback address {} via helper, and the osascript fallback would block.",
            addr
        );
        Ok(LoopbackRelease::PrivilegeUnavailable(format!(
            "Loopback address {addr} is still configured: removing it needs the helper"
        )))
    }

    #[cfg(target_os = "linux")]
    {
        info!("Using Linux-specific method for loopback removal");
        // Linux routes the whole 127.0.0.0/8 range to the loopback interface,
        // so an address can be bound without ever adding an explicit alias.
        // Deleting one that was never added is not a cleanup failure.
        // Only a successful query proves the alias is absent. A failed one
        // (for example `ip` missing from the process PATH) must not be read as
        // confirmed cleanup.
        if !linux_alias_exists(addr)? {
            debug!("No explicit loopback alias for {addr}; nothing to remove");

            return Ok(LoopbackRelease::AlreadyAbsent);
        }
        if unsafe { libc::geteuid() } == 0 {
            match execute_command("ip", &["addr", "del", addr, "dev", "lo"]) {
                Ok(()) => Ok(LoopbackRelease::Released),
                Err(e) => Ok(LoopbackRelease::Failed(e.to_string())),
            }
        } else {
            // `-n` keeps this non-interactive: a password prompt during a stop
            // would block until someone typed into a terminal that may not even
            // be attached. `sudo -n` failing outright, rather than prompting,
            // means this process cannot escalate without one, so it is a
            // privilege problem rather than a failure a retry can fix.
            match execute_command("sudo", &["-n", "ip", "addr", "del", addr, "dev", "lo"]) {
                Ok(()) => Ok(LoopbackRelease::Released),
                Err(e) => Ok(LoopbackRelease::PrivilegeUnavailable(format!(
                    "Removing loopback address {addr} needs a privileged `ip addr del`, and \
                     `sudo -n` failed (a password may be required): {e}"
                ))),
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        info!("Using Windows-specific method for loopback removal");
        Ok(LoopbackRelease::AlreadyAbsent)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows",)))]
    {
        Ok(LoopbackRelease::Failed(
            "Loopback address removal not supported on this platform".to_string(),
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
                Err(anyhow!("User cancelled loopback address configuration"))
            } else {
                Err(anyhow!("Failed to configure loopback address: {}", stderr))
            }
        }
        Err(e) => Err(anyhow!("Failed to execute osascript: {}", e)),
    }
}

/// Whether `addr` is configured on the loopback interface.
///
/// Matched field by field rather than by substring: `127.0.0.1` is a substring
/// of `127.0.0.10`, and treating one as the other would either skip a removal
/// that is owed or report one that is not.
#[cfg(target_os = "macos")]
fn macos_alias_exists(addr: &str) -> Result<bool> {
    let output = Command::new("/sbin/ifconfig")
        .arg("lo0")
        .output()
        .map_err(|error| anyhow!("Failed to query loopback aliases: {error}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "Failed to query loopback aliases: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(ifconfig_lists_address(
        &String::from_utf8_lossy(&output.stdout),
        addr,
    ))
}

/// Whether `ifconfig` output lists `addr` as a configured address.
#[cfg(target_os = "macos")]
fn ifconfig_lists_address(output: &str, addr: &str) -> bool {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            matches!(fields.next(), Some("inet" | "inet6")).then(|| fields.next())?
        })
        .any(|configured| configured == addr)
}

#[cfg(target_os = "linux")]
fn linux_alias_exists(addr: &str) -> Result<bool> {
    let output = Command::new("ip")
        .args(["-o", "addr", "show", "dev", "lo"])
        .output()
        .map_err(|error| anyhow!("Failed to query loopback aliases: {error}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "Failed to query loopback aliases: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .any(|field| field == addr || field.starts_with(&format!("{addr}/"))))
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
        execute_command("/usr/bin/pkexec", &["ip", "addr", "add", addr, "dev", "lo"])
            .or_else(|_| execute_command("sudo", &["ip", "addr", "add", addr, "dev", "lo"]))?;

        execute_command(
            "/usr/bin/pkexec",
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
fn configure_loopback_windows(_addr: &str) -> Result<()> {
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

    #[cfg(target_os = "macos")]
    #[test]
    fn a_configured_alias_is_matched_whole() {
        let output = "lo0: flags=8049<UP,LOOPBACK,RUNNING,MULTICAST> mtu 16384\n\
                      \toptions=1203<RXCSUM,TXCSUM,TXSTATUS,SW_TIMESTAMP>\n\
                      \tinet 127.0.0.1 netmask 0xff000000\n\
                      \tinet6 ::1 prefixlen 128\n\
                      \tinet 127.0.0.10 netmask 0xff000000\n";

        assert!(ifconfig_lists_address(output, "127.0.0.1"));
        assert!(ifconfig_lists_address(output, "127.0.0.10"));
        assert!(ifconfig_lists_address(output, "::1"));
        // A prefix of a configured address is not configured itself, and
        // treating it as present would keep reporting a removal that is owed.
        assert!(!ifconfig_lists_address(output, "127.0.0.2"));
        assert!(!ifconfig_lists_address(output, "127.0.0."));
        // Only the address field counts, never the flags or netmask columns.
        assert!(!ifconfig_lists_address(output, "0xff000000"));
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
        assert!(!is_custom_loopback_address("127.0.0.1"));
        assert!(is_custom_loopback_address("127.0.0.2"));
        assert!(is_custom_loopback_address("127.0.0.5"));
        assert!(is_custom_loopback_address("127.255.255.255"));
        assert!(!is_custom_loopback_address("192.168.1.1"));
        assert!(!is_custom_loopback_address("10.0.0.1"));
        assert!(!is_custom_loopback_address("invalid-ip"));
    }

    #[tokio::test]
    async fn test_ensure_loopback_address_non_loopback() {
        assert!(ensure_loopback_address("192.168.1.1").await.is_ok());
    }

    #[tokio::test]
    async fn test_ensure_loopback_address_default() {
        assert!(ensure_loopback_address("127.0.0.1").await.is_ok());
    }

    #[tokio::test]
    async fn test_remove_loopback_address_non_loopback_is_already_absent() {
        assert_eq!(
            remove_loopback_address("192.168.1.1").await.unwrap(),
            LoopbackRelease::AlreadyAbsent
        );
    }

    #[tokio::test]
    async fn test_remove_loopback_address_default_is_already_absent() {
        assert_eq!(
            remove_loopback_address("127.0.0.1").await.unwrap(),
            LoopbackRelease::AlreadyAbsent
        );
    }

    #[cfg(test)]
    #[cfg(target_os = "windows")]
    #[tokio::test]
    async fn test_configure_loopback_windows() {
        assert!(configure_loopback_windows("127.0.0.2").is_ok());
    }
}

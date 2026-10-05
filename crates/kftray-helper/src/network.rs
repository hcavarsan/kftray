use std::{
    process::Command,
    sync::Arc,
};

#[cfg(target_os = "macos")]
use log::warn;
use log::{
    debug,
    error,
    info,
};
use tokio::sync::Mutex;

use crate::error::HelperError;

pub struct NetworkConfigManager {
    lock: Arc<Mutex<()>>,
}

impl NetworkConfigManager {
    pub fn new() -> Result<Self, HelperError> {
        Ok(Self {
            lock: Arc::new(Mutex::new(())),
        })
    }

    pub async fn add_loopback_address(&self, address: &str) -> Result<(), HelperError> {
        debug!("Starting add_loopback_address for: {address}");

        let _guard = self.lock.lock().await;

        if let Err(e) = self.validate_loopback_address(address) {
            error!("Address validation failed: {e}");
            return Err(e);
        }

        debug!("Address validation successful for: {address}");

        #[cfg(target_os = "macos")]
        {
            debug!("Using macOS-specific implementation");
            let result = self.add_loopback_macos(address).await;
            debug!(
                "macOS implementation completed with result: {:?}",
                result.is_ok()
            );
            result
        }

        #[cfg(target_os = "linux")]
        {
            debug!("Using Linux-specific implementation");
            self.add_loopback_linux(address).await
        }

        #[cfg(target_os = "windows")]
        {
            debug!("Using Windows-specific implementation");
            self.add_loopback_windows(address).await
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            error!("Unsupported platform");
            Err(HelperError::UnsupportedPlatform)
        }
    }

    pub async fn remove_loopback_address(&self, address: &str) -> Result<(), HelperError> {
        let _guard = self.lock.lock().await;

        self.validate_loopback_address(address)?;

        if !self.is_address_configured(address).await? {
            info!("Address already removed, returning success: {address}");
            return Ok(());
        }

        debug!("Address is configured, proceeding with removal: {address}");

        #[cfg(target_os = "macos")]
        {
            self.remove_loopback_macos(address).await
        }

        #[cfg(target_os = "linux")]
        {
            self.remove_loopback_linux(address).await
        }

        #[cfg(target_os = "windows")]
        {
            self.remove_loopback_windows(address).await
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            Err(HelperError::UnsupportedPlatform)
        }
    }

    pub async fn list_loopback_addresses(&self) -> Result<Vec<String>, HelperError> {
        let _guard = self.lock.lock().await;

        #[cfg(target_os = "macos")]
        {
            self.list_loopback_macos().await
        }

        #[cfg(target_os = "linux")]
        {
            self.list_loopback_linux().await
        }

        #[cfg(target_os = "windows")]
        {
            self.list_loopback_windows().await
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            Err(HelperError::UnsupportedPlatform)
        }
    }

    /// Callers must already hold `self.lock`: this reads the platform list
    /// directly instead of through `list_loopback_addresses`, which takes the
    /// lock itself and would deadlock the non-reentrant mutex.
    async fn is_address_configured(&self, address: &str) -> Result<bool, HelperError> {
        #[cfg(target_os = "macos")]
        {
            debug!("Checking if address is configured on macOS: {address}");

            let address_owned = address.to_string();
            let result = tokio::task::spawn_blocking(move || {
                debug!("Executing ifconfig command to check if address exists");

                let output = Command::new("/sbin/ifconfig").arg("lo0").output();

                match output {
                    Ok(output) => {
                        let output_str = String::from_utf8_lossy(&output.stdout);
                        let mut found = false;

                        for line in output_str.lines() {
                            if line.contains("inet ") && line.contains(&address_owned) {
                                found = true;
                                break;
                            }
                        }

                        debug!("Address {address_owned} found status: {found}");
                        Ok(found)
                    }
                    Err(e) => {
                        error!("Error checking interface: {e}");
                        Err(HelperError::NetworkConfig(format!(
                            "Failed to check if address is configured: {e}"
                        )))
                    }
                }
            })
            .await;

            match result {
                Ok(inner_result) => inner_result,
                Err(e) => {
                    error!("Task execution error: {e}");
                    Err(HelperError::NetworkConfig(format!(
                        "Task execution error: {e}"
                    )))
                }
            }
        }

        #[cfg(target_os = "linux")]
        {
            let addresses = self.list_loopback_linux().await?;
            Ok(addresses.contains(&address.to_string()))
        }

        #[cfg(target_os = "windows")]
        {
            let addresses = self.list_loopback_windows().await?;
            Ok(addresses.contains(&address.to_string()))
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            Err(HelperError::UnsupportedPlatform)
        }
    }

    fn validate_loopback_address(&self, address: &str) -> Result<(), HelperError> {
        if !address.starts_with("127.") {
            return Err(HelperError::NetworkConfig(format!(
                "Invalid loopback address: {address}"
            )));
        }

        let parts: Vec<&str> = address.split('.').collect();
        if parts.len() != 4 {
            return Err(HelperError::NetworkConfig(format!(
                "Invalid IP address format: {address}"
            )));
        }

        for part in parts {
            if let Ok(_num) = part.parse::<u8>() {
                if part.starts_with('0') && part.len() > 1 {
                    return Err(HelperError::NetworkConfig(format!(
                        "Invalid IP address format (leading zeros): {address}"
                    )));
                }
            } else {
                return Err(HelperError::NetworkConfig(format!(
                    "Invalid IP address format (non-numeric): {address}"
                )));
            }
        }

        Ok(())
    }

    #[cfg(target_os = "macos")]
    async fn add_loopback_macos(&self, address: &str) -> Result<(), HelperError> {
        debug!("Adding loopback address on macOS: {address}");

        let address_owned = address.to_string();
        let result = tokio::task::spawn_blocking(move || {
            debug!("Executing ifconfig command to add loopback address");

            let output = Command::new("/sbin/ifconfig")
                .args(["lo0", "alias", &address_owned])
                .output();

            match output {
                Ok(output) => {
                    if output.status.success() {
                        info!("Successfully added loopback address: {address_owned}");
                        Ok(())
                    } else {
                        let error = String::from_utf8_lossy(&output.stderr);
                        error!("Failed to add loopback address: {error}");
                        Err(HelperError::NetworkConfig(format!(
                            "Failed to add loopback address: {error}"
                        )))
                    }
                }
                Err(e) => {
                    error!("Error executing ifconfig command: {e}");
                    Err(HelperError::NetworkConfig(format!(
                        "Failed to add loopback address: {e}"
                    )))
                }
            }
        })
        .await;

        match result {
            Ok(inner_result) => inner_result,
            Err(e) => {
                error!("Task execution error: {e}");
                Err(HelperError::NetworkConfig(format!(
                    "Task execution error: {e}"
                )))
            }
        }
    }

    #[cfg(target_os = "macos")]
    async fn remove_loopback_macos(&self, address: &str) -> Result<(), HelperError> {
        debug!("Removing loopback address on macOS: {address}");
        debug!("Thread ID: {:?}", std::thread::current().id());

        let address_owned = address.to_string();
        let result = tokio::task::spawn_blocking(move || {
            debug!("Executing ifconfig command to remove loopback address: {address_owned}");
            debug!("Blocking task thread ID: {:?}", std::thread::current().id());

            debug!("Directly calling ifconfig to remove loopback address");
            let output = Command::new("/sbin/ifconfig")
                .args(["lo0", "-alias", &address_owned])
                .output();

            match output {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);

                    debug!(
                        "ifconfig -alias result: success={}, stdout={}, stderr={}",
                        output.status.success(),
                        stdout,
                        stderr
                    );

                    if output.status.success() {
                        info!("Successfully removed loopback address: {address_owned}");
                        Ok(())
                    } else if stderr.contains("not found") || stderr.contains("No such process") {
                        info!("Address already removed, considering operation successful");
                        Ok(())
                    } else {
                        error!("Failed to remove loopback address, error: {stderr}");
                        Err(HelperError::NetworkConfig(format!(
                            "Failed to remove loopback address: {stderr}"
                        )))
                    }
                }
                Err(e) => {
                    error!("Error executing ifconfig command: {e}");
                    Err(HelperError::NetworkConfig(format!(
                        "Failed to remove loopback address: {e}"
                    )))
                }
            }
        })
        .await;

        match result {
            Ok(inner_result) => {
                debug!(
                    "Address removal task completed with result: {:?}",
                    inner_result.is_ok()
                );

                let verify_result = self.is_address_configured(address).await;
                match verify_result {
                    Ok(still_exists) => {
                        if !still_exists {
                            info!("Verified address was successfully removed: {address}");
                        } else {
                            warn!("Warning: Address still exists after removal attempt: {address}");
                        }
                    }
                    Err(e) => {
                        error!("Error verifying address removal: {e}");
                    }
                }

                inner_result
            }
            Err(e) => {
                error!("Task execution error: {e}");
                Err(HelperError::NetworkConfig(format!(
                    "Task execution error: {e}"
                )))
            }
        }
    }

    #[cfg(target_os = "macos")]
    async fn list_loopback_macos(&self) -> Result<Vec<String>, HelperError> {
        debug!("Listing loopback addresses on macOS");

        let result = tokio::task::spawn_blocking(move || {
            debug!("Executing ifconfig command to list loopback addresses");

            let output = Command::new("/sbin/ifconfig").arg("lo0").output();

            match output {
                Ok(output) => {
                    if output.status.success() {
                        let output_str = String::from_utf8_lossy(&output.stdout);
                        let mut addresses = Vec::new();

                        for line in output_str.lines() {
                            if line.contains("inet ") {
                                let parts: Vec<&str> = line.split_whitespace().collect();
                                if parts.len() >= 2 {
                                    let addr = parts[1];
                                    if addr.starts_with("127.") {
                                        addresses.push(addr.to_string());
                                    }
                                }
                            }
                        }

                        debug!("Found {} loopback addresses", addresses.len());
                        Ok(addresses)
                    } else {
                        let error = String::from_utf8_lossy(&output.stderr);
                        error!("Failed to list loopback addresses: {error}");
                        Err(HelperError::NetworkConfig(format!(
                            "Failed to list loopback addresses: {error}"
                        )))
                    }
                }
                Err(e) => {
                    error!("Error executing ifconfig command: {e}");
                    Err(HelperError::NetworkConfig(format!(
                        "Failed to list loopback addresses: {e}"
                    )))
                }
            }
        })
        .await;

        match result {
            Ok(inner_result) => inner_result,
            Err(e) => {
                error!("Task execution error: {e}");
                Err(HelperError::NetworkConfig(format!(
                    "Task execution error: {e}"
                )))
            }
        }
    }

    #[cfg(target_os = "linux")]
    async fn add_loopback_linux(&self, address: &str) -> Result<(), HelperError> {
        // `replace` rather than `add`: an address left on `lo` by a helper
        // that died before removing it must not make re-allocation fail.
        let output = Command::new("/sbin/ip")
            .args(["addr", "replace", &format!("{}/32", address), "dev", "lo"])
            .output()
            .map_err(|e| {
                HelperError::NetworkConfig(format!("Failed to add loopback address: {}", e))
            })?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(HelperError::NetworkConfig(format!(
                "Failed to add loopback address: {}",
                error
            )));
        }

        Ok(())
    }

    #[cfg(target_os = "linux")]
    async fn remove_loopback_linux(&self, address: &str) -> Result<(), HelperError> {
        let output = Command::new("/sbin/ip")
            .args(["addr", "del", &format!("{}/32", address), "dev", "lo"])
            .output()
            .map_err(|e| {
                HelperError::NetworkConfig(format!("Failed to remove loopback address: {}", e))
            })?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(HelperError::NetworkConfig(format!(
                "Failed to remove loopback address: {}",
                error
            )));
        }

        Ok(())
    }

    #[cfg(target_os = "linux")]
    async fn list_loopback_linux(&self) -> Result<Vec<String>, HelperError> {
        let output = Command::new("/sbin/ip")
            .args(["addr", "show", "dev", "lo"])
            .output()
            .map_err(|e| {
                HelperError::NetworkConfig(format!("Failed to list loopback addresses: {}", e))
            })?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(HelperError::NetworkConfig(format!(
                "Failed to list loopback addresses: {}",
                error
            )));
        }

        let output_str = String::from_utf8_lossy(&output.stdout);
        let mut addresses = Vec::new();

        for line in output_str.lines() {
            if line.contains("inet ") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let addr_with_prefix = parts[1];
                    let addr = addr_with_prefix.split('/').next().unwrap_or("");
                    if addr.starts_with("127.") {
                        addresses.push(addr.to_string());
                    }
                }
            }
        }

        Ok(addresses)
    }

    #[cfg(target_os = "windows")]
    async fn add_loopback_windows(&self, address: &str) -> Result<(), HelperError> {
        let output = Command::new("netsh")
            .args([
                "interface",
                "ipv4",
                "add",
                "address",
                "Loopback",
                address,
                "255.0.0.0",
            ])
            .output()
            .map_err(|e| {
                HelperError::NetworkConfig(format!("Failed to add loopback address: {}", e))
            })?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(HelperError::NetworkConfig(format!(
                "Failed to add loopback address: {}",
                error
            )));
        }

        Ok(())
    }

    #[cfg(target_os = "windows")]
    async fn remove_loopback_windows(&self, address: &str) -> Result<(), HelperError> {
        let output = Command::new("netsh")
            .args([
                "interface",
                "ipv4",
                "delete",
                "address",
                "Loopback",
                address,
            ])
            .output()
            .map_err(|e| {
                HelperError::NetworkConfig(format!("Failed to remove loopback address: {}", e))
            })?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(HelperError::NetworkConfig(format!(
                "Failed to remove loopback address: {}",
                error
            )));
        }

        Ok(())
    }

    #[cfg(target_os = "windows")]
    async fn list_loopback_windows(&self) -> Result<Vec<String>, HelperError> {
        let output = Command::new("netsh")
            .args(["interface", "ipv4", "show", "addresses", "Loopback"])
            .output()
            .map_err(|e| {
                HelperError::NetworkConfig(format!("Failed to list loopback addresses: {}", e))
            })?;

        if !output.status.success() {
            let error = String::from_utf8_lossy(&output.stderr);
            return Err(HelperError::NetworkConfig(format!(
                "Failed to list loopback addresses: {}",
                error
            )));
        }

        let output_str = String::from_utf8_lossy(&output.stdout);
        let mut addresses = Vec::new();

        for line in output_str.lines() {
            if line.contains("IP Address:") {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 2 {
                    let addr = parts[1].trim();
                    if addr.starts_with("127.") {
                        addresses.push(addr.to_string());
                    }
                }
            }
        }

        Ok(addresses)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::time::Duration;

    use super::*;

    /// Removing an address checks whether it is configured while already
    /// holding the manager lock; that check re-entering the lock deadlocked
    /// every later network request after the first removal.
    ///
    /// The address is picked from those absent on `lo`, so removal is a
    /// no-op and the test never touches the host's interfaces, even as root.
    #[tokio::test]
    async fn removing_an_unconfigured_address_does_not_deadlock_later_requests() {
        let manager = NetworkConfigManager::new().unwrap();

        let configured = manager.list_loopback_linux().await.unwrap();
        let address = (2..=254)
            .map(|octet| format!("127.0.0.{octet}"))
            .rev()
            .find(|address| !configured.contains(address))
            .expect("a free 127.0.0.x address");

        let removal = tokio::time::timeout(
            Duration::from_secs(5),
            manager.remove_loopback_address(&address),
        )
        .await
        .expect("removal must not deadlock on the manager lock");
        assert!(removal.is_ok(), "unconfigured address: {removal:?}");

        tokio::time::timeout(Duration::from_secs(5), manager.list_loopback_addresses())
            .await
            .expect("lock must be released after removal")
            .unwrap();
    }
}

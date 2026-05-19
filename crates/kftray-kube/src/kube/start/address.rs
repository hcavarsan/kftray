use kftray_commons::models::config_model::Config;
use log::{
    debug,
    error,
    info,
    warn,
};
use once_cell::sync::Lazy;
use tokio::sync::Mutex as TokioMutex;

use crate::port_forward_error::PortForwardError;

pub(super) static FALLBACK_ALLOCATION_MUTEX: Lazy<TokioMutex<()>> =
    Lazy::new(|| TokioMutex::new(()));

pub(super) async fn allocate_local_address_for_config(
    config: &mut Config,
) -> Result<String, PortForwardError> {
    if !config.auto_loopback_address {
        let address = config
            .local_address
            .clone()
            .unwrap_or_else(|| "127.0.0.1".to_string());

        if kftray_hosts::loopback::is_custom_loopback_address(&address) {
            info!("Configuring custom loopback address: {address}");
            if let Err(config_err) = kftray_hosts::loopback::ensure_loopback_address(&address).await
            {
                let error_msg = config_err.to_string();
                if error_msg.contains("cancelled") || error_msg.contains("canceled") {
                    return Err(PortForwardError::AddressAllocation(format!(
                        "Custom loopback address configuration cancelled: {error_msg}"
                    )));
                }
                warn!("Failed to configure custom loopback address {address}: {config_err}");
            }
        }

        return Ok(address);
    }

    let service_name = config
        .service
        .clone()
        .unwrap_or_else(|| format!("service-{}", config.id.unwrap_or_default()));

    match try_allocate_address(&service_name).await {
        Ok(allocated_address) => {
            info!("Auto-allocated address {allocated_address} for service {service_name}");
            config.local_address = Some(allocated_address.clone());

            info!(
                "Setting config.local_address to {} for config_id {}",
                allocated_address,
                config.id.unwrap_or_default()
            );
            if let Err(e) = save_allocated_address_to_db(config).await {
                error!(
                    "Failed to save allocated address {} to database for config {}: {}",
                    allocated_address,
                    config.id.unwrap_or_default(),
                    e
                );
            } else {
                info!(
                    "Successfully updated database with allocated address {} for config {}",
                    allocated_address,
                    config.id.unwrap_or_default()
                );
            }

            Ok(allocated_address)
        }
        Err(e) => {
            warn!(
                "Failed to auto-allocate address for service {service_name} via helper: {e}. Trying fallback allocation"
            );

            match try_fallback_allocate_and_save(&service_name, config).await {
                Ok(allocated_address) => {
                    info!(
                        "Fallback-allocated address {allocated_address} for service {service_name}"
                    );
                    Ok(allocated_address)
                }
                Err(fallback_err) => {
                    let fallback_msg = fallback_err.to_string();
                    if fallback_msg.contains("cancelled") || fallback_msg.contains("canceled") {
                        error!("Address allocation cancelled by user: {fallback_msg}");
                        return Err(fallback_err);
                    }

                    warn!(
                        "Fallback allocation also failed for service {service_name}: {fallback_msg}. Using default 127.0.0.1"
                    );
                    let default_address = "127.0.0.1".to_string();
                    config.local_address = Some(default_address.clone());
                    Ok(default_address)
                }
            }
        }
    }
}

/// Synchronous helper for address allocation via helper service IPC.
/// Must be called from spawn_blocking to avoid blocking the Tokio runtime.
fn try_allocate_address_sync(service_name: &str) -> Result<String, PortForwardError> {
    let app_id = "com.kftray.app".to_string();

    let socket_path = kftray_helper::communication::get_default_socket_path()
        .map_err(|e| PortForwardError::AddressAllocation(e.to_string()))?;

    if !kftray_helper::client::socket_comm::is_socket_available(&socket_path) {
        return Err(PortForwardError::AddressAllocation(
            "Helper service is not available".to_string(),
        ));
    }

    let command = kftray_helper::messages::RequestCommand::Address(
        kftray_helper::messages::AddressCommand::Allocate {
            service_name: service_name.to_string(),
        },
    );

    match kftray_helper::client::socket_comm::send_request(&socket_path, &app_id, command) {
        Ok(response) => match response.result {
            kftray_helper::messages::RequestResult::StringSuccess(address) => Ok(address),
            kftray_helper::messages::RequestResult::Error(error) => {
                Err(PortForwardError::AddressAllocation(error))
            }
            _ => Err(PortForwardError::AddressAllocation(
                "Unexpected response format".to_string(),
            )),
        },
        Err(e) => Err(PortForwardError::AddressAllocation(e.to_string())),
    }
}

async fn try_allocate_address(service_name: &str) -> Result<String, PortForwardError> {
    use std::time::Duration;

    const ADDRESS_ALLOCATE_TIMEOUT: Duration = Duration::from_secs(5);

    let service_name_owned = service_name.to_string();

    // Wrap blocking helper-service IPC in spawn_blocking with timeout,
    // matching the pattern used in stop.rs::release_address_with_fallback.
    let result = tokio::time::timeout(ADDRESS_ALLOCATE_TIMEOUT, async {
        let svc = service_name_owned.clone();
        tokio::task::spawn_blocking(move || try_allocate_address_sync(&svc))
            .await
            .map_err(|e| {
                PortForwardError::AddressAllocation(format!(
                    "Address allocation task panicked: {e}"
                ))
            })
    })
    .await;

    match result {
        Ok(Ok(inner)) => inner,
        Ok(Err(e)) => Err(e),
        Err(_) => Err(PortForwardError::AddressAllocation(format!(
            "Address allocation timed out after {:?}",
            ADDRESS_ALLOCATE_TIMEOUT
        ))),
    }
}

async fn try_fallback_allocate_and_save(
    service_name: &str, config: &mut Config,
) -> Result<String, PortForwardError> {
    // Acquire lock only to find and reserve an available address, then release
    let candidate = {
        let _lock = FALLBACK_ALLOCATION_MUTEX.lock().await;
        debug!("Acquired fallback allocation lock for service: {service_name}");

        let allocated_addresses = get_allocated_loopback_addresses().await;

        let mut found = None;
        for octet in 2..255u8 {
            let address = format!("127.0.0.{octet}");

            if allocated_addresses.contains(&address) {
                debug!("Address {address} already allocated to another config, skipping");
                continue;
            }

            if kftray_hosts::loopback::is_address_accessible(&address).await {
                debug!("Address {address} is already in use on system, skipping");
                continue;
            }

            found = Some(address);
            break;
        }
        found
    };
    // Lock released here

    let address = match candidate {
        Some(addr) => addr,
        None => {
            return Err(PortForwardError::AddressAllocation(
                "No available addresses found in fallback allocation".to_string(),
            ));
        }
    };

    match kftray_hosts::loopback::ensure_loopback_address(&address).await {
        Ok(_) => {
            debug!(
                "Successfully allocated and configured fallback address: {address} for service: {service_name}"
            );

            config.local_address = Some(address.clone());
            info!(
                "Setting config.local_address to {} (fallback) for config_id {}",
                address,
                config.id.unwrap_or_default()
            );

            match save_allocated_address_to_db(config).await {
                Ok(_) => {
                    info!(
                        "Successfully updated database with fallback allocated address {} for config {}",
                        address,
                        config.id.unwrap_or_default()
                    );
                    Ok(address)
                }
                Err(e) => {
                    error!(
                        "Failed to save fallback allocated address {} to database for config {}: {}",
                        address,
                        config.id.unwrap_or_default(),
                        e
                    );
                    if let Err(cleanup_err) =
                        kftray_hosts::loopback::remove_loopback_address(&address).await
                    {
                        error!(
                            "Failed to cleanup address {} after DB save failure: {}",
                            address, cleanup_err
                        );
                    }
                    Err(PortForwardError::AddressAllocation(format!(
                        "Failed to save allocated address: {e}"
                    )))
                }
            }
        }
        Err(e) => {
            let error_msg = e.to_string();
            debug!("Failed to configure fallback address {address}: {error_msg}");

            if error_msg.contains("cancelled") || error_msg.contains("canceled") {
                return Err(PortForwardError::AddressAllocation(format!(
                    "Address allocation cancelled by user: {error_msg}"
                )));
            }

            Err(PortForwardError::AddressAllocation(format!(
                "Failed to configure fallback address: {error_msg}"
            )))
        }
    }
}

async fn get_allocated_loopback_addresses() -> std::collections::HashSet<String> {
    use std::collections::HashSet;

    let mut allocated = HashSet::new();

    if let Ok(configs) = kftray_commons::config::get_configs().await {
        for config in configs {
            if let Some(addr) = &config.local_address
                && kftray_hosts::loopback::is_custom_loopback_address(addr)
                && config.auto_loopback_address
            {
                allocated.insert(addr.clone());
                debug!(
                    "Found allocated address {} for config {}",
                    addr,
                    config.id.unwrap_or_default()
                );
            }
        }
    }

    debug!("Currently allocated loopback addresses: {allocated:?}");
    allocated
}

async fn save_allocated_address_to_db(config: &Config) -> Result<(), PortForwardError> {
    use kftray_commons::utils::config::update_config;

    match update_config(config.clone()).await {
        Ok(_) => {
            info!(
                "Successfully saved allocated address to database for config {}",
                config.id.unwrap_or_default()
            );
            Ok(())
        }
        Err(e) => {
            error!("Failed to update config in database: {e}");
            Err(PortForwardError::Internal(e))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_config() -> Config {
        Config {
            id: Some(1),
            context: Some("test-context".to_string()),
            kubeconfig: None,
            namespace: "test-namespace".to_string(),
            service: Some("test-service".to_string()),
            alias: Some("test-alias".to_string()),
            local_port: Some(0),
            remote_port: Some(8080),
            protocol: "tcp".to_string(),
            workload_type: Some("service".to_string()),
            target: None,
            local_address: None,
            remote_address: None,
            domain_enabled: None,
            auto_loopback_address: false,
            http_logs_enabled: Some(false),
            http_logs_max_file_size: Some(10 * 1024 * 1024),
            http_logs_retention_days: Some(7),
            http_logs_auto_cleanup: Some(true),
            exposure_type: None,
            cert_manager_enabled: None,
            cert_issuer: None,
            cert_issuer_kind: None,
            ingress_class: None,
            ingress_annotations: None,
        }
    }

    #[tokio::test]
    async fn test_allocate_local_address_for_config_disabled() {
        let mut config = setup_test_config();
        config.auto_loopback_address = false;
        config.local_address = Some("192.168.1.1".to_string());

        let result = allocate_local_address_for_config(&mut config)
            .await
            .unwrap();
        assert_eq!(result, "192.168.1.1");
        assert_eq!(config.local_address, Some("192.168.1.1".to_string()));
    }

    async fn mock_allocate_local_address_for_config(config: &mut Config) -> String {
        if !config.auto_loopback_address {
            return config
                .local_address
                .clone()
                .unwrap_or_else(|| "127.0.0.1".to_string());
        }

        let service_name = config
            .service
            .clone()
            .unwrap_or_else(|| format!("service-{}", config.id.unwrap_or_default()));

        let mock_address = format!("127.0.0.{}", 100 + (service_name.len() % 155));
        config.local_address = Some(mock_address.clone());
        mock_address
    }

    #[tokio::test]
    async fn test_allocate_local_address_for_config_mocked() {
        let mut config = setup_test_config();
        config.auto_loopback_address = true;
        config.local_address = None;

        let result = mock_allocate_local_address_for_config(&mut config).await;

        assert!(result.starts_with("127.0.0."));
        assert_ne!(result, "127.0.0.1");
        assert_eq!(config.local_address, Some(result.clone()));

        let mut config2 = setup_test_config();
        config2.auto_loopback_address = true;
        config2.local_address = None;
        let result2 = mock_allocate_local_address_for_config(&mut config2).await;
        assert_eq!(result, result2);
    }
}

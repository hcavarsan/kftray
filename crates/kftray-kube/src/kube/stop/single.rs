use kftray_commons::{
    models::{
        config_state_model::ConfigState,
        response::CustomResponse,
    },
    utils::{
        config_state::update_config_state_with_mode,
        db_mode::DatabaseMode,
        timeout_manager::cancel_timeout_for_forward,
    },
};
use kftray_hosts::hostsfile::{
    remove_host_entry,
    remove_ssl_host_entry,
};
use tracing::{
    debug,
    error,
    info,
    warn,
};

use super::address::release_address_with_fallback;
use super::cleanup::{
    delete_proxy_cluster_resources,
    load_configs,
};
use crate::kube::shared_client::ServiceClientKey;
use crate::port_forward_error::PortForwardError;
use crate::registry::PORT_FORWARD_REGISTRY;

pub async fn stop_port_forward(config_id: String) -> Result<CustomResponse, PortForwardError> {
    stop_port_forward_with_mode(config_id, DatabaseMode::File).await
}

pub async fn stop_port_forward_with_mode(
    config_id: String, mode: DatabaseMode,
) -> Result<CustomResponse, PortForwardError> {
    let config_id_parsed =
        config_id
            .parse::<i64>()
            .map_err(|_| PortForwardError::ConfigurationError {
                message: "Invalid config ID".to_string(),
            })?;

    let configs = load_configs(mode).await;

    // Expose configs must be stopped by callers via kftray_expose, not here.
    if let Some(config) = configs.iter().find(|c| c.id == Some(config_id_parsed))
        && config.workload_type.as_deref() == Some("expose")
    {
        return Err(PortForwardError::ConfigurationError {
            message: "expose workload_type must be stopped via kftray_expose, not kftray_kube"
                .to_string(),
        });
    }

    let pf_key = PORT_FORWARD_REGISTRY.find_key_for_config(config_id_parsed);

    if let Some(pf_key) = pf_key {
        if let Some(config) = configs.iter().find(|c| c.id == Some(config_id_parsed)) {
            info!(
                "Found config {} during stop with local_address: {:?} and auto_loopback_address: {}",
                config_id, config.local_address, config.auto_loopback_address
            );
            if let Some(local_addr) = &config.local_address
                && kftray_hosts::loopback::is_custom_loopback_address(local_addr)
            {
                info!(
                    "Cleaning up loopback address for config {config_id}: {local_addr} (auto_allocated: {})",
                    config.auto_loopback_address
                );
                release_address_with_fallback(local_addr).await;
            }
        }

        if let Some(entry) = PORT_FORWARD_REGISTRY.remove_process(&pf_key) {
            entry.process.cleanup_and_abort().await;
        }

        // Cancel any in-progress recovery for this config
        if let Some((_, manager)) =
            crate::kube::proxy_recovery::RECOVERY_MANAGERS.remove(&config_id_parsed)
        {
            manager.cancel();
            info!("Cancelled recovery manager for config {}", config_id_parsed);
        }
        // Clean up recovery coordination lock
        crate::kube::proxy_recovery::remove_recovery_lock(config_id_parsed);

        if let Some(config) = configs.iter().find(|c| c.id == Some(config_id_parsed)) {
            let needs_cluster_cleanup =
                config.protocol == "udp" || config.workload_type.as_deref() == Some("proxy");

            if needs_cluster_cleanup {
                info!(
                    "Cleaning up cluster resources for config {config_id} \
                    (protocol={}, workload_type={:?})",
                    config.protocol, config.workload_type
                );
                let client_key =
                    ServiceClientKey::new(config.context.clone(), config.kubeconfig.clone());
                match PORT_FORWARD_REGISTRY.acquire_client(client_key).await {
                    Ok(shared_client) => {
                        let client = kube::Client::clone(&shared_client);
                        delete_proxy_cluster_resources(client, &config.namespace, config_id_parsed)
                            .await;
                    }
                    Err(e) => error!(
                        "Failed to get K8s client for cluster cleanup of config {config_id}: {e}"
                    ),
                }
            }
        }

        cancel_timeout_for_forward(config_id_parsed).await;

        let service_name = match &pf_key.slot {
            crate::registry::PortForwardSlot::Named(name) => name.as_str(),
            crate::registry::PortForwardSlot::Expose => "expose",
        };

        if let Some(config) = configs.iter().find(|c| c.id == Some(config_id_parsed)) {
            if config.domain_enabled.unwrap_or_default() {
                if let Err(e) = remove_host_entry(&config_id) {
                    error!("Failed to remove host entry for ID {config_id}: {e}");

                    let config_state = ConfigState::new(config_id_parsed, false);
                    if let Err(e) = update_config_state_with_mode(&config_state, mode).await {
                        error!("Failed to update config state: {e}");
                    }
                    return Err(PortForwardError::HostsFile(e.to_string()));
                }

                if let Err(e) = remove_ssl_host_entry(&config_id) {
                    error!("Failed to remove SSL host entry for ID {config_id}: {e}");
                }
            }
        } else {
            warn!("Config with id '{config_id}' not found in database.");
        }

        let config_state = ConfigState::new(config_id_parsed, false);
        if let Err(e) = update_config_state_with_mode(&config_state, mode).await {
            error!("Failed to update config state: {e}");
        }

        if let Some(config) = configs.iter().find(|c| c.id == Some(config_id_parsed)) {
            let client_key =
                ServiceClientKey::new(config.context.clone(), config.kubeconfig.clone());
            PORT_FORWARD_REGISTRY.invalidate_client(&client_key);
            debug!("Invalidated client for config {}", config_id);
        }

        Ok(CustomResponse {
            id: None,
            service: service_name.to_string(),
            namespace: String::new(),
            local_port: 0,
            remote_port: 0,
            context: String::new(),
            protocol: String::new(),
            stdout: String::from("Service port forwarding has been stopped"),
            stderr: String::new(),
            status: 0,
        })
    } else {
        let config_id_parsed = config_id.parse::<i64>().unwrap_or_default();

        let configs = load_configs(mode).await;

        let config = configs.iter().find(|c| c.id == Some(config_id_parsed));

        if config.is_none() {
            return Err(PortForwardError::Internal(format!(
                "No port forwarding process found for config_id '{config_id}'"
            )));
        }

        let config_state = ConfigState::new(config_id_parsed, false);
        if let Err(e) = update_config_state_with_mode(&config_state, mode).await {
            error!("Failed to update config state: {e}");
        }

        debug!("No active process found for config_id '{config_id}', marked as stopped");
        Ok(CustomResponse {
            id: Some(config_id_parsed),
            service: String::new(),
            namespace: String::new(),
            local_port: 0,
            remote_port: 0,
            context: String::new(),
            protocol: String::new(),
            stdout: String::from("Port forwarding was already stopped"),
            stderr: String::new(),
            status: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::port_forward::PortForwardProcess;
    use crate::registry::PortForwardKey;

    async fn create_dummy_handle() -> PortForwardProcess {
        let handle = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            Ok(())
        });
        PortForwardProcess::new(handle, "test-config".to_string())
    }

    #[tokio::test]
    async fn test_stop_port_forward_nonexistent() {
        let result = stop_port_forward("999".to_string()).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No port forwarding process found")
        );
    }

    #[tokio::test]
    async fn test_stop_port_forward_with_handle() {
        use crate::kube::shared_client::ServiceClientKey;

        let dummy_handle = create_dummy_handle().await;
        let key = PortForwardKey::named(20100, "test-service");
        let client_key = ServiceClientKey::new(None, None);

        PORT_FORWARD_REGISTRY.insert_process(key.clone(), dummy_handle, client_key);
        assert!(
            PORT_FORWARD_REGISTRY.has_process_for_config(20100),
            "Process should be present"
        );

        if let Some(entry) = PORT_FORWARD_REGISTRY.remove_process(&key) {
            entry.process.abort();
        }

        assert!(
            !PORT_FORWARD_REGISTRY.has_process_for_config(20100),
            "Process handle should be removed"
        );
    }

    #[tokio::test]
    async fn test_stop_port_forward_with_multiple_handles() {
        use crate::kube::shared_client::ServiceClientKey;

        let dummy_handle1 = create_dummy_handle().await;
        let dummy_handle2 = create_dummy_handle().await;
        let dummy_handle3 = create_dummy_handle().await;

        let key1 = PortForwardKey::named(30101, "service1");
        let key2 = PortForwardKey::named(30102, "service2");
        let key3 = PortForwardKey::named(30103, "service3");
        let client_key = ServiceClientKey::new(None, None);

        PORT_FORWARD_REGISTRY.insert_process(key1.clone(), dummy_handle1, client_key.clone());
        PORT_FORWARD_REGISTRY.insert_process(key2.clone(), dummy_handle2, client_key.clone());
        PORT_FORWARD_REGISTRY.insert_process(key3.clone(), dummy_handle3, client_key);

        assert!(PORT_FORWARD_REGISTRY.has_process_for_config(30101));
        assert!(PORT_FORWARD_REGISTRY.has_process_for_config(30102));
        assert!(PORT_FORWARD_REGISTRY.has_process_for_config(30103));

        // Remove key2
        if let Some(entry) = PORT_FORWARD_REGISTRY.remove_process(&key2) {
            entry.process.abort();
        } else {
            panic!("key2 should have been found for removal");
        }

        assert!(PORT_FORWARD_REGISTRY.has_process_for_config(30101));
        assert!(!PORT_FORWARD_REGISTRY.has_process_for_config(30102));
        assert!(PORT_FORWARD_REGISTRY.has_process_for_config(30103));

        if let Some(e) = PORT_FORWARD_REGISTRY.remove_process(&key1) {
            e.process.abort();
        }
        if let Some(e) = PORT_FORWARD_REGISTRY.remove_process(&key3) {
            e.process.abort();
        }
    }

    #[tokio::test]
    async fn test_format_composite_key() {
        let config_id = 123;
        let service_id = "my-service";

        let composite_key = format!("{config_id}_{service_id}");
        assert_eq!(composite_key, "123_my-service");

        let (config_id_str, service_name) = composite_key.split_once('_').unwrap();
        assert_eq!(config_id_str, "123");
        assert_eq!(service_name, "my-service");

        let config_id_parsed = config_id_str.parse::<i64>().unwrap();
        assert_eq!(config_id_parsed, 123);
    }

    #[tokio::test]
    async fn test_invalid_composite_key() {
        let invalid_key = "invalid-key-without-underscore";
        let parts = invalid_key.split_once('_');
        assert!(parts.is_none());
    }
}

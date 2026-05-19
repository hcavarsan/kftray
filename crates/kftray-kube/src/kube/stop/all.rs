use std::collections::HashMap;
use std::sync::Arc;

use futures::stream::{
    FuturesUnordered,
    StreamExt,
};
use kftray_commons::{
    config_state::get_configs_state,
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
use tracing::{
    debug,
    error,
    info,
    warn,
};

use kftray_hosts::hostsfile::{
    remove_all_host_entries,
    remove_host_entry,
    remove_ssl_host_entry,
};
use crate::kube::shared_client::ServiceClientKey;
use crate::port_forward_error::PortForwardError;
use crate::registry::PORT_FORWARD_REGISTRY;

use super::address::release_address_with_fallback;
use super::cleanup::{
    delete_proxy_cluster_resources,
    load_configs,
};

pub async fn stop_all_port_forward() -> Result<Vec<CustomResponse>, PortForwardError> {
    stop_all_port_forward_with_mode(DatabaseMode::File).await
}

pub async fn stop_all_port_forward_with_mode(
    mode: DatabaseMode,
) -> Result<Vec<CustomResponse>, PortForwardError> {
    kftray_ssl::ensure_crypto_provider_installed();
    info!("Attempting to stop all port forwards in mode: {mode:?}");

    let mut responses = Vec::with_capacity(1024);

    let all_keys = PORT_FORWARD_REGISTRY.all_keys();

    if all_keys.is_empty() {
        debug!("No port forwarding processes to stop");
    }

    for key in &all_keys {
        cancel_timeout_for_forward(key.config_id).await;
    }

    let running_configs_state = match get_configs_state().await {
        Ok(states) => states
            .into_iter()
            .filter(|s| s.is_running)
            .map(|s| s.config_id)
            .collect::<Vec<i64>>(),
        Err(e) => {
            let error_message = format!("Failed to retrieve config states: {e}");
            error!("{error_message}");
            return Err(PortForwardError::Internal(error_message));
        }
    };

    let configs = load_configs(mode).await;

    let config_map: HashMap<i64, &kftray_commons::models::config_model::Config> = configs
        .iter()
        .filter_map(|c| c.id.map(|id| (id, c)))
        .collect();

    // Remove all processes from registry at once
    let removed_entries = PORT_FORWARD_REGISTRY.remove_all();

    let config_map = Arc::new(config_map);

    let mut abort_handles: FuturesUnordered<_> = removed_entries
        .into_iter()
        .map(|(pf_key, entry)| {
            let config_map_ref = Arc::clone(&config_map);

            async move {
                let config_id_parsed = pf_key.config_id;
                let config_id_str = config_id_parsed.to_string();
                let service_id = match &pf_key.slot {
                    crate::registry::PortForwardSlot::Named(name) => name.clone(),
                    crate::registry::PortForwardSlot::Expose => "expose".to_string(),
                };

                let config_option = config_map_ref.get(&config_id_parsed).cloned();

                if let Some(config) = &config_option {
                    if config.domain_enabled.unwrap_or_default() {
                        if let Err(e) = remove_host_entry(&config_id_str) {
                            error!(
                                "Failed to remove host entry for ID {config_id_str}: {e}"
                            );
                        }

                        if let Err(e) = remove_ssl_host_entry(&config_id_str) {
                            error!(
                                "Failed to remove SSL host entry for ID {config_id_str}: {e}"
                            );
                        }
                    }
                } else {
                    warn!("Config with id '{config_id_str}' not found in database.");
                }

                info!(
                    "Aborting port forwarding task for config_id: {config_id_str}"
                );

                if let Some(config) = &config_option {
                    info!("stop_all: Found config {} with local_address: {:?} and auto_loopback_address: {}",
                          config_id_str, config.local_address, config.auto_loopback_address);
                    if let Some(local_addr) = &config.local_address && kftray_hosts::loopback::is_custom_loopback_address(local_addr) {
                        info!(
                            "Cleaning up loopback address for config {config_id_str}: {local_addr}"
                        );

                        release_address_with_fallback(local_addr).await;
                    }
                }

                entry.process.cleanup_and_abort().await;

                CustomResponse {
                    id: Some(config_id_parsed),
                    service: service_id,
                    namespace: String::new(),
                    local_port: 0,
                    remote_port: 0,
                    context: String::new(),
                    protocol: String::new(),
                    stdout: String::from("Service port forwarding has been stopped"),
                    stderr: String::new(),
                    status: 0,
                }
            }
        })
        .collect();

    while let Some(response) = abort_handles.next().await {
        responses.push(response);
    }

    let cluster_cleanup_tasks: FuturesUnordered<_> = configs
        .iter()
        .filter(|config| running_configs_state.contains(&config.id.unwrap_or_default()))
        .filter(|config| {
            config.protocol == "udp" || matches!(config.workload_type.as_deref(), Some("proxy"))
        })
        .map(|config| {
            let config_id = config.id.unwrap_or_default();
            let namespace = config.namespace.clone();
            let client_key =
                ServiceClientKey::new(config.context.clone(), config.kubeconfig.clone());
            async move {
                match PORT_FORWARD_REGISTRY.acquire_client(client_key).await {
                    Ok(shared_client) => {
                        let client = kube::Client::clone(&shared_client);
                        delete_proxy_cluster_resources(client, &namespace, config_id).await;
                    }
                    Err(e) => error!(
                        "Failed to get K8s client for cluster cleanup of config {config_id}: {e}"
                    ),
                }
            }
        })
        .collect();

    cluster_cleanup_tasks.collect::<Vec<_>>().await;

    // Cancel all recovery managers
    for entry in crate::kube::proxy_recovery::RECOVERY_MANAGERS.iter() {
        entry.value().cancel();
    }
    crate::kube::proxy_recovery::RECOVERY_MANAGERS.clear();
    crate::kube::proxy_recovery::RECOVERY_LOCKS.clear();
    info!("Cancelled and cleared all recovery managers");

    let address_cleanup_tasks: FuturesUnordered<_> = configs
        .iter()
        .filter(|config| running_configs_state.contains(&config.id.unwrap_or_default()))
        .filter_map(|config| {
            if let Some(local_addr) = &config.local_address {
                if kftray_hosts::loopback::is_custom_loopback_address(local_addr) {
                    Some(async move {
                        info!(
                            "Releasing loopback address for config {}: {} (auto_allocated: {})",
                            config.id.unwrap_or_default(),
                            local_addr,
                            config.auto_loopback_address
                        );
                        release_address_with_fallback(local_addr).await;
                    })
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    address_cleanup_tasks.collect::<Vec<_>>().await;

    let update_config_tasks: FuturesUnordered<_> = configs
        .iter()
        .map(|config| {
            let config_id_parsed = config.id.unwrap_or_default();
            async move {
                let config_state = ConfigState::new(config_id_parsed, false);
                if let Err(e) = update_config_state_with_mode(&config_state, mode).await {
                    error!("Failed to update config state: {e}");
                } else {
                    info!("Successfully updated config state for config_id: {config_id_parsed}");
                }
            }
        })
        .collect();

    update_config_tasks.collect::<Vec<_>>().await;

    if let Err(e) = remove_all_host_entries() {
        error!("Failed to clean up all host entries: {e}");
    }

    info!(
        "Port forward stopping process completed with {} responses",
        responses.len()
    );

    Ok(responses)
}

#[cfg(test)]
mod tests {
    use kftray_commons::models::config_model::Config;

    fn create_test_config() -> Config {
        Config {
            id: Some(1),
            context: Some("test-context".to_string()),
            kubeconfig: Some("test-kubeconfig".to_string()),
            namespace: "test-namespace".to_string(),
            service: Some("test-service".to_string()),
            alias: Some("test-alias".to_string()),
            local_port: Some(8080),
            remote_port: Some(8080),
            protocol: "tcp".to_string(),
            workload_type: Some("service".to_string()),
            target: None,
            local_address: Some("127.0.0.1".to_string()),
            auto_loopback_address: false,
            remote_address: None,
            domain_enabled: Some(true),
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

    fn create_udp_config() -> Config {
        let mut config = create_test_config();
        config.id = Some(2);
        config.protocol = "udp".to_string();
        config
    }

    fn create_proxy_config() -> Config {
        let mut config = create_test_config();
        config.id = Some(3);
        config.workload_type = Some("proxy".to_string());
        config
    }

    #[tokio::test]
    async fn test_running_configs_filter() {
        let configs = [
            create_test_config(),
            create_udp_config(),
            create_proxy_config(),
        ];

        let running_configs_state = [1, 2, 3];

        let filtered_configs: Vec<&Config> = configs
            .iter()
            .filter(|config| running_configs_state.contains(&config.id.unwrap_or_default()))
            .filter(|config| {
                config.protocol == "udp" || matches!(config.workload_type.as_deref(), Some("proxy"))
            })
            .collect();

        assert_eq!(filtered_configs.len(), 2);
        assert_eq!(filtered_configs[0].id, Some(2));
        assert_eq!(filtered_configs[1].id, Some(3));
    }

    #[tokio::test]
    async fn test_extract_config_with_id() {
        use std::collections::HashMap;

        let configs = [
            create_test_config(),
            create_udp_config(),
            create_proxy_config(),
        ];

        let config_map: HashMap<i64, &Config> = configs
            .iter()
            .filter_map(|c| c.id.map(|id| (id, c)))
            .collect();

        assert_eq!(config_map.len(), 3);
        assert!(config_map.contains_key(&1));
        assert!(config_map.contains_key(&2));
        assert!(config_map.contains_key(&3));

        let config1 = config_map.get(&1).unwrap();
        assert_eq!(config1.protocol, "tcp");

        let config2 = config_map.get(&2).unwrap();
        assert_eq!(config2.protocol, "udp");

        let config3 = config_map.get(&3).unwrap();
        assert_eq!(config3.workload_type, Some("proxy".to_string()));
    }
}

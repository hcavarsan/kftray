use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::stream::{
    FuturesUnordered,
    StreamExt,
};
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::Pod;
use kftray_commons::config_model::Config;
use kftray_commons::config_state::get_configs_state;
use kftray_commons::{
    config::get_configs,
    models::{
        config_state_model::ConfigState,
        response::CustomResponse,
    },
    utils::{
        config::read_configs_with_mode,
        config_state::update_config_state_with_mode,
        db_mode::DatabaseMode,
        timeout_manager::cancel_timeout_for_forward,
    },
};
use kube::Client;
use kube::api::{
    Api,
    DeleteParams,
    ListParams,
};
use tokio::task::spawn_blocking;
use tokio::time::timeout;
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
#[cfg(test)]
use crate::port_forward::PortForwardProcess;
use crate::port_forward_error::PortForwardError;
use crate::registry::PORT_FORWARD_REGISTRY;
#[cfg(test)]
use crate::registry::PortForwardKey;

/// Load configs from the database, logging errors and falling back to empty
/// vec.
async fn load_configs(mode: DatabaseMode) -> Vec<Config> {
    let result = match mode {
        DatabaseMode::File => get_configs().await,
        DatabaseMode::Memory => read_configs_with_mode(mode).await,
    };
    match result {
        Ok(v) => v,
        Err(e) => {
            warn!("Failed to read configs ({mode:?}): {e}");
            vec![]
        }
    }
}

/// Synchronous helper function to release address via helper service.
/// Must be called from spawn_blocking to avoid blocking the tokio runtime.
fn try_release_address_sync(address: &str) -> Result<(), PortForwardError> {
    let app_id = "com.kftray.app".to_string();

    let socket_path = kftray_helper::communication::get_default_socket_path()
        .map_err(|e| PortForwardError::AddressAllocation(e.to_string()))?;

    if !kftray_helper::client::socket_comm::is_socket_available(&socket_path) {
        return Err(PortForwardError::AddressAllocation(
            "Helper service is not available".to_string(),
        ));
    }

    let command = kftray_helper::messages::RequestCommand::Address(
        kftray_helper::messages::AddressCommand::Release {
            address: address.to_string(),
        },
    );

    match kftray_helper::client::socket_comm::send_request(&socket_path, &app_id, command) {
        Ok(response) => match response.result {
            kftray_helper::messages::RequestResult::Success => Ok(()),
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

/// Release address with timeout. Skips osascript fallback to avoid blocking on
/// user interaction. Address cleanup is not critical - addresses will be freed
/// on system restart.
async fn release_address_with_fallback(address: &str) {
    const ADDRESS_RELEASE_TIMEOUT: Duration = Duration::from_secs(3);

    let address_owned = address.to_string();

    // Wrap blocking helper service call in spawn_blocking with timeout
    let result = timeout(ADDRESS_RELEASE_TIMEOUT, async {
        let addr = address_owned.clone();
        spawn_blocking(move || try_release_address_sync(&addr)).await
    })
    .await;

    match result {
        Ok(Ok(Ok(_))) => {
            info!("Successfully released address via helper: {}", address);
        }
        Ok(Ok(Err(e))) => {
            // Helper service returned an error - skip fallback (osascript blocks for user
            // input)
            warn!(
                "Failed to release address {} via helper: {}. Skipping fallback to avoid blocking.",
                address, e
            );
        }
        Ok(Err(e)) => {
            // spawn_blocking panicked
            warn!(
                "Address release task panicked for {}: {}. Skipping.",
                address, e
            );
        }
        Err(_) => {
            // Timeout elapsed
            warn!(
                "Address release timed out for {} after {:?}. Skipping.",
                address, ADDRESS_RELEASE_TIMEOUT
            );
        }
    }
}

pub(crate) async fn delete_proxy_cluster_resources(
    client: Client, namespace: &str, config_id: i64,
) {
    let username = whoami::username().unwrap_or_else(|_| "unknown".to_string());
    let pod_prefix = format!("kftray-forward-{username}");
    let lp = ListParams::default().labels(&format!("config_id={config_id}"));

    // Delete pods
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    match pods.list(&lp).await {
        Ok(pod_list) => {
            for pod in pod_list.items {
                if let Some(pod_name) = pod.metadata.name
                    && pod_name.starts_with(&pod_prefix)
                {
                    let dp = DeleteParams {
                        grace_period_seconds: Some(0),
                        ..DeleteParams::default()
                    };
                    match pods.delete(&pod_name, &dp).await {
                        Ok(_) => info!("Deleted proxy pod: {pod_name}"),
                        Err(e) => warn!("Failed to delete proxy pod {pod_name}: {e}"),
                    }
                }
            }
        }
        Err(e) => warn!("Failed to list pods for cleanup (config_id={config_id}): {e}"),
    }

    let deployments: Api<Deployment> = Api::namespaced(client, namespace);
    match deployments.list(&lp).await {
        Ok(dep_list) => {
            for dep in dep_list.items {
                if let Some(dep_name) = dep.metadata.name
                    && dep_name.starts_with(&pod_prefix)
                {
                    let dp = DeleteParams {
                        grace_period_seconds: Some(0),
                        ..DeleteParams::default()
                    };
                    match deployments.delete(&dep_name, &dp).await {
                        Ok(_) => info!("Deleted proxy deployment: {dep_name}"),
                        Err(e) => warn!("Failed to delete proxy deployment {dep_name}: {e}"),
                    }
                }
            }
        }
        Err(e) => warn!("Failed to list deployments for cleanup (config_id={config_id}): {e}"),
    }
}

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

    let config_map: HashMap<i64, &Config> = configs
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
                        let client = Client::clone(&shared_client);
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

    if let Some(config) = configs.iter().find(|c| c.id == Some(config_id_parsed))
        && config.workload_type.as_deref() == Some("expose")
    {
        return crate::expose::stop_expose(config_id_parsed, &config.namespace, mode).await;
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
                        let client = Client::clone(&shared_client);
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
    use std::collections::HashMap;

    use kftray_commons::models::config_model::Config;

    use super::*;

    async fn create_dummy_handle() -> PortForwardProcess {
        let handle = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            Ok(())
        });
        PortForwardProcess::new(handle, "test-config".to_string())
    }

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

use std::collections::HashMap;
use std::time::Duration;

use futures::stream::{
    FuturesUnordered,
    StreamExt,
};
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

use crate::hostsfile::{
    remove_all_host_entries,
    remove_host_entry,
    remove_ssl_host_entry,
};
use crate::kube::shared_client::{
    SHARED_CLIENT_MANAGER,
    ServiceClientKey,
};
use crate::port_forward::CHILD_PROCESSES;
#[cfg(test)]
use crate::port_forward::PortForwardProcess;

/// Synchronous helper function to release address via helper service.
/// Must be called from spawn_blocking to avoid blocking the tokio runtime.
fn try_release_address_sync(address: &str) -> Result<(), String> {
    let app_id = "com.kftray.app".to_string();

    let socket_path =
        kftray_helper::communication::get_default_socket_path().map_err(|e| e.to_string())?;

    if !kftray_helper::client::socket_comm::is_socket_available(&socket_path) {
        return Err("Helper service is not available".to_string());
    }

    let command = kftray_helper::messages::RequestCommand::Address(
        kftray_helper::messages::AddressCommand::Release {
            address: address.to_string(),
        },
    );

    match kftray_helper::client::socket_comm::send_request(&socket_path, &app_id, command) {
        Ok(response) => match response.result {
            kftray_helper::messages::RequestResult::Success => Ok(()),
            kftray_helper::messages::RequestResult::Error(error) => Err(error),
            _ => Err("Unexpected response format".to_string()),
        },
        Err(e) => Err(e.to_string()),
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

pub async fn stop_all_port_forward() -> Result<Vec<CustomResponse>, String> {
    stop_all_port_forward_with_mode(DatabaseMode::File).await
}

pub async fn stop_all_port_forward_with_mode(
    mode: DatabaseMode,
) -> Result<Vec<CustomResponse>, String> {
    crate::ssl::ensure_crypto_provider_installed();
    info!("Attempting to stop all port forwards in mode: {mode:?}");

    let mut responses = Vec::with_capacity(1024);

    let handle_keys: Vec<String> = CHILD_PROCESSES
        .iter()
        .map(|entry| entry.key().clone())
        .collect();

    if handle_keys.is_empty() {
        debug!("No port forwarding processes to stop");
    }

    for composite_key in &handle_keys {
        if let Some(config_id_str) = composite_key
            .strip_prefix("config:")
            .and_then(|s| s.split(":service:").next())
            && let Ok(config_id) = config_id_str.parse::<i64>()
        {
            cancel_timeout_for_forward(config_id).await;
        }
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
            return Err(error_message);
        }
    };

    let configs = match mode {
        DatabaseMode::File => get_configs().await.unwrap_or_default(),
        DatabaseMode::Memory => read_configs_with_mode(mode).await.unwrap_or_default(),
    };

    let config_map: HashMap<i64, &Config> = configs
        .iter()
        .filter_map(|c| c.id.map(|id| (id, c)))
        .collect();

    let empty_str = String::new();

    let mut abort_handles: FuturesUnordered<_> = handle_keys
        .into_iter()
        .map(|composite_key| {
            let empty_str_clone = empty_str.clone();
            let config_map_cloned = config_map.clone();

            async move {
                let (config_id_str, service_id) = if let Some(content) = composite_key.strip_prefix("config:") {
                    if let Some((config_part, service_part)) = content.split_once(":service:") {
                        (config_part, service_part.to_string())
                    } else {
                        error!("Invalid composite key format encountered: {composite_key}");
                        return CustomResponse {
                            id: None,
                            service: empty_str_clone.clone(),
                            namespace: empty_str_clone.clone(),
                            local_port: 0,
                            remote_port: 0,
                            context: empty_str_clone.clone(),
                            protocol: empty_str_clone.clone(),
                            stdout: empty_str_clone.clone(),
                            stderr: String::from("Invalid composite key format"),
                            status: 1,
                        };
                    }
                } else {
                    error!("Invalid composite key format encountered: {composite_key}");
                    return CustomResponse {
                        id: None,
                        service: empty_str_clone.clone(),
                        namespace: empty_str_clone.clone(),
                        local_port: 0,
                        remote_port: 0,
                        context: empty_str_clone.clone(),
                        protocol: empty_str_clone.clone(),
                        stdout: empty_str_clone.clone(),
                        stderr: String::from("Invalid composite key format"),
                        status: 1,
                    };
                };
                let config_id_parsed = config_id_str.parse::<i64>().unwrap_or_default();
                let config_option = config_map_cloned.get(&config_id_parsed).cloned();

                if let Some(config) = config_option
                    && config.domain_enabled.unwrap_or_default()
                {
                    if let Err(e) = remove_host_entry(config_id_str) {
                        error!(
                            "Failed to remove host entry for ID {config_id_str}: {e}"
                        );
                    }


                    if let Err(e) = remove_ssl_host_entry(config_id_str) {
                        error!(
                            "Failed to remove SSL host entry for ID {config_id_str}: {e}"
                        );
                    }
                } else {
                    warn!("Config with id '{config_id_str}' not found.");
                }

                info!(
                    "Aborting port forwarding task for config_id: {config_id_str}"
                );

                if let Some(config) = config_map_cloned.get(&config_id_parsed).cloned() {
                    info!("stop_all: Found config {} with local_address: {:?} and auto_loopback_address: {}",
                          config_id_str, config.local_address, config.auto_loopback_address);
                    if let Some(local_addr) = &config.local_address && crate::network_utils::is_custom_loopback_address(local_addr) {
                        info!(
                            "Cleaning up loopback address for config {config_id_str}: {local_addr}"
                        );

                        release_address_with_fallback(local_addr).await;
                    }
                }

                if let Some((_, process)) = CHILD_PROCESSES.remove(&composite_key) {
                    process.cleanup_and_abort().await;
                }

                CustomResponse {
                    id: Some(config_id_parsed),
                    service: service_id,
                    namespace: empty_str_clone.clone(),
                    local_port: 0,
                    remote_port: 0,
                    context: empty_str_clone.clone(),
                    protocol: empty_str_clone.clone(),
                    stdout: String::from("Service port forwarding has been stopped"),
                    stderr: empty_str_clone,
                    status: 0,
                }
            }
        })
        .collect();

    while let Some(response) = abort_handles.next().await {
        responses.push(response);
    }

    let pod_deletion_tasks: FuturesUnordered<_> = configs
        .iter()
        .filter(|config| running_configs_state.contains(&config.id.unwrap_or_default()))
        .filter(|config| {
            config.protocol == "udp" || matches!(config.workload_type.as_deref(), Some("proxy"))
        })
        .filter_map(|config| {
            config
                .kubeconfig
                .as_ref()
                .map(|kubeconfig| (config, kubeconfig))
        })
        .map(|(config, kubeconfig)| {
            let config_id_str = config.id.unwrap_or_default();
            async move {
                let client_key =
                    ServiceClientKey::new(config.context.clone(), Some(kubeconfig.clone()));

                match SHARED_CLIENT_MANAGER.get_client(client_key).await {
                    Ok(shared_client) => {
                        let client = Client::clone(&shared_client);
                        let pods: Api<Pod> = Api::all(client.clone());
                        let lp =
                            ListParams::default().labels(&format!("config_id={config_id_str}"));

                        match pods.list(&lp).await {
                            Ok(pod_list) => {
                                let username =
                                    whoami::username().unwrap_or_else(|_| "unknown".to_string());
                                let pod_prefix = format!("kftray-forward-{username}");
                                let delete_tasks: FuturesUnordered<_> = pod_list
                                    .items
                                    .into_iter()
                                    .filter_map(|pod| {
                                        if let Some(pod_name) = pod.metadata.name
                                            && pod_name.starts_with(&pod_prefix)
                                        {
                                            let namespace = pod
                                                .metadata
                                                .namespace
                                                .unwrap_or_else(|| "default".to_string());
                                            let pods_in_namespace: Api<Pod> =
                                                Api::namespaced(client.clone(), &namespace);
                                            let dp = DeleteParams {
                                                grace_period_seconds: Some(0),
                                                ..DeleteParams::default()
                                            };

                                            return Some(async move {
                                                match pods_in_namespace.delete(&pod_name, &dp).await
                                                {
                                                    Ok(_) => info!(
                                                        "Successfully deleted pod: {pod_name}"
                                                    ),
                                                    Err(e) => error!(
                                                        "Failed to delete pod {pod_name}: {e}"
                                                    ),
                                                }
                                            });
                                        }
                                        None
                                    })
                                    .collect();

                                delete_tasks.collect::<Vec<_>>().await;
                            }
                            _ => {
                                error!("Error listing pods for config_id {config_id_str}");
                            }
                        }
                    }
                    Err(e) => error!("Failed to get shared Kubernetes client: {e}"),
                }
            }
        })
        .collect();

    pod_deletion_tasks.collect::<Vec<_>>().await;

    let address_cleanup_tasks: FuturesUnordered<_> = configs
        .iter()
        .filter(|config| running_configs_state.contains(&config.id.unwrap_or_default()))
        .filter_map(|config| {
            if let Some(local_addr) = &config.local_address {
                if crate::network_utils::is_custom_loopback_address(local_addr) {
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

pub async fn stop_port_forward(config_id: String) -> Result<CustomResponse, String> {
    stop_port_forward_with_mode(config_id, DatabaseMode::File).await
}

pub async fn stop_port_forward_with_mode(
    config_id: String, mode: DatabaseMode,
) -> Result<CustomResponse, String> {
    let config_id_parsed = config_id
        .parse::<i64>()
        .map_err(|_| "Invalid config ID".to_string())?;

    let configs = match mode {
        DatabaseMode::File => get_configs().await.unwrap_or_default(),
        DatabaseMode::Memory => read_configs_with_mode(mode).await.unwrap_or_default(),
    };

    if let Some(config) = configs.iter().find(|c| c.id == Some(config_id_parsed))
        && config.workload_type.as_deref() == Some("expose")
    {
        return crate::expose::stop_expose(config_id_parsed, &config.namespace, mode).await;
    }

    let composite_key = CHILD_PROCESSES
        .iter()
        .find(|entry| {
            entry
                .key()
                .starts_with(&format!("config:{config_id}:service:"))
        })
        .map(|entry| entry.key().clone());

    if let Some(composite_key) = composite_key {
        let config_id_parsed = config_id.parse::<i64>().unwrap_or_default();

        let configs = match mode {
            DatabaseMode::File => get_configs().await.unwrap_or_default(),
            DatabaseMode::Memory => read_configs_with_mode(mode).await.unwrap_or_default(),
        };

        if let Some(config) = configs.iter().find(|c| c.id == Some(config_id_parsed)) {
            info!(
                "Found config {} during stop with local_address: {:?} and auto_loopback_address: {}",
                config_id, config.local_address, config.auto_loopback_address
            );
            if let Some(local_addr) = &config.local_address
                && crate::network_utils::is_custom_loopback_address(local_addr)
            {
                info!(
                    "Cleaning up loopback address for config {config_id}: {local_addr} (auto_allocated: {})",
                    config.auto_loopback_address
                );
                release_address_with_fallback(local_addr).await;
            }
        }

        if let Some((_, process)) = CHILD_PROCESSES.remove(&composite_key) {
            process.cleanup_and_abort().await;
        }

        let config_id_parsed = config_id.parse::<i64>().unwrap_or_default();
        cancel_timeout_for_forward(config_id_parsed).await;

        let service_name = composite_key
            .strip_prefix("config:")
            .and_then(|s| s.split_once(":service:"))
            .map(|(_, service)| service)
            .unwrap_or("");

        if let Some(config) = configs.iter().find(|c| c.id == Some(config_id_parsed))
            && config.domain_enabled.unwrap_or_default()
        {
            if let Err(e) = remove_host_entry(&config_id) {
                error!("Failed to remove host entry for ID {config_id}: {e}");

                let config_state = ConfigState::new(config_id_parsed, false);
                if let Err(e) = update_config_state_with_mode(&config_state, mode).await {
                    error!("Failed to update config state: {e}");
                }
                return Err(e.to_string());
            }

            if let Err(e) = remove_ssl_host_entry(&config_id) {
                error!("Failed to remove SSL host entry for ID {config_id}: {e}");
            }
        } else {
            warn!("Config with id '{config_id}' not found.");
        }

        let config_state = ConfigState::new(config_id_parsed, false);
        if let Err(e) = update_config_state_with_mode(&config_state, mode).await {
            error!("Failed to update config state: {e}");
        }

        if let Some(config) = configs.iter().find(|c| c.id == Some(config_id_parsed)) {
            let client_key =
                ServiceClientKey::new(config.context.clone(), config.kubeconfig.clone());
            SHARED_CLIENT_MANAGER.invalidate_client(&client_key);
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

        let configs = match mode {
            DatabaseMode::File => get_configs().await.unwrap_or_default(),
            DatabaseMode::Memory => read_configs_with_mode(mode).await.unwrap_or_default(),
        };

        let config = configs.iter().find(|c| c.id == Some(config_id_parsed));

        if config.is_none() {
            return Err(format!(
                "No port forwarding process found for config_id '{config_id}'"
            ));
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
                .contains("No port forwarding process found")
        );
    }

    #[tokio::test]
    async fn test_stop_port_forward_with_handle() {
        let dummy_handle = create_dummy_handle().await;
        let key = "config:201:service:test-service".to_string();

        println!("Before cleanup, found {} processes:", CHILD_PROCESSES.len());
        for entry in CHILD_PROCESSES.iter() {
            println!("  Existing process: {}", entry.key());
        }

        for entry in CHILD_PROCESSES.iter() {
            entry.value().abort();
        }
        CHILD_PROCESSES.clear();

        CHILD_PROCESSES.insert(key.clone(), dummy_handle);
        assert_eq!(CHILD_PROCESSES.len(), 1, "Process should be added");
        assert!(
            CHILD_PROCESSES.contains_key(&key),
            "Process should be present"
        );

        if let Some((_, process)) = CHILD_PROCESSES.remove(&key) {
            process.abort();
        }

        assert!(
            CHILD_PROCESSES.is_empty(),
            "Process handle should be removed. Found {} processes",
            CHILD_PROCESSES.len()
        );
    }

    #[tokio::test]
    async fn test_stop_port_forward_with_multiple_handles() {
        use std::time::{
            SystemTime,
            UNIX_EPOCH,
        };

        let unique_suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        let dummy_handle1 = create_dummy_handle().await;
        let dummy_handle2 = create_dummy_handle().await;
        let dummy_handle3 = create_dummy_handle().await;

        let key1 = format!("config:test_multi_101_{}:service:service1", unique_suffix);
        let key2 = format!("config:test_multi_102_{}:service:service2", unique_suffix);
        let key3 = format!("config:test_multi_103_{}:service:service3", unique_suffix);

        // Insert test processes (no global clear - other tests may be running)
        CHILD_PROCESSES.insert(key1.clone(), dummy_handle1);
        CHILD_PROCESSES.insert(key2.clone(), dummy_handle2);
        CHILD_PROCESSES.insert(key3.clone(), dummy_handle3);

        // Verify all 3 keys we inserted exist
        assert!(
            CHILD_PROCESSES.contains_key(&key1),
            "Should contain key1 after insert"
        );
        assert!(
            CHILD_PROCESSES.contains_key(&key2),
            "Should contain key2 after insert"
        );
        assert!(
            CHILD_PROCESSES.contains_key(&key3),
            "Should contain key3 after insert"
        );

        // Remove key2
        if let Some((_, process)) = CHILD_PROCESSES.remove(&key2) {
            process.abort();
        } else {
            panic!("key2 should have been found for removal");
        }

        assert!(
            CHILD_PROCESSES.contains_key(&key1),
            "Should still contain key1: {}",
            key1
        );
        assert!(
            !CHILD_PROCESSES.contains_key(&key2),
            "Should not contain key2 after removal: {}",
            key2
        );
        assert!(
            CHILD_PROCESSES.contains_key(&key3),
            "Should still contain key3: {}",
            key3
        );

        if let Some((_, p)) = CHILD_PROCESSES.remove(&key1) {
            p.abort();
        }
        if let Some((_, p)) = CHILD_PROCESSES.remove(&key3) {
            p.abort();
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

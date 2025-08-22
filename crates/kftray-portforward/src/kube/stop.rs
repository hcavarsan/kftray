use std::collections::HashMap;

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
use kube::api::{
    Api,
    DeleteParams,
    ListParams,
};
use kube::Client;
use tracing::{
    debug,
    error,
    info,
    warn,
};

use crate::hostsfile::{
    remove_all_host_entries,
    remove_host_entry,
};
use crate::kube::shared_client::{
    ServiceClientKey,
    SHARED_CLIENT_MANAGER,
};
#[cfg(test)]
use crate::port_forward::PortForwardProcess;
use crate::port_forward::{
    CHILD_PROCESSES,
    PROCESS_MANAGEMENT_LOCK,
};

async fn try_release_address(address: &str) -> Result<(), String> {
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

async fn try_fallback_release_address(address: &str) {
    match crate::network_utils::remove_loopback_address(address).await {
        Ok(_) => {
            debug!(
                "Successfully removed loopback address via fallback: {}",
                address
            );
        }
        Err(e) => {
            let error_msg = e.to_string();
            if error_msg.contains("cancelled") || error_msg.contains("canceled") {
                warn!(
                    "User cancelled loopback address removal for {}, but continuing stop process",
                    address
                );
            } else {
                warn!(
                    "Failed to remove loopback address {} via fallback: {}",
                    address, error_msg
                );
            }
        }
    }
}

async fn release_address_with_fallback(address: &str) {
    match try_release_address(address).await {
        Ok(_) => {
            info!("Successfully released address via helper: {}", address);
        }
        Err(e) => {
            warn!(
                "Failed to release address {} via helper: {}. Trying fallback",
                address, e
            );
            try_fallback_release_address(address).await;
        }
    }
}

pub async fn stop_all_port_forward() -> Result<Vec<CustomResponse>, String> {
    stop_all_port_forward_with_mode(DatabaseMode::File).await
}

pub async fn stop_all_port_forward_with_mode(
    mode: DatabaseMode,
) -> Result<Vec<CustomResponse>, String> {
    info!("Attempting to stop all port forwards in mode: {mode:?}");

    let mut responses = Vec::with_capacity(1024);

    let handle_keys: Vec<String> = {
        let _global_lock = PROCESS_MANAGEMENT_LOCK.lock().await;
        let processes = CHILD_PROCESSES.lock().await;
        if processes.is_empty() {
            debug!("No port forwarding processes to stop");
            return Ok(Vec::new());
        }
        processes.keys().cloned().collect()
    };

    for composite_key in &handle_keys {
        if let Some(config_id_str) = composite_key
            .strip_prefix("config:")
            .and_then(|s| s.split(":service:").next())
        {
            if let Ok(config_id) = config_id_str.parse::<i64>() {
                cancel_timeout_for_forward(config_id).await;
            }
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

    // Get configs using the appropriate database mode
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

                if let Some(config) = config_option {
                    if config.domain_enabled.unwrap_or_default() {
                        if let Err(e) = remove_host_entry(config_id_str) {
                            error!(
                                "Failed to remove host entry for ID {config_id_str}: {e}"
                            );
                        }
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
                    if let Some(local_addr) = &config.local_address {
                        if crate::network_utils::is_custom_loopback_address(local_addr) {
                            info!(
                                "Cleaning up loopback address for config {config_id_str}: {local_addr}"
                            );

                            release_address_with_fallback(local_addr).await;
                        }
                    }
                }

                let process = {
                    let _global_lock = PROCESS_MANAGEMENT_LOCK.lock().await;
                    CHILD_PROCESSES.lock().await.remove(&composite_key)
                };

                if let Some(process) = process {
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
                let client_key = ServiceClientKey::new(
                    config.context.clone(),
                    Some(kubeconfig.clone()),
                    config_id_str,
                );

                match SHARED_CLIENT_MANAGER.get_client(client_key).await {
                    Ok(shared_client) => {
                        let client = Client::clone(&shared_client);
                        let pods: Api<Pod> = Api::all(client.clone());
                        let lp =
                            ListParams::default().labels(&format!("config_id={config_id_str}"));

                        if let Ok(pod_list) = pods.list(&lp).await {
                            let username = whoami::username();
                            let pod_prefix = format!("kftray-forward-{username}");
                            let delete_tasks: FuturesUnordered<_> = pod_list
                                .items
                                .into_iter()
                                .filter_map(|pod| {
                                    if let Some(pod_name) = pod.metadata.name {
                                        if pod_name.starts_with(&pod_prefix) {
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
                                    }
                                    None
                                })
                                .collect();

                            delete_tasks.collect::<Vec<_>>().await;
                        } else {
                            error!("Error listing pods for config_id {config_id_str}");
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
    let composite_key = {
        let _global_lock = PROCESS_MANAGEMENT_LOCK.lock().await;
        let child_processes = CHILD_PROCESSES.lock().await;
        child_processes
            .keys()
            .find(|key| key.starts_with(&format!("config:{config_id}:service:")))
            .map(|key| key.to_string())
    };

    if let Some(composite_key) = composite_key {
        let config_id_parsed = config_id.parse::<i64>().unwrap_or_default();

        // Get configs using the appropriate database mode
        let configs = match mode {
            DatabaseMode::File => get_configs().await.unwrap_or_default(),
            DatabaseMode::Memory => read_configs_with_mode(mode).await.unwrap_or_default(),
        };

        // Handle loopback address cleanup
        if let Some(config) = configs.iter().find(|c| c.id == Some(config_id_parsed)) {
            info!("Found config {} during stop with local_address: {:?} and auto_loopback_address: {}",
                  config_id, config.local_address, config.auto_loopback_address);
            if let Some(local_addr) = &config.local_address {
                if crate::network_utils::is_custom_loopback_address(local_addr) {
                    info!("Cleaning up loopback address for config {config_id}: {local_addr} (auto_allocated: {})", config.auto_loopback_address);
                    release_address_with_fallback(local_addr).await;
                }
            }
        }

        let port_forward_process = {
            let _global_lock = PROCESS_MANAGEMENT_LOCK.lock().await;
            let mut child_processes = CHILD_PROCESSES.lock().await;
            child_processes.remove(&composite_key)
        };

        if let Some(process) = port_forward_process {
            process.cleanup_and_abort().await;
        }

        let config_id_parsed = config_id.parse::<i64>().unwrap_or_default();
        cancel_timeout_for_forward(config_id_parsed).await;

        let service_name = composite_key
            .strip_prefix("config:")
            .and_then(|s| s.split_once(":service:"))
            .map(|(_, service)| service)
            .unwrap_or("");

        // Handle host entry cleanup for domain-enabled configs
        if let Some(config) = configs.iter().find(|c| c.id == Some(config_id_parsed)) {
            if config.domain_enabled.unwrap_or_default() {
                if let Err(e) = remove_host_entry(&config_id) {
                    error!("Failed to remove host entry for ID {config_id}: {e}");

                    let config_state = ConfigState::new(config_id_parsed, false);
                    if let Err(e) = update_config_state_with_mode(&config_state, mode).await {
                        error!("Failed to update config state: {e}");
                    }
                    return Err(e.to_string());
                }
            }
        } else {
            warn!("Config with id '{config_id}' not found.");
        }

        let config_state = ConfigState::new(config_id_parsed, false);
        if let Err(e) = update_config_state_with_mode(&config_state, mode).await {
            error!("Failed to update config state: {e}");
        }

        // Invalidate the client for this specific config
        if let Some(config) = configs.iter().find(|c| c.id == Some(config_id_parsed)) {
            let client_key = ServiceClientKey::new(
                config.context.clone(),
                config.kubeconfig.clone(),
                config_id_parsed,
            );
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

        // Check if config exists first
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
        assert!(result
            .unwrap_err()
            .contains("No port forwarding process found"));
    }

    #[tokio::test]
    async fn test_stop_all_port_forward_empty() {
        {
            let mut processes = CHILD_PROCESSES.lock().await;
            processes.clear();
            assert!(processes.is_empty());
        }

        let result = stop_all_port_forward().await;

        match result {
            Ok(responses) => {
                assert!(
                    responses.is_empty(),
                    "Expected empty responses for empty process list"
                );
            }
            Err(e) => {
                panic!("Expected Ok result but got error: {e}");
            }
        }
    }

    #[tokio::test]
    async fn test_stop_port_forward_with_handle() {
        let dummy_handle = create_dummy_handle().await;

        {
            let mut processes = CHILD_PROCESSES.lock().await;
            processes.clear();
            processes.insert("1_test-service".to_string(), dummy_handle);
        }
        {
            let mut processes = CHILD_PROCESSES.lock().await;
            if let Some(process) = processes.remove("1_test-service") {
                process.abort();
            }
        }

        let processes = CHILD_PROCESSES.lock().await;
        assert!(processes.is_empty(), "Process handle should be removed");
    }

    #[tokio::test]
    async fn test_stop_port_forward_with_multiple_handles() {
        let dummy_handle1 = create_dummy_handle().await;
        let dummy_handle2 = create_dummy_handle().await;
        let dummy_handle3 = create_dummy_handle().await;

        {
            let mut processes = CHILD_PROCESSES.lock().await;
            processes.clear();
            processes.insert("1_service1".to_string(), dummy_handle1);
            processes.insert("2_service2".to_string(), dummy_handle2);
            processes.insert("3_service3".to_string(), dummy_handle3);
            assert_eq!(processes.len(), 3);
        }
        {
            let mut processes = CHILD_PROCESSES.lock().await;
            if let Some(process) = processes.remove("2_service2") {
                process.abort();
            }
        }

        let processes = CHILD_PROCESSES.lock().await;
        assert_eq!(
            processes.len(),
            2,
            "Only the specified process should be removed"
        );
        assert!(processes.contains_key("1_service1"));
        assert!(!processes.contains_key("2_service2"));
        assert!(processes.contains_key("3_service3"));
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
        let configs = vec![
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
        let configs = vec![
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

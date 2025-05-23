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
    utils::config_state::update_config_state,
};
use kube::api::{
    Api,
    DeleteParams,
    ListParams,
};
use log::{
    debug,
    error,
    info,
    warn,
};
use tokio::task::JoinHandle;

use crate::create_client_with_specific_context;
use crate::hostsfile::{
    remove_all_host_entries,
    remove_host_entry,
};
use crate::port_forward::{
    CANCEL_NOTIFIER,
    CHILD_PROCESSES,
};

pub async fn stop_all_port_forward() -> Result<Vec<CustomResponse>, String> {
    info!("Attempting to stop all port forwards");

    let mut responses = Vec::with_capacity(1024);
    CANCEL_NOTIFIER.notify_waiters();

    let handle_map: HashMap<String, JoinHandle<()>> = {
        let mut processes = CHILD_PROCESSES.lock().unwrap();
        if processes.is_empty() {
            debug!("No port forwarding processes to stop");
            return Ok(Vec::new());
        }
        processes.drain().collect()
    };

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

    let configs = match get_configs().await {
        Ok(configs) => configs,
        Err(e) => {
            let error_message = format!("Failed to retrieve configs: {e}");
            error!("{error_message}");
            return Err(error_message);
        }
    };

    let config_map: HashMap<i64, &Config> = configs
        .iter()
        .filter_map(|c| c.id.map(|id| (id, c)))
        .collect();

    let empty_str = String::new();

    let mut abort_handles: FuturesUnordered<_> = handle_map
        .iter()
        .map(|(composite_key, handle)| {
            let ids: Vec<&str> = composite_key.split('_').collect();
            let empty_str_clone = empty_str.clone();
            let config_map_cloned = config_map.clone();

            async move {
                if ids.len() != 2 {
                    error!(
                        "Invalid composite key format encountered: {composite_key}"
                    );
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

                let config_id_str = ids[0];
                let service_id = ids[1].to_string();
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
                    if let Some(local_addr) = &config.local_address {
                        if local_addr != "127.0.0.1" {
                            info!(
                                "Cleaning up loopback address for config {config_id_str}: {local_addr}"
                            );

                            if let Err(e) =
                                crate::network_utils::remove_loopback_address(local_addr).await
                            {
                                warn!("Failed to remove loopback address {local_addr}: {e}");
                            }
                        }
                    }
                }

                handle.abort();

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
                match create_client_with_specific_context(
                    Some(kubeconfig.clone()),
                    Some(&config.context),
                )
                .await
                {
                    Ok((Some(client), _, _)) => {
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
                    Ok((None, _, _)) => {
                        error!("Client not created for kubeconfig: {kubeconfig:?}")
                    }
                    Err(e) => error!("Failed to create Kubernetes client: {e}"),
                }
            }
        })
        .collect();

    pod_deletion_tasks.collect::<Vec<_>>().await;

    let update_config_tasks: FuturesUnordered<_> = configs
        .iter()
        .map(|config| {
            let config_id_parsed = config.id.unwrap_or_default();
            async move {
                let config_state = ConfigState {
                    id: None,
                    config_id: config_id_parsed,
                    is_running: false,
                };
                if let Err(e) = update_config_state(&config_state).await {
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
    let cancellation_notifier = CANCEL_NOTIFIER.clone();
    cancellation_notifier.notify_waiters();

    let composite_key = {
        let child_processes = CHILD_PROCESSES.lock().unwrap();
        child_processes
            .keys()
            .find(|key| key.starts_with(&format!("{config_id}_")))
            .map(|key| key.to_string())
    };

    if let Some(composite_key) = composite_key {
        let config_id_parsed = config_id.parse::<i64>().unwrap_or_default();

        if let Ok(configs) = get_configs().await {
            if let Some(config) = configs.iter().find(|c| c.id == Some(config_id_parsed)) {
                if let Some(local_addr) = &config.local_address {
                    if local_addr != "127.0.0.1" {
                        info!("Cleaning up loopback address for config {config_id}: {local_addr}");

                        if let Err(e) =
                            crate::network_utils::remove_loopback_address(local_addr).await
                        {
                            warn!("Failed to remove loopback address {local_addr}: {e}");
                        }
                    }
                }
            }
        }

        let join_handle = {
            let mut child_processes = CHILD_PROCESSES.lock().unwrap();
            debug!("child_processes: {child_processes:?}");
            child_processes.remove(&composite_key)
        };

        if let Some(join_handle) = join_handle {
            debug!("Join handle: {join_handle:?}");
            join_handle.abort();
        }

        let (config_id_str, service_name) = composite_key.split_once('_').unwrap_or(("", ""));
        let config_id_parsed = config_id_str.parse::<i64>().unwrap_or_default();

        match get_configs().await {
            Ok(configs) => {
                if let Some(config) = configs.iter().find(|c| c.id == Some(config_id_parsed)) {
                    if config.domain_enabled.unwrap_or_default() {
                        if let Err(e) = remove_host_entry(config_id_str) {
                            error!("Failed to remove host entry for ID {config_id_str}: {e}");

                            let config_state = ConfigState {
                                id: None,
                                config_id: config_id_parsed,
                                is_running: false,
                            };
                            if let Err(e) = update_config_state(&config_state).await {
                                error!("Failed to update config state: {e}");
                            }
                            return Err(e.to_string());
                        }
                    }
                } else {
                    warn!("Config with id '{config_id_str}' not found.");
                }

                let config_state = ConfigState {
                    id: None,
                    config_id: config_id_parsed,
                    is_running: false,
                };
                if let Err(e) = update_config_state(&config_state).await {
                    error!("Failed to update config state: {e}");
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
            }
            Err(e) => {
                let config_id_parsed = config_id.parse::<i64>().unwrap_or_default();
                let config_state = ConfigState {
                    id: None,
                    config_id: config_id_parsed,
                    is_running: false,
                };
                if let Err(e) = update_config_state(&config_state).await {
                    error!("Failed to update config state: {e}");
                }
                Err(format!("Failed to retrieve configs: {e}"))
            }
        }
    } else {
        let config_id_parsed = config_id.parse::<i64>().unwrap_or_default();
        let config_state = ConfigState {
            id: None,
            config_id: config_id_parsed,
            is_running: false,
        };
        if let Err(e) = update_config_state(&config_state).await {
            error!("Failed to update config state: {e}");
        }
        Err(format!(
            "No port forwarding process found for config_id '{config_id}'"
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use kftray_commons::models::config_model::Config;

    use super::*;

    async fn create_dummy_handle() -> JoinHandle<()> {
        tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        })
    }

    fn create_test_config() -> Config {
        Config {
            id: Some(1),
            context: "test-context".to_string(),
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
            remote_address: None,
            domain_enabled: Some(true),
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
            let mut processes = CHILD_PROCESSES.lock().unwrap();
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
            let mut processes = CHILD_PROCESSES.lock().unwrap();
            processes.clear();
            processes.insert("1_test-service".to_string(), dummy_handle);
        }
        {
            let mut processes = CHILD_PROCESSES.lock().unwrap();
            if let Some(handle) = processes.remove("1_test-service") {
                handle.abort();
            }
        }

        let processes = CHILD_PROCESSES.lock().unwrap();
        assert!(processes.is_empty(), "Process handle should be removed");
    }

    #[tokio::test]
    async fn test_stop_port_forward_with_multiple_handles() {
        let dummy_handle1 = create_dummy_handle().await;
        let dummy_handle2 = create_dummy_handle().await;
        let dummy_handle3 = create_dummy_handle().await;

        {
            let mut processes = CHILD_PROCESSES.lock().unwrap();
            processes.clear();
            processes.insert("1_service1".to_string(), dummy_handle1);
            processes.insert("2_service2".to_string(), dummy_handle2);
            processes.insert("3_service3".to_string(), dummy_handle3);
            assert_eq!(processes.len(), 3);
        }
        {
            let mut processes = CHILD_PROCESSES.lock().unwrap();
            if let Some(handle) = processes.remove("2_service2") {
                handle.abort();
            }
        }

        let processes = CHILD_PROCESSES.lock().unwrap();
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

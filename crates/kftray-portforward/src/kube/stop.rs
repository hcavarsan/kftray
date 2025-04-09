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
        processes.drain().collect()
    };

    let running_configs_state = match get_configs_state().await {
        Ok(states) => states
            .into_iter()
            .filter(|s| s.is_running)
            .map(|s| s.config_id)
            .collect::<Vec<i64>>(),
        Err(e) => {
            let error_message = format!("Failed to retrieve config states: {}", e);
            error!("{}", error_message);
            return Err(error_message);
        }
    };

    let configs = match get_configs().await {
        Ok(configs) => configs,
        Err(e) => {
            let error_message = format!("Failed to retrieve configs: {}", e);
            error!("{}", error_message);
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
                        "Invalid composite key format encountered: {}",
                        composite_key
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
                                "Failed to remove host entry for ID {}: {}",
                                config_id_str, e
                            );
                        }
                    }
                } else {
                    warn!("Config with id '{}' not found.", config_id_str);
                }

                info!(
                    "Aborting port forwarding task for config_id: {}",
                    config_id_str
                );
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
                            ListParams::default().labels(&format!("config_id={}", config_id_str));

                        if let Ok(pod_list) = pods.list(&lp).await {
                            let username = whoami::username();
                            let pod_prefix = format!("kftray-forward-{}", username);
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
                                                        "Successfully deleted pod: {}",
                                                        pod_name
                                                    ),
                                                    Err(e) => error!(
                                                        "Failed to delete pod {}: {}",
                                                        pod_name, e
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
                            error!("Error listing pods for config_id {}", config_id_str);
                        }
                    }
                    Ok((None, _, _)) => {
                        error!("Client not created for kubeconfig: {:?}", kubeconfig)
                    }
                    Err(e) => error!("Failed to create Kubernetes client: {}", e),
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
                    error!("Failed to update config state: {}", e);
                } else {
                    info!(
                        "Successfully updated config state for config_id: {}",
                        config_id_parsed
                    );
                }
            }
        })
        .collect();

    update_config_tasks.collect::<Vec<_>>().await;

    if let Err(e) = remove_all_host_entries() {
        error!("Failed to clean up all host entries: {}", e);
    }

    // Clean up loopback addresses for all configs
    let loopback_cleanup_tasks: FuturesUnordered<_> = configs
        .iter()
        .filter_map(|config| config.local_address.as_ref())
        .filter(|local_addr| crate::network_utils::is_loopback_address(local_addr) && *local_addr != "127.0.0.1")
        .map(|local_addr| {
            let local_addr = local_addr.clone();
            async move {
                match crate::network_utils::remove_loopback_address(&local_addr).await {
                    Ok(_) => debug!("Successfully removed loopback address: {}", local_addr),
                    Err(e) => warn!("Failed to remove loopback address {}: {}", local_addr, e),
                }
            }
        })
        .collect();
    
    loopback_cleanup_tasks.collect::<Vec<_>>().await;

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
            .find(|key| key.starts_with(&format!("{}_", config_id)))
            .map(|key| key.to_string())
    };

    if let Some(composite_key) = composite_key {
        let join_handle = {
            let mut child_processes = CHILD_PROCESSES.lock().unwrap();
            debug!("child_processes: {:?}", child_processes);
            child_processes.remove(&composite_key)
        };

        if let Some(join_handle) = join_handle {
            debug!("Join handle: {:?}", join_handle);
            join_handle.abort();
        }

        let (config_id_str, service_name) = composite_key.split_once('_').unwrap_or(("", ""));
        let config_id_parsed = config_id_str.parse::<i64>().unwrap_or_default();

        match get_configs().await {
            Ok(configs) => {
                if let Some(config) = configs.iter().find(|c| c.id == Some(config_id_parsed)) {
                    if config.domain_enabled.unwrap_or_default() {
                        if let Err(e) = remove_host_entry(config_id_str) {
                            error!(
                                "Failed to remove host entry for ID {}: {}",
                                config_id_str, e
                            );

                            let config_state = ConfigState {
                                id: None,
                                config_id: config_id_parsed,
                                is_running: false,
                            };
                            if let Err(e) = update_config_state(&config_state).await {
                                error!("Failed to update config state: {}", e);
                            }
                            return Err(e.to_string());
                        }
                    }
                    
                    // Clean up loopback addresses if needed
                    if let Some(local_addr) = &config.local_address {
                        if crate::network_utils::is_loopback_address(local_addr) && local_addr != "127.0.0.1" {
                            match crate::network_utils::remove_loopback_address(local_addr).await {
                                Ok(_) => debug!("Successfully removed loopback address: {}", local_addr),
                                Err(e) => warn!("Failed to remove loopback address {}: {}", local_addr, e),
                            }
                        }
                    }
                } else {
                    warn!("Config with id '{}' not found.", config_id_str);
                }

                let config_state = ConfigState {
                    id: None,
                    config_id: config_id_parsed,
                    is_running: false,
                };
                if let Err(e) = update_config_state(&config_state).await {
                    error!("Failed to update config state: {}", e);
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
                    error!("Failed to update config state: {}", e);
                }
                Err(format!("Failed to retrieve configs: {}", e))
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
            error!("Failed to update config state: {}", e);
        }
        Err(format!(
            "No port forwarding process found for config_id '{}'",
            config_id
        ))
    }
}

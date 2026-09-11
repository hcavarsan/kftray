use std::collections::{
    HashMap,
    HashSet,
};
use std::sync::Arc;
use std::time::Duration;

use futures::stream::{
    self,
    StreamExt,
};
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::Pod;
use kftray_commons::config_model::Config;
use kftray_commons::{
    config::get_config_with_mode,
    models::{
        config_state_model::ConfigState,
        response::CustomResponse,
    },
    utils::{
        config::read_configs_with_mode,
        config_state::{
            get_configs_state_with_mode,
            update_config_state_with_mode,
        },
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
    info,
    warn,
};

use crate::hostsfile::{
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

pub(crate) async fn delete_proxy_cluster_resources(
    client: Client, namespace: &str, config_id: i64,
) -> Result<(), String> {
    let username: String = whoami::username()
        .unwrap_or_else(|_| "unknown".to_string())
        .to_lowercase()
        .chars()
        .filter(|character| character.is_alphanumeric())
        .collect();
    let prefix = format!("kftray-forward-{username}-");
    let lp = ListParams::default().labels(&format!("config_id={config_id}"));
    let dp = DeleteParams {
        grace_period_seconds: Some(0),
        ..DeleteParams::default()
    };
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let deployments: Api<Deployment> = Api::namespaced(client, namespace);
    let delete_pods = async {
        let mut errors = Vec::new();
        let list = pods.list(&lp).await.map_err(|error| error.to_string())?;
        for pod in list.items {
            if let Some(name) = pod.metadata.name
                && name.starts_with(&prefix)
                && let Err(error) = pods.delete(&name, &dp).await
                && !matches!(&error, kube::Error::Api(response) if response.code == 404)
            {
                errors.push(error.to_string());
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    };
    let delete_deployments = async {
        let mut errors = Vec::new();
        let list = deployments
            .list(&lp)
            .await
            .map_err(|error| error.to_string())?;
        for deployment in list.items {
            if let Some(name) = deployment.metadata.name
                && name.starts_with(&prefix)
                && let Err(error) = deployments.delete(&name, &dp).await
                && !matches!(&error, kube::Error::Api(response) if response.code == 404)
            {
                errors.push(error.to_string());
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    };
    let (pods, deployments) = tokio::join!(delete_pods, delete_deployments);
    let errors: Vec<_> = [pods, deployments]
        .into_iter()
        .filter_map(Result::err)
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

pub async fn stop_all_port_forward() -> Result<Vec<CustomResponse>, String> {
    stop_all_port_forward_with_mode(DatabaseMode::File).await
}

pub async fn stop_all_port_forward_with_mode(
    mode: DatabaseMode,
) -> Result<Vec<CustomResponse>, String> {
    let mut ids: HashSet<i64> = CHILD_PROCESSES.iter().map(|entry| *entry.key()).collect();
    for entry in crate::kube::proxy_recovery::RECOVERY_MANAGERS.iter() {
        entry.value().cancel();
        ids.insert(*entry.key());
    }
    for entry in CHILD_PROCESSES.iter() {
        entry.value().cancel();
    }
    for entry in crate::kube::proxy_recovery::RECOVERY_LOCKS.iter() {
        if Arc::strong_count(entry.value()) > 1 {
            ids.insert(*entry.key());
        }
    }

    let configs_result = read_configs_with_mode(mode).await;
    let states_result = get_configs_state_with_mode(mode).await;
    if let Ok(states) = &states_result {
        ids.extend(
            states
                .iter()
                .filter(|state| state.is_running)
                .map(|state| state.config_id),
        );
    }
    let configs: HashMap<_, _> = configs_result
        .as_ref()
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|config| config.id.map(|id| (id, config)))
        .collect();
    let responses = stream::iter(ids)
        .map(|id| {
            let config = configs.get(&id).copied();
            async move {
                match stop_config(id, config, mode).await {
                    Ok(response) => response,
                    Err(error) => stop_response(id, config, Some(error)),
                }
            }
        })
        .buffer_unordered(16)
        .collect()
        .await;
    configs_result?;
    states_result?;
    Ok(responses)
}

pub async fn stop_port_forward(config_id: String) -> Result<CustomResponse, String> {
    stop_port_forward_with_mode(config_id, DatabaseMode::File).await
}

pub async fn stop_port_forward_with_mode(
    config_id: String, mode: DatabaseMode,
) -> Result<CustomResponse, String> {
    let id = config_id
        .parse::<i64>()
        .map_err(|_| "Invalid config ID".to_string())?;
    if let Some(manager) = crate::kube::proxy_recovery::RECOVERY_MANAGERS.get(&id) {
        manager.cancel();
    }
    if let Some(process) = CHILD_PROCESSES.get(&id) {
        process.cancel();
    }
    let config = get_config_with_mode(id, mode).await;
    let response = stop_config(id, config.as_ref().ok(), mode).await;
    if let Ok(config) = &config
        && config.context.is_some()
    {
        SHARED_CLIENT_MANAGER.invalidate_client(&ServiceClientKey::new(
            config.context.clone(),
            config.kubeconfig.clone(),
        ));
    }
    match (config, response) {
        (Err(error), Err(_)) => Err(error),
        (_, response) => response,
    }
}

async fn stop_config(
    id: i64, config: Option<&Config>, mode: DatabaseMode,
) -> Result<CustomResponse, String> {
    if let Some(manager) = crate::kube::proxy_recovery::RECOVERY_MANAGERS.get(&id) {
        manager.cancel();
    }
    let lock = crate::kube::proxy_recovery::acquire_recovery_lock(id).await;
    let (guard, was_starting) = match lock.try_lock() {
        Ok(guard) => (guard, false),
        Err(_) => (lock.lock().await, true),
    };
    let refreshed = if was_starting {
        get_config_with_mode(id, mode).await.ok()
    } else {
        None
    };
    let config = refreshed.as_ref().or(config);
    if let Some((_, manager)) = crate::kube::proxy_recovery::RECOVERY_MANAGERS.remove(&id) {
        manager.cancel();
    }
    let process = CHILD_PROCESSES.remove(&id);
    let existed = process.is_some();
    if let Some((_, process)) = process {
        process.cleanup_and_abort().await;
    }
    cancel_timeout_for_forward(id).await;

    let result = if let Some(config) = config {
        let cluster_cleanup = async {
            if config.workload_type.as_deref() == Some("expose")
                || config.workload_type.as_deref() == Some("proxy")
                || config.protocol == "udp"
            {
                let key = ServiceClientKey::new(config.context.clone(), config.kubeconfig.clone());
                let connection = SHARED_CLIENT_MANAGER
                    .get_connection(key)
                    .await
                    .map_err(|error| error.to_string())?;
                if config.workload_type.as_deref() == Some("expose") {
                    crate::expose::kubernetes::delete_expose_resources(
                        connection.client.clone(),
                        &config.namespace,
                        &id.to_string(),
                    )
                    .await
                } else {
                    delete_proxy_cluster_resources(connection.client.clone(), &config.namespace, id)
                        .await
                }
            } else {
                Ok(())
            }
        };
        let local_cleanup = async {
            if let Some(address) = &config.local_address
                && crate::network_utils::is_custom_loopback_address(address)
            {
                release_address_with_fallback(address).await;
            }
            let mut errors = Vec::new();
            if config.domain_enabled.unwrap_or_default() {
                if let Err(error) = remove_host_entry(&id.to_string()) {
                    errors.push(error.to_string());
                }
                if let Err(error) = remove_ssl_host_entry(&id.to_string()) {
                    errors.push(error.to_string());
                }
            }
            if errors.is_empty() {
                Ok(())
            } else {
                Err(errors.join("; "))
            }
        };
        let state = ConfigState::new(id, false);
        let (cluster, local, state) = tokio::join!(
            cluster_cleanup,
            local_cleanup,
            update_config_state_with_mode(&state, mode)
        );
        let errors: Vec<_> = [cluster, local, state]
            .into_iter()
            .filter_map(Result::err)
            .collect();
        if errors.is_empty() {
            Ok(stop_response(id, Some(config), None))
        } else {
            Err(errors.join("; "))
        }
    } else if existed {
        Ok(stop_response(id, None, None))
    } else {
        Err(format!(
            "No port forwarding process found for config_id '{id}'"
        ))
    };
    drop(guard);
    drop(lock);
    crate::kube::proxy_recovery::remove_recovery_lock(id);
    result
}

fn stop_response(id: i64, config: Option<&Config>, error: Option<String>) -> CustomResponse {
    CustomResponse {
        id: Some(id),
        service: config
            .and_then(|config| config.service.clone())
            .unwrap_or_default(),
        namespace: config
            .map(|config| config.namespace.clone())
            .unwrap_or_default(),
        local_port: config
            .and_then(|config| config.local_port)
            .unwrap_or_default(),
        remote_port: config
            .and_then(|config| config.remote_port)
            .unwrap_or_default(),
        context: config
            .and_then(|config| config.context.clone())
            .unwrap_or_default(),
        protocol: config
            .map(|config| config.protocol.clone())
            .unwrap_or_default(),
        stdout: if error.is_none() {
            "Port forwarding has been stopped".to_string()
        } else {
            String::new()
        },
        status: i32::from(error.is_some()),
        stderr: error.unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use tokio::net::{
        TcpListener,
        UdpSocket,
    };

    use super::*;

    #[tokio::test]
    async fn stop_releases_listener_and_websocket_owner_before_returning() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let id = 410_011;
        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let tcp_address = tcp.local_addr().unwrap();
        let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let udp_address = udp.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let _tcp = tcp;
            std::future::pending::<anyhow::Result<()>>().await
        });
        let websocket = tokio::spawn(async move {
            let _udp = udp;
            std::future::pending::<()>().await;
        });
        let mut process = PortForwardProcess::new(task, id.to_string());
        process.set_ws_client_handle(websocket);
        CHILD_PROCESSES.insert(id, process);

        stop_port_forward_with_mode(id.to_string(), DatabaseMode::Memory)
            .await
            .unwrap();

        let _tcp = TcpListener::bind(tcp_address).await.unwrap();
        let _udp = UdpSocket::bind(udp_address).await.unwrap();
    }

    #[tokio::test]
    async fn stop_all_releases_every_transport_in_memory_mode() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let mut addresses = Vec::new();
        for id in [410_021, 410_022] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            addresses.push(listener.local_addr().unwrap());
            let task = tokio::spawn(async move {
                let _listener = listener;
                std::future::pending::<anyhow::Result<()>>().await
            });
            CHILD_PROCESSES.insert(id, PortForwardProcess::new(task, id.to_string()));
        }
        let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let udp_address = udp.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let _udp = udp;
            std::future::pending::<anyhow::Result<()>>().await
        });
        CHILD_PROCESSES.insert(410_023, PortForwardProcess::new(task, "410023".to_string()));

        let responses = stop_all_port_forward_with_mode(DatabaseMode::Memory)
            .await
            .unwrap();
        for id in [410_021, 410_022, 410_023] {
            assert!(
                responses
                    .iter()
                    .any(|response| response.id == Some(id) && response.status == 0)
            );
        }
        for address in addresses {
            let _listener = TcpListener::bind(address).await.unwrap();
        }
        let _udp = UdpSocket::bind(udp_address).await.unwrap();
    }
}

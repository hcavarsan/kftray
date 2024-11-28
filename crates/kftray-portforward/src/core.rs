use std::collections::HashMap;
use std::sync::Arc;

use hostsfile::HostsBuilder;
use k8s_openapi::api::core::v1::Pod;
use k8s_openapi::api::core::v1::Service;
use kftray_commons::{
    models::{
        config_model::Config,
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
    error,
    info,
    warn,
};
use rand::{
    distributions::Alphanumeric,
    Rng,
};
use serde::de::DeserializeOwned;
use serde_json::json;
use tokio::task::JoinHandle;

use crate::kubernetes::ResourceManager;
use crate::{
    models::kube::{
        HttpLogState,
        Port,
        PortForward,
        Target,
        TargetSelector,
    },
    port_forward::{
        CANCEL_NOTIFIER,
        CHILD_PROCESSES,
    },
};

pub async fn start_port_forward(
    configs: Vec<Config>, protocol: &str, http_log_state: Arc<HttpLogState>,
) -> Result<Vec<CustomResponse>, String> {
    let mut responses = Vec::new();
    let mut errors = Vec::new();
    let mut child_handles = Vec::new();

    for config in configs {
        match forward_single_config(&config, protocol, http_log_state.clone()).await {
            Ok((response, handle_key)) => {
                responses.push(response);
                child_handles.push(handle_key);
            }
            Err(e) => errors.push(e),
        }
    }

    if !errors.is_empty() {
        cleanup_handles(&child_handles).await;
        return Err(errors.join("\n"));
    }

    Ok(responses)
}

async fn forward_single_config(
    config: &Config, protocol: &str, http_log_state: Arc<HttpLogState>,
) -> Result<(CustomResponse, String), String> {
    let target = create_target_from_config(config);
    let port_forward = create_port_forward(config, target).await?;

    let (actual_local_port, handle) = match protocol {
        "udp" => port_forward.port_forward_udp().await,
        "tcp" => port_forward.port_forward_tcp(http_log_state).await,
        _ => return Err("Unsupported protocol".into()),
    }
    .map_err(|e| e.to_string())?;

    let handle_key = format!(
        "{}_{}",
        config.id.unwrap(),
        config.service.clone().unwrap_or_default()
    );
    CHILD_PROCESSES
        .lock()
        .unwrap()
        .insert(handle_key.clone(), handle);

    if config.domain_enabled.unwrap_or_default() {
        setup_hosts_file(config)?;
    }

    update_config_state(&ConfigState {
        id: None,
        config_id: config.id.unwrap(),
        is_running: true,
    })
    .await
    .map_err(|e| e.to_string())?;

    Ok((
        create_response(config, actual_local_port, protocol),
        handle_key,
    ))
}

fn create_target_from_config(config: &Config) -> Target {
    let selector = match config.workload_type.as_deref() {
        Some("pod") => TargetSelector::PodLabel(config.target.clone().unwrap_or_default()),
        Some("proxy") => TargetSelector::PodLabel(format!(
            "app=kftray-server,config_id={}",
            config.id.unwrap()
        )),
        _ => TargetSelector::ServiceName(config.service.clone().unwrap_or_default()),
    };

    let remote_port = config.remote_port.unwrap_or(0);
    let port = Port::Number(remote_port as i32);

    Target::new(
        selector,
        port,
        config.namespace.clone(),
        config.remote_address.clone(),
    )
}

async fn create_port_forward(config: &Config, target: Target) -> Result<PortForward, String> {
    PortForward::new(
        target,
        config.local_port,
        config.local_address.clone(),
        Some(config.context.clone()),
        config.kubeconfig.clone(),
        config.id.unwrap_or_default(),
        config.workload_type.clone().unwrap_or_default(),
    )
    .await
    .map_err(|e| e.to_string())
}

fn setup_hosts_file(config: &Config) -> Result<(), String> {
    if let (Some(service_name), Some(local_address)) = (&config.service, &config.local_address) {
        let hostfile_comment = format!(
            "kftray custom host for {} - {}",
            service_name,
            config.id.unwrap_or_default()
        );

        let mut hosts_builder = HostsBuilder::new(hostfile_comment);

        match local_address.parse() {
            Ok(ip_addr) => {
                hosts_builder.add_hostname(ip_addr, config.alias.clone().unwrap_or_default());
                hosts_builder.write().map_err(|e| {
                    error!("Failed to write to hostfile: {}", e);
                    e.to_string()
                })?;
            }
            Err(_) => {
                warn!("Invalid IP address format: {}", local_address);
            }
        }
    }
    Ok(())
}

fn create_response(config: &Config, actual_local_port: u16, protocol: &str) -> CustomResponse {
    CustomResponse {
        id: config.id,
        service: config.service.clone().unwrap_or_default(),
        namespace: config.namespace.clone(),
        local_port: actual_local_port,
        remote_port: config.remote_port.unwrap_or_default(),
        context: config.context.clone(),
        protocol: config.protocol.clone(),
        stdout: format!(
            "{} forwarding from 127.0.0.1:{} -> {:?}:{}",
            protocol.to_uppercase(),
            actual_local_port,
            config.remote_port.unwrap_or_default(),
            config.service.clone().unwrap_or_default()
        ),
        stderr: String::new(),
        status: 0,
    }
}

async fn cleanup_handles(handles: &[String]) {
    for handle_key in handles {
        if let Some(handle) = CHILD_PROCESSES.lock().unwrap().remove(handle_key) {
            handle.abort();
        }
    }
}

pub async fn stop_all_port_forward() -> Result<Vec<CustomResponse>, String> {
    info!("Stopping all port forwards");
    CANCEL_NOTIFIER.notify_waiters();

    // Get active configs first
    let (configs, running_configs) = get_active_configs().await?;
    if running_configs.is_empty() {
        info!("No active configs found, nothing to stop");
        return Ok(Vec::new());
    }

    // Stop port forwards first
    let handle_map = {
        let mut processes = CHILD_PROCESSES.lock().unwrap();
        processes
            .drain()
            .collect::<HashMap<String, JoinHandle<()>>>()
    };

    let config_map: HashMap<i64, &Config> = configs
        .iter()
        .filter_map(|c| c.id.map(|id| (id, c)))
        .collect();

    // Get proxy configs that need cleanup
    let proxy_configs: Vec<(&Config, i64)> = configs
        .iter()
        .filter_map(|config| {
            let config_id = config.id?;
            if running_configs.contains(&config_id)
                && config.workload_type.as_deref() == Some("proxy")
            {
                Some((config, config_id))
            } else {
                None
            }
        })
        .collect();

    // Execute all cleanups concurrently
    let cleanup_futures = proxy_configs.into_iter().map(|(config, config_id)| {
        let namespace = config.namespace.clone();
        let service = config.service.clone().unwrap_or_default();
        async move { stop_proxy_forward(config_id, &namespace, service).await }
    });

    let (port_forward_responses, proxy_results) = tokio::join!(
        stop_port_forwards(handle_map, &config_map),
        futures::future::join_all(cleanup_futures)
    );

    // Collect any errors from proxy cleanups
    let proxy_errors: Vec<String> = proxy_results
        .into_iter()
        .filter_map(|result| result.err())
        .collect();

    if !proxy_errors.is_empty() {
        warn!("Errors during proxy cleanup: {}", proxy_errors.join("; "));
    }

    // Final cleanup of config states
    cleanup_resources(&configs, &running_configs).await?;

    Ok(port_forward_responses)
}

// Helper function to stop proxy forwards
pub async fn stop_proxy_forward(
    config_id: i64, namespace: &str, service_name: String,
) -> Result<CustomResponse, String> {
    info!(
        "Stopping proxy forward for config_id: {} in namespace: {}",
        config_id, namespace
    );

    let config = kftray_commons::config::get_config(config_id)
        .await
        .map_err(|e| format!("Failed to get config {}: {}", config_id, e))?;

    let (client_opt, _, _) = crate::client::create_client_with_specific_context(
        config.kubeconfig.clone(),
        Some(&config.context),
    )
    .await
    .map_err(|e| format!("Failed to create client: {}", e))?;

    let client = client_opt.ok_or_else(|| "Failed to create kubernetes client".to_string())?;

    // Delete resources in parallel
    let (pod_result, svc_result) = tokio::join!(
        delete_resources::<Pod>(client.clone(), namespace, config_id),
        delete_resources::<Service>(client, namespace, config_id)
    );

    // Log any errors but don't fail the operation
    if let Err(e) = pod_result {
        warn!("Error deleting pods: {}", e);
    }
    if let Err(e) = svc_result {
        warn!("Error deleting services: {}", e);
    }

    Ok(CustomResponse {
        id: Some(config_id),
        service: service_name,
        namespace: namespace.to_string(),
        local_port: 0,
        remote_port: 0,
        context: String::new(),
        protocol: String::new(),
        stdout: "Proxy resources cleanup completed".to_string(),
        stderr: String::new(),
        status: 0,
    })
}

// Generic function to delete kubernetes resources
async fn delete_resources<K>(
    client: kube::Client, namespace: &str, config_id: i64,
) -> Result<(), String>
where
    K: kube::api::Resource<Scope = kube::core::NamespaceResourceScope>
        + Clone
        + DeserializeOwned
        + std::fmt::Debug,
    K::DynamicType: Default,
{
    let api: Api<K> = Api::namespaced(client, namespace);
    let label_selector = format!("app=kftray-server,config_id={}", config_id);
    let list_params = ListParams::default().labels(&label_selector);

    let resources = api
        .list(&list_params)
        .await
        .map_err(|e| format!("Failed to list resources: {}", e))?;

    for resource in resources.items {
        if let Some(ref name) = resource.meta().name {
            if let Err(e) = api.delete(name, &DeleteParams::default()).await {
                if !e.to_string().contains("not found") {
                    return Err(format!("Failed to delete resource {}: {}", name, e));
                }
            }
        }
    }

    Ok(())
}

async fn get_active_configs() -> Result<(Vec<Config>, Vec<i64>), String> {
    let configs = kftray_commons::utils::config::get_configs()
        .await
        .map_err(|e| format!("Failed to retrieve configs: {}", e))?;

    let running_configs = kftray_commons::config_state::get_configs_state()
        .await
        .map_err(|e| format!("Failed to retrieve config states: {}", e))?
        .into_iter()
        .filter(|s| s.is_running)
        .map(|s| s.config_id)
        .collect();

    Ok((configs, running_configs))
}

async fn stop_port_forwards(
    handle_map: HashMap<String, JoinHandle<()>>, config_map: &HashMap<i64, &Config>,
) -> Vec<CustomResponse> {
    let mut responses = Vec::new();

    for (composite_key, handle) in handle_map {
        if let Some(response) = stop_single_forward(&composite_key, handle, config_map).await {
            responses.push(response);
        }
    }

    responses
}

async fn stop_single_forward(
    composite_key: &str, handle: JoinHandle<()>, config_map: &HashMap<i64, &Config>,
) -> Option<CustomResponse> {
    let parts: Vec<&str> = composite_key.split('_').collect();
    if parts.len() != 2 {
        error!("Invalid composite key format: {}", composite_key);
        return None;
    }

    let (config_id_str, service_id) = (parts[0], parts[1]);
    let config_id = config_id_str.parse::<i64>().ok()?;

    if let Some(config) = config_map.get(&config_id) {
        cleanup_host_entry(config, service_id, config_id_str);
    }

    handle.abort();

    Some(CustomResponse {
        id: Some(config_id),
        service: service_id.to_string(),
        namespace: String::new(),
        local_port: 0,
        remote_port: 0,
        context: String::new(),
        protocol: String::new(),
        stdout: "Service port forwarding has been stopped".to_string(),
        stderr: String::new(),
        status: 0,
    })
}

fn cleanup_host_entry(config: &Config, service_id: &str, config_id_str: &str) {
    if config.domain_enabled.unwrap_or_default() {
        let hostfile_comment = format!("kftray custom host for {} - {}", service_id, config_id_str);
        if let Err(e) = HostsBuilder::new(hostfile_comment).write() {
            error!("Failed to clean up hostfile entry: {}", e);
        }
    }
}

async fn cleanup_resources(configs: &[Config], running_configs: &[i64]) -> Result<(), String> {
    for config in configs {
        if running_configs.contains(&config.id.unwrap_or_default()) {
            let config_state = ConfigState {
                id: None,
                config_id: config.id.unwrap_or_default(),
                is_running: false,
            };
            if let Err(e) = update_config_state(&config_state).await {
                error!("Failed to update config state: {}", e);
            }
        }
    }
    Ok(())
}

pub async fn stop_port_forward(config_id: String) -> Result<CustomResponse, String> {
    CANCEL_NOTIFIER.notify_waiters();

    let handle_key = find_handle_key(&config_id)?;
    let join_handle = remove_handle(&handle_key)?;

    let (config_id_str, service_name) = parse_handle_key(&handle_key)?;
    let config_id_parsed = config_id_str.parse::<i64>().unwrap_or_default();

    cleanup_config(config_id_parsed, &service_name).await?;
    join_handle.abort();

    Ok(CustomResponse {
        id: None,
        service: service_name,
        namespace: String::new(),
        local_port: 0,
        remote_port: 0,
        context: String::new(),
        protocol: String::new(),
        stdout: "Service port forwarding has been stopped".to_string(),
        stderr: String::new(),
        status: 0,
    })
}

fn find_handle_key(config_id: &str) -> Result<String, String> {
    let child_processes = CHILD_PROCESSES.lock().unwrap();
    child_processes
        .keys()
        .find(|key| key.starts_with(&format!("{}_", config_id)))
        .map(|key| key.to_string())
        .ok_or_else(|| {
            format!(
                "No port forwarding process found for config_id '{}'",
                config_id
            )
        })
}

fn remove_handle(handle_key: &str) -> Result<JoinHandle<()>, String> {
    let mut child_processes = CHILD_PROCESSES.lock().unwrap();
    child_processes
        .remove(handle_key)
        .ok_or_else(|| "Failed to remove handle".to_string())
}

fn parse_handle_key(handle_key: &str) -> Result<(String, String), String> {
    handle_key
        .split_once('_')
        .map(|(id, service)| (id.to_string(), service.to_string()))
        .ok_or_else(|| "Invalid handle key format".to_string())
}

async fn cleanup_config(config_id: i64, service_name: &str) -> Result<(), String> {
    if let Ok(configs) = kftray_commons::config::get_configs().await {
        if let Some(config) = configs.iter().find(|c| c.id == Some(config_id)) {
            if config.domain_enabled.unwrap_or_default() {
                cleanup_host_entry(config, service_name, &config_id.to_string());
            }
        }
    }

    let config_state = ConfigState {
        id: None,
        config_id,
        is_running: false,
    };
    update_config_state(&config_state)
        .await
        .map_err(|e| e.to_string())
}

// Add these missing functions
pub async fn retrieve_service_configs(
    context: &str, kubeconfig: Option<String>,
) -> Result<Vec<Config>, String> {
    let (client_opt, _, _) =
        crate::client::create_client_with_specific_context(kubeconfig.clone(), Some(context))
            .await
            .map_err(|e| e.to_string())?;

    let client = client_opt.ok_or_else(|| "Client not created".to_string())?;
    let annotation = "kftray.app/configs";

    let namespaces = crate::client::list_all_namespaces(client.clone())
        .await
        .map_err(|e| e.to_string())?;

    let mut all_configs = Vec::new();

    for namespace in namespaces {
        let services =
            crate::client::get_services_with_annotation(client.clone(), &namespace, annotation)
                .await
                .map_err(|e| e.to_string())?;

        for (service_name, annotations, ports) in services {
            if let Some(configs_str) = annotations.get(annotation) {
                all_configs.extend(parse_service_configs(
                    configs_str,
                    context,
                    &namespace,
                    &service_name,
                    &ports,
                    kubeconfig.clone(),
                ));
            } else {
                all_configs.extend(create_default_service_configs(
                    context,
                    &namespace,
                    &service_name,
                    &ports,
                    kubeconfig.clone(),
                ));
            }
        }
    }

    Ok(all_configs)
}

// In core.rs, update the pod name generation:

// In core.rs, update the values creation:

pub async fn deploy_and_forward_pod(
    configs: Vec<Config>, http_log_state: Arc<HttpLogState>,
) -> Result<Vec<CustomResponse>, String> {
    let mut responses = Vec::new();

    for config in configs {
        let pod_name = format!(
            "pkf-{}-{}-{}",
            whoami::username()
                .to_lowercase()
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                .collect::<String>(),
            config.protocol.to_lowercase(),
            rand::thread_rng()
                .sample_iter(Alphanumeric)
                .take(6)
                .map(char::from)
                .map(|c| c.to_ascii_lowercase())
                .collect::<String>()
        );

        let (client, _, _) = crate::client::create_client_with_specific_context(
            config.kubeconfig.clone(),
            Some(&config.context),
        )
        .await
        .map_err(|e| e.to_string())?;

        let client = client.ok_or_else(|| "Client not created".to_string())?;

        let resource_manager = ResourceManager::new(client.clone(), config.namespace.clone())
            .await
            .map_err(|e| e.to_string())?;

        // Create manifest values with proper types
        let mut values = serde_json::Map::new();
        values.insert("hashed_name".to_string(), json!(pod_name));
        values.insert("namespace".to_string(), json!(config.namespace));
        values.insert(
            "config_id".to_string(),
            json!(config.id.unwrap_or_default().to_string()),
        );

        // Convert ports to integers with default values
        let local_port = config.local_port.unwrap_or(0) as i32;
        let remote_port = config.remote_port.unwrap_or(0) as i32;

        values.insert("local_port".to_string(), json!(local_port));
        values.insert("remote_port".to_string(), json!(remote_port));

        values.insert(
            "remote_address".to_string(),
            json!(config
                .remote_address
                .clone()
                .unwrap_or_else(|| config.service.clone().unwrap_or_default())),
        );
        values.insert(
            "protocol".to_string(),
            json!(config.protocol.to_uppercase()),
        );

        // Create resources using the resource manager
        resource_manager
            .create_resources(&values)
            .await
            .map_err(|e| e.to_string())?;

        let mut config_clone = config.clone();
        config_clone.service = Some(pod_name.clone());

        let forward_response =
            start_port_forward(vec![config_clone], &config.protocol, http_log_state.clone())
                .await?;

        responses.extend(forward_response);
    }

    Ok(responses)
}

fn parse_service_configs(
    configs_str: &str, context: &str, namespace: &str, service_name: &str,
    ports: &HashMap<String, i32>, kubeconfig: Option<String>,
) -> Vec<Config> {
    configs_str
        .split(',')
        .filter_map(|config_str| {
            let parts: Vec<&str> = config_str.trim().split('-').collect();
            if parts.len() != 3 {
                return None;
            }

            let alias = parts[0].to_string();
            let local_port: u16 = parts[1].parse().ok()?;
            let target_port = parts[2]
                .parse()
                .ok()
                .or_else(|| ports.get(parts[2]).map(|&p| p as u16))?;

            Some(Config {
                id: None,
                context: context.to_string(),
                kubeconfig: kubeconfig.clone(),
                namespace: namespace.to_string(),
                service: Some(service_name.to_string()),
                alias: Some(alias),
                local_port: Some(local_port),
                remote_port: Some(target_port),
                protocol: "tcp".to_string(),
                workload_type: Some("service".to_string()),
                ..Default::default()
            })
        })
        .collect()
}

fn create_default_service_configs(
    context: &str, namespace: &str, service_name: &str, ports: &HashMap<String, i32>,
    kubeconfig: Option<String>,
) -> Vec<Config> {
    ports
        .iter()
        .map(|(_port_name, &port)| Config {
            id: None,
            context: context.to_string(),
            kubeconfig: kubeconfig.clone(),
            namespace: namespace.to_string(),
            service: Some(service_name.to_string()),
            alias: Some(service_name.to_string()),
            local_port: Some(port as u16),
            remote_port: Some(port as u16),
            protocol: "tcp".to_string(),
            workload_type: Some("service".to_string()),
            ..Default::default()
        })
        .collect()
}

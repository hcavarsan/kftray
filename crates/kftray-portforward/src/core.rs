use std::collections::HashMap;
use std::sync::Arc;

use hostsfile::HostsBuilder;
use kftray_commons::{
    models::{
        config_model::Config,
        config_state_model::ConfigState,
        response::CustomResponse,
    },
    utils::config_state::update_config_state,
};
use log::{
    error,
    info,
    warn,
};
use rand::Rng;
use tokio::task::JoinHandle;

use crate::kubernetes::ResourceManager;
use crate::{
    error::Error,
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

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Port forwarding error: {0}")]
    PortForward(String),
    #[error("Kubernetes client error: {0}")]
    KubeClient(String),
    #[error("Resource cleanup error: {0}")]
    Cleanup(String),
    #[error("Other error: {0}")]
    Other(String),
}

impl From<Error> for CoreError {
    fn from(e: Error) -> Self {
        match e {
            Error::Config(e) => CoreError::Config(e),
            Error::Kubernetes(e) => CoreError::KubeClient(e.to_string()),
            Error::PodNotReady(e) => CoreError::PortForward(e),
            Error::Timeout(e) => CoreError::PortForward(e),
            Error::Resource(e) => CoreError::Cleanup(e),
            e => CoreError::Other(e.to_string()),
        }
    }
}

pub async fn start_port_forward(
    configs: Vec<Config>, protocol: &str, http_log_state: Arc<HttpLogState>,
) -> Result<Vec<CustomResponse>, CoreError> {
    let mut responses = Vec::with_capacity(configs.len());
    let mut errors = Vec::new();
    let mut child_handles = Vec::with_capacity(configs.len());

    for config in configs {
        match forward_single_config(&config, protocol, http_log_state.clone()).await {
            Ok((response, handle_key)) => {
                responses.push(response);
                child_handles.push(handle_key);
            }
            Err(e) => errors.push(e.to_string()),
        }
    }

    if !errors.is_empty() {
        cleanup_handles(&child_handles).await;
        return Err(CoreError::PortForward(errors.join("; ")));
    }

    Ok(responses)
}

async fn forward_single_config(
    config: &Config, protocol: &str, http_log_state: Arc<HttpLogState>,
) -> Result<(CustomResponse, String), CoreError> {
    let target = create_target_from_config(config);
    let port_forward = create_port_forward(config, target)
        .await
        .map_err(CoreError::PortForward)?;

    let (actual_local_port, handle) = match protocol.to_lowercase().as_str() {
        "udp" => port_forward.port_forward_udp().await,
        "tcp" => port_forward.port_forward_tcp(http_log_state).await,
        _ => return Err(CoreError::Config("Unsupported protocol".into())),
    }
    .map_err(|e| CoreError::PortForward(e.to_string()))?;

    let handle_key = create_handle_key(config);

    CHILD_PROCESSES
        .lock()
        .map_err(|e| CoreError::PortForward(e.to_string()))?
        .insert(handle_key.clone(), handle);

    if config.domain_enabled.unwrap_or_default() {
        setup_hosts_file(config)?;
    }

    update_config_state(&ConfigState {
        id: None,
        config_id: config.id.unwrap_or_default(),
        is_running: true,
    })
    .await
    .map_err(|e| CoreError::Config(e.to_string()))?;

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
            config.id.unwrap_or_default()
        )),
        _ => TargetSelector::ServiceName(config.service.clone().unwrap_or_default()),
    };

    Target::new(
        selector,
        Port::Number(config.remote_port.unwrap_or(0) as i32),
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

fn setup_hosts_file(config: &Config) -> Result<(), CoreError> {
    if let (Some(service_name), Some(local_address)) = (&config.service, &config.local_address) {
        let comment = format!(
            "kftray custom host for {} - {}",
            service_name,
            config.id.unwrap_or_default()
        );

        let mut hosts_builder = HostsBuilder::new(comment);

        if let Ok(ip_addr) = local_address.parse() {
            hosts_builder.add_hostname(ip_addr, config.alias.clone().unwrap_or_default());
            hosts_builder
                .write()
                .map_err(|e| CoreError::Config(format!("Failed to write to hostfile: {}", e)))?;
        } else {
            warn!("Invalid IP address format: {}", local_address);
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

fn create_handle_key(config: &Config) -> String {
    format!(
        "{}_{}",
        config.id.unwrap_or_default(),
        config.service.clone().unwrap_or_default()
    )
}

pub async fn stop_all_port_forward() -> Result<Vec<CustomResponse>, String> {
    info!("Stopping all port forwards");
    CANCEL_NOTIFIER.notify_waiters();

    let (configs, running_configs) = get_active_configs().await?;
    if running_configs.is_empty() {
        info!("No active configs found, nothing to stop");
        return Ok(Vec::new());
    }

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

    // Stop regular port forwards
    let port_forward_responses = stop_port_forwards(handle_map, &config_map).await;

    // Clean up proxy/expose resources
    for config in &configs {
        if let (Some(config_id), Some(workload_type)) = (config.id, config.workload_type.as_deref())
        {
            if running_configs.contains(&config_id)
                && (workload_type == "proxy" || workload_type == "expose")
            {
                if let Err(e) = cleanup_proxy_resources(config).await {
                    warn!(
                        "Error cleaning up {} resources for config {}: {}",
                        workload_type, config_id, e
                    );
                }
            }
        }
    }

    cleanup_resources(&configs, &running_configs).await?;

    Ok(port_forward_responses)
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
    let mut responses = Vec::with_capacity(handle_map.len());

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
    let (config_id_str, service_id) = parse_handle_key(composite_key)?;
    let config_id = config_id_str.parse().ok()?;

    if let Some(config) = config_map.get(&config_id) {
        // Handle proxy/expose cleanup
        if let Some(workload_type) = &config.workload_type {
            if workload_type == "proxy" || workload_type == "expose" {
                if let Err(e) = cleanup_proxy_resources(config).await {
                    warn!(
                        "Error cleaning up {} resources for config {}: {}",
                        workload_type, config_id, e
                    );
                }
            }
        }

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

fn parse_handle_key(handle_key: &str) -> Option<(&str, &str)> {
    handle_key.split_once('_')
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

    let configs = kftray_commons::utils::config::get_configs()
        .await
        .map_err(|e| format!("Failed to get configs: {}", e))?;

    let config = configs
        .iter()
        .find(|c| c.id.map(|id| id.to_string()) == Some(config_id.clone()))
        .ok_or_else(|| format!("Config not found for id: {}", config_id))?;

    // Handle proxy/expose cleanup first if applicable
    if let Some(workload_type) = &config.workload_type {
        if workload_type == "proxy" || workload_type == "expose" {
            if let Err(e) = cleanup_proxy_resources(config).await {
                warn!(
                    "Error cleaning up {} resources for config {}: {}",
                    workload_type, config_id, e
                );
            }
        }
    }

    let handle_key = find_handle_key(&config_id)?;
    let join_handle = remove_handle(&handle_key)?;
    let (config_id_str, service_name) =
        parse_handle_key(&handle_key).ok_or_else(|| "Invalid handle key format".to_string())?;
    let config_id_parsed = config_id_str.parse::<i64>().unwrap_or_default();

    cleanup_config(config_id_parsed, service_name).await?;
    join_handle.abort();

    Ok(CustomResponse {
        id: None,
        service: service_name.to_string(),
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

async fn cleanup_config(config_id: i64, service_name: &str) -> Result<(), String> {
    if let Ok(configs) = kftray_commons::config::get_configs().await {
        if let Some(config) = configs.iter().find(|c| c.id == Some(config_id)) {
            cleanup_host_entry(config, service_name, &config_id.to_string());
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

fn cleanup_host_entry(config: &Config, service_id: &str, config_id_str: &str) {
    if config.domain_enabled.unwrap_or_default() {
        let hostfile_comment = format!("kftray custom host for {} - {}", service_id, config_id_str);
        if let Err(e) = HostsBuilder::new(hostfile_comment).write() {
            error!("Failed to clean up hostfile entry: {}", e);
        }
    }
}

pub async fn retrieve_service_configs(
    context: &str, kubeconfig: Option<String>,
) -> Result<Vec<Config>, String> {
    let (client_opt, _, _) =
        crate::client::create_client_with_specific_context(kubeconfig.clone(), Some(context))
            .await
            .map_err(|e| e.to_string())?;

    let client = client_opt.ok_or("Client not created")?;
    let namespaces = crate::client::list_all_namespaces(client.clone())
        .await
        .map_err(|e| e.to_string())?;

    let mut all_configs = Vec::new();
    for namespace in namespaces {
        let services = crate::client::get_services_with_annotation(
            client.clone(),
            &namespace,
            "kftray.app/configs",
        )
        .await
        .map_err(|e| e.to_string())?;

        for (service_name, annotations, ports) in services {
            if let Some(configs_str) = annotations.get("kftray.app/configs") {
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

            let (alias, local_port_str, target_port_str) = (parts[0], parts[1], parts[2]);
            let local_port = local_port_str.parse().ok()?;
            let target_port = target_port_str
                .parse()
                .ok()
                .or_else(|| ports.get(target_port_str).copied())
                .map(|p| p as u16)?;

            Some(Config {
                id: None,
                context: context.to_string(),
                kubeconfig: kubeconfig.clone(),
                namespace: namespace.to_string(),
                service: Some(service_name.to_string()),
                alias: Some(alias.to_string()),
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
        .map(|(_, &port)| Config {
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

pub async fn deploy_and_forward_pod(
    configs: Vec<Config>, http_log_state: Arc<HttpLogState>,
) -> Result<Vec<CustomResponse>, CoreError> {
    let mut responses = Vec::with_capacity(configs.len());

    for config in configs {
        let pod_name = generate_pod_name(&config)?;
        let (client, _, _) = crate::client::create_client_with_specific_context(
            config.kubeconfig.clone(),
            Some(&config.context),
        )
        .await
        .map_err(|e| CoreError::KubeClient(e.to_string()))?;

        let client = client.ok_or_else(|| CoreError::Config("Client not created".into()))?;
        let resource_manager = ResourceManager::new(client, config.namespace.clone())
            .await
            .map_err(|e| CoreError::KubeClient(e.to_string()))?;

        let values = create_manifest_values(&config, &pod_name)?;
        resource_manager
            .create_resources(&values)
            .await
            .map_err(|e| CoreError::KubeClient(e.to_string()))?;

        let mut config_clone = config.clone();
        config_clone.service = Some(pod_name);

        let forward_response =
            start_port_forward(vec![config_clone], &config.protocol, http_log_state.clone())
                .await?;

        responses.extend(forward_response);
    }

    Ok(responses)
}

fn create_manifest_values(
    config: &Config, pod_name: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, CoreError> {
    use serde_json::json;
    let mut values = serde_json::Map::new();

    values.insert("hashed_name".to_string(), json!(pod_name));
    values.insert("namespace".to_string(), json!(config.namespace));
    values.insert(
        "config_id".to_string(),
        json!(config.id.unwrap_or_default().to_string()),
    );

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

    Ok(values)
}

fn generate_pod_name(config: &Config) -> Result<String, CoreError> {
    let username = whoami::username()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect::<String>();

    let random_suffix: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(6)
        .map(char::from)
        .map(|c| c.to_ascii_lowercase())
        .collect();

    Ok(format!(
        "pkf-{}-{}-{}",
        username,
        config.protocol.to_lowercase(),
        random_suffix
    ))
}

async fn cleanup_proxy_resources(config: &Config) -> Result<(), String> {
    let (client_opt, _, _) = crate::client::create_client_with_specific_context(
        config.kubeconfig.clone(),
        Some(&config.context),
    )
    .await
    .map_err(|e| e.to_string())?;

    let client = client_opt.ok_or("Failed to create kubernetes client")?;
    let resource_manager = ResourceManager::new(client, config.namespace.clone())
        .await
        .map_err(|e| e.to_string())?;

    resource_manager
        .cleanup_proxy_resources(config)
        .await
        .map_err(|e| e.to_string())
}

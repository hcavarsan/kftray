use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::sync::Arc;
use std::time::{
    SystemTime,
    UNIX_EPOCH,
};

use hostsfile::HostsBuilder;
use k8s_openapi::api::core::v1::Pod;
use kftray_commons::models::{
    config_model::Config,
    config_state_model::ConfigState,
    response::CustomResponse,
};
use kftray_commons::utils::config_dir::get_pod_manifest_path;
use kftray_commons::utils::config_state::update_config_state;
use kube::{
    api::{
        Api,
        DeleteParams,
        ListParams,
    },
    Client,
};
use kube_runtime::wait::conditions;
use log::{
    error,
    info,
};
use rand::{
    distributions::Alphanumeric,
    Rng,
};

use crate::client::create_client_with_specific_context;
use crate::models::kube::{
    HttpLogState,
    Port,
    PortForward,
    Target,
    TargetSelector,
};
use crate::port_forward::CANCEL_NOTIFIER;
use crate::port_forward::CHILD_PROCESSES;

pub async fn start_port_forward(
    configs: Vec<Config>, protocol: &str, http_log_state: Arc<HttpLogState>,
) -> Result<Vec<CustomResponse>, String> {
    let mut responses = Vec::new();
    let mut errors = Vec::new();
    let mut child_handles = Vec::new();

    for config in configs.iter() {
        let selector = match config.workload_type.as_str() {
            "pod" => TargetSelector::PodLabel(config.target.clone().unwrap_or_default()),
            _ => TargetSelector::ServiceName(config.service.clone().unwrap_or_default()),
        };

        let remote_port = Port::from(config.remote_port as i32);
        let context_name = Some(config.context.clone());
        let kubeconfig = Some(config.kubeconfig.clone());
        let namespace = config.namespace.clone();
        let target = Target::new(selector, remote_port, namespace.clone());

        log::info!("Remote Port: {}", config.remote_port);
        log::info!("Local Port: {}", config.local_port);
        log::debug!(
            "Attempting to forward to {}: {:?}",
            if config.workload_type.as_str() == "pod" {
                "pod label"
            } else {
                "service"
            },
            &config.service
        );

        let local_address_clone = config.local_address.clone();

        let port_forward_result = PortForward::new(
            target,
            config.local_port,
            local_address_clone,
            context_name,
            kubeconfig.flatten(),
            config.id.unwrap_or_default(),
            config.workload_type.clone(),
        )
        .await;

        match port_forward_result {
            Ok(port_forward) => {
                let forward_result = match protocol {
                    "udp" => port_forward.clone().port_forward_udp().await,
                    "tcp" => {
                        port_forward
                            .clone()
                            .port_forward_tcp(http_log_state.clone())
                            .await
                    }
                    _ => Err(anyhow::anyhow!("Unsupported protocol")),
                };

                match forward_result {
                    Ok((actual_local_port, handle)) => {
                        log::info!(
                            "{} port forwarding is set up on local port: {:?} for {}: {:?}",
                            protocol.to_uppercase(),
                            actual_local_port,
                            if config.workload_type.as_str() == "pod" {
                                "pod label"
                            } else {
                                "service"
                            },
                            &config.service
                        );

                        error!("Port forwarding details: {:?}", port_forward);
                        error!("Actual local port: {:?}", actual_local_port);

                        let handle_key = format!(
                            "{}_{}",
                            config.id.unwrap(),
                            config.service.clone().unwrap_or_default()
                        );
                        CHILD_PROCESSES
                            .lock()
                            .unwrap()
                            .insert(handle_key.clone(), handle);
                        child_handles.push(handle_key.clone());

                        if config.domain_enabled.unwrap_or_default() {
                            let hostfile_comment = format!(
                                "kftray custom host for {} - {}",
                                config.service.clone().unwrap_or_default(),
                                config.id.unwrap_or_default()
                            );

                            let mut hosts_builder = HostsBuilder::new(hostfile_comment);

                            if let Some(service_name) = &config.service {
                                if let Some(local_address) = &config.local_address {
                                    match local_address.parse::<std::net::IpAddr>() {
                                        Ok(ip_addr) => {
                                            hosts_builder.add_hostname(
                                                ip_addr,
                                                config.alias.clone().unwrap_or_default(),
                                            );
                                            if let Err(e) = hosts_builder.write() {
                                                let error_message = format!(
                                                    "Failed to write to the hostfile for {}: {}",
                                                    service_name, e
                                                );
                                                log::error!("{}", &error_message);
                                                errors.push(error_message);

                                                if let Some(handle) = CHILD_PROCESSES
                                                    .lock()
                                                    .unwrap()
                                                    .remove(&handle_key)
                                                {
                                                    handle.abort();
                                                }
                                                continue;
                                            }
                                        }
                                        Err(_) => {
                                            let warning_message = format!(
                                                "Invalid IP address format: {}",
                                                local_address
                                            );
                                            log::warn!("{}", &warning_message);
                                            errors.push(warning_message);
                                        }
                                    }
                                }
                            }
                        }

                        // Update config state to running
                        let config_state = ConfigState {
                            id: None,
                            config_id: config.id.unwrap(),
                            is_running: true,
                        };
                        if let Err(e) = update_config_state(&config_state).await {
                            log::error!("Failed to update config state: {}", e);
                        }

                        responses.push(CustomResponse {
                            id: config.id,
                            service: config.service.clone().unwrap(),
                            namespace: namespace.clone(),
                            local_port: actual_local_port,
                            remote_port: config.remote_port,
                            context: config.context.clone(),
                            protocol: config.protocol.clone(),
                            stdout: format!(
                                "{} forwarding from 127.0.0.1:{} -> {}:{}",
                                protocol.to_uppercase(),
                                actual_local_port,
                                config.remote_port,
                                config.service.clone().unwrap()
                            ),
                            stderr: String::new(),
                            status: 0,
                        });
                    }
                    Err(e) => {
                        let error_message = format!(
                            "Failed to start {} port forwarding for {} {}: {}",
                            protocol.to_uppercase(),
                            if config.workload_type.as_str() == "pod" {
                                "pod label"
                            } else {
                                "service"
                            },
                            config.service.clone().unwrap_or_default(),
                            e
                        );
                        log::error!("{}", &error_message);
                        errors.push(error_message);
                    }
                }
            }
            Err(e) => {
                let error_message = format!(
                    "Failed to create PortForward for {} {}: {}",
                    if config.workload_type.as_str() == "pod" {
                        "pod label"
                    } else {
                        "service"
                    },
                    config.service.clone().unwrap_or_default(),
                    e
                );
                log::error!("{}", &error_message);
                errors.push(error_message);
            }
        }
    }

    if !errors.is_empty() {
        for handle_key in child_handles {
            if let Some(handle) = CHILD_PROCESSES.lock().unwrap().remove(&handle_key) {
                handle.abort();
            }
        }
        return Err(errors.join("\n"));
    }

    if !responses.is_empty() {
        log::info!(
            "{} port forwarding responses generated successfully.",
            protocol.to_uppercase()
        );
    }

    Ok(responses)
}

pub async fn stop_all_port_forward() -> Result<Vec<CustomResponse>, String> {
    log::info!("Attempting to stop all port forwards");

    let mut responses = Vec::new();

    let client = Client::try_default().await.map_err(|e| {
        log::error!("Failed to create Kubernetes client: {}", e);
        e.to_string()
    })?;

    // Notify all port forwarding tasks to cancel
    CANCEL_NOTIFIER.notify_waiters();

    let handle_map: HashMap<String, tokio::task::JoinHandle<()>> =
        CHILD_PROCESSES.lock().unwrap().drain().collect();

    let configs_result = kftray_commons::utils::config::get_configs().await;
    if let Err(e) = configs_result {
        let error_message = format!("Failed to retrieve configs: {}", e);
        log::error!("{}", error_message);
        return Err(error_message);
    }
    let configs = configs_result.unwrap();

    let mut pod_deletion_tasks = Vec::new();

    for (composite_key, handle) in handle_map.iter() {
        let ids: Vec<&str> = composite_key.split('_').collect();
        if ids.len() != 2 {
            log::error!(
                "Invalid composite key format encountered: {}",
                composite_key
            );
            continue;
        }

        let config_id_str = ids[0];
        let service_id = ids[1];
        let config_id_parsed = config_id_str.parse::<i64>().unwrap_or_default();

        let config = configs
            .iter()
            .find(|c| c.id.map_or(false, |id| id == config_id_parsed));
        if let Some(config) = config {
            if config.domain_enabled.unwrap_or_default() {
                let hostfile_comment =
                    format!("kftray custom host for {} - {}", service_id, config_id_str);
                let hosts_builder = HostsBuilder::new(&hostfile_comment);

                if let Err(e) = hosts_builder.write() {
                    log::error!("Failed to write to the hostfile for {}: {}", service_id, e);
                    responses.push(CustomResponse {
                        id: Some(config_id_parsed),
                        service: service_id.to_string(),
                        namespace: String::new(),
                        local_port: 0,
                        remote_port: 0,
                        context: String::new(),
                        protocol: String::new(),
                        stdout: String::new(),
                        stderr: e.to_string(),
                        status: 1,
                    });
                    let config_state = ConfigState {
                        id: None,
                        config_id: config_id_parsed,
                        is_running: false,
                    };
                    if let Err(e) = update_config_state(&config_state).await {
                        log::error!("Failed to update config state: {}", e);
                    }
                    continue;
                }
            }
        } else {
            log::warn!("Config with id '{}' not found.", config_id_str);
        }

        log::info!(
            "Aborting port forwarding task for config_id: {}",
            config_id_str
        );

        handle.abort();

        let client_clone = client.clone();
        let pod_deletion_task = async move {
            let pods: Api<Pod> = Api::all(client_clone.clone());
            let lp = ListParams::default().labels(&format!("config_id={}", config_id_str));
            log::info!(
                "Listing pods with label selector: config_id={}",
                config_id_str
            );

            let pod_list = pods.list(&lp).await.map_err(|e| {
                log::error!("Error listing pods for config_id {}: {}", config_id_str, e);
                e.to_string()
            })?;

            let username = whoami::username();
            let pod_prefix = format!("kftray-forward-{}", username);

            for pod in pod_list.items.into_iter() {
                if let Some(pod_name) = pod.metadata.name {
                    log::info!("Found pod: {}", pod_name);
                    if pod_name.starts_with(&pod_prefix) {
                        log::info!("Deleting pod: {}", pod_name);
                        let namespace = pod
                            .metadata
                            .namespace
                            .clone()
                            .unwrap_or_else(|| "default".to_string());
                        let pods_in_namespace: Api<Pod> =
                            Api::namespaced(client_clone.clone(), &namespace);
                        let dp = DeleteParams {
                            grace_period_seconds: Some(0),
                            ..DeleteParams::default()
                        };
                        if let Err(e) = pods_in_namespace.delete(&pod_name, &dp).await {
                            log::error!(
                                "Failed to delete pod {} in namespace {}: {}",
                                pod_name,
                                namespace,
                                e
                            );
                        } else {
                            log::info!("Successfully deleted pod: {}", pod_name);
                        }
                    }
                }
            }

            Ok::<(), String>(())
        };

        pod_deletion_tasks.push(pod_deletion_task);

        // Update config state to not running
        let config_state = ConfigState {
            id: None,
            config_id: config_id_parsed,
            is_running: false,
        };
        if let Err(e) = update_config_state(&config_state).await {
            log::error!("Failed to update config state: {}", e);
        }

        responses.push(CustomResponse {
            id: Some(config_id_parsed),
            service: service_id.to_string(),
            namespace: String::new(),
            local_port: 0,
            remote_port: 0,
            context: String::new(),
            protocol: String::new(),
            stdout: String::from("Service port forwarding has been stopped"),
            stderr: String::new(),
            status: 0,
        });
    }

    futures::future::join_all(pod_deletion_tasks).await;

    log::info!(
        "Port forward stopping process completed with {} responses",
        responses.len()
    );

    Ok(responses)
}

pub async fn stop_port_forward(config_id: String) -> Result<CustomResponse, String> {
    let cancellation_notifier = CANCEL_NOTIFIER.clone();
    cancellation_notifier.notify_waiters();

    // Retrieve composite key representing the child process
    let composite_key = {
        let child_processes = CHILD_PROCESSES.lock().unwrap();
        child_processes
            .keys()
            .find(|key| key.starts_with(&format!("{}_", config_id)))
            .map(|key| key.to_string())
    };

    if let Some(composite_key) = composite_key {
        // Remove and retrieve child process handle
        let join_handle = {
            let mut child_processes = CHILD_PROCESSES.lock().unwrap();
            info!("child_processes: {:?}", child_processes);
            child_processes.remove(&composite_key)
        };

        if let Some(join_handle) = join_handle {
            info!("Join handle: {:?}", join_handle);
            join_handle.abort();
        }

        // Split the composite key to get config_id and service_name
        let (config_id_str, service_name) = composite_key.split_once('_').unwrap_or(("", ""));
        let config_id_parsed = config_id_str.parse::<i64>().unwrap_or_default();

        match kftray_commons::config::get_configs().await {
            Ok(configs) => {
                if let Some(config) = configs
                    .iter()
                    .find(|c| c.id.map_or(false, |id| id == config_id_parsed))
                {
                    if config.domain_enabled.unwrap_or_default() {
                        let hostfile_comment = format!(
                            "kftray custom host for {} - {}",
                            service_name, config_id_str
                        );

                        let hosts_builder = HostsBuilder::new(hostfile_comment);

                        if let Err(e) = hosts_builder.write() {
                            log::error!(
                                "Failed to remove from the hostfile for {}: {}",
                                service_name,
                                e
                            );

                            let config_state = ConfigState {
                                id: None,
                                config_id: config_id_parsed,
                                is_running: false,
                            };
                            if let Err(e) = update_config_state(&config_state).await {
                                log::error!("Failed to update config state: {}", e);
                            }
                            return Err(e.to_string());
                        }
                    }
                } else {
                    log::warn!("Config with id '{}' not found.", config_id_str);
                }

                // Update config state to not running
                let config_state = ConfigState {
                    id: None,
                    config_id: config_id_parsed,
                    is_running: false,
                };
                if let Err(e) = update_config_state(&config_state).await {
                    log::error!("Failed to update config state: {}", e);
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
                    log::error!("Failed to update config state: {}", e);
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
            log::error!("Failed to update config state: {}", e);
        }
        Err(format!(
            "No port forwarding process found for config_id '{}'",
            config_id
        ))
    }
}

fn render_json_template(template: &str, values: &HashMap<&str, String>) -> String {
    let mut rendered_template = template.to_string();

    for (key, value) in values.iter() {
        rendered_template = rendered_template.replace(&format!("{{{}}}", key), value);
    }

    rendered_template
}

pub async fn deploy_and_forward_pod(
    configs: Vec<Config>, http_log_state: Arc<HttpLogState>,
) -> Result<Vec<CustomResponse>, String> {
    let mut responses: Vec<CustomResponse> = Vec::new();

    for mut config in configs.into_iter() {
        let client = if !config.context.is_empty() {
            let kubeconfig = config.kubeconfig.clone();

            create_client_with_specific_context(kubeconfig, &config.context)
                .await
                .map_err(|e| e.to_string())?
        } else {
            Client::try_default().await.map_err(|e| e.to_string())?
        };

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs();

        let random_string: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(6)
            .map(char::from)
            .map(|c| c.to_ascii_lowercase())
            .collect();

        let username = whoami::username().to_lowercase();
        let clean_username: String = username.chars().filter(|c| c.is_alphanumeric()).collect();

        info!("Cleaned username: {}", clean_username);

        let protocol = config.protocol.to_string().to_lowercase();

        let hashed_name = format!(
            "kftray-forward-{}-{}-{}-{}",
            clean_username, protocol, timestamp, random_string
        )
        .to_lowercase();

        let config_id_str = config
            .id
            .map_or_else(|| "default".into(), |id| id.to_string());

        if config
            .remote_address
            .as_ref()
            .map_or(true, String::is_empty)
        {
            config.remote_address.clone_from(&config.service)
        }

        let mut values: HashMap<&str, String> = HashMap::new();
        values.insert("hashed_name", hashed_name.clone());
        values.insert("config_id", config_id_str);
        values.insert("service_name", config.service.as_ref().unwrap().clone());
        values.insert(
            "remote_address",
            config.remote_address.as_ref().unwrap().clone(),
        );
        values.insert("remote_port", config.remote_port.to_string());
        values.insert("local_port", config.remote_port.to_string());
        values.insert("protocol", protocol.clone());

        let manifest_path = get_pod_manifest_path().map_err(|e| e.to_string())?;
        let mut file = File::open(manifest_path).map_err(|e| e.to_string())?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .map_err(|e| e.to_string())?;

        let rendered_json = render_json_template(&contents, &values);
        let pod: Pod = serde_json::from_str(&rendered_json).map_err(|e| e.to_string())?;

        let pods: Api<Pod> = Api::namespaced(client.clone(), &config.namespace);

        match pods.create(&kube::api::PostParams::default(), &pod).await {
            Ok(_) => {
                if let Err(e) = kube_runtime::wait::await_condition(
                    pods.clone(),
                    &hashed_name,
                    conditions::is_pod_running(),
                )
                .await
                {
                    let dp = DeleteParams {
                        grace_period_seconds: Some(0),
                        ..DeleteParams::default()
                    };
                    let _ = pods.delete(&hashed_name, &dp).await;
                    return Err(e.to_string());
                }

                config.service = Some(hashed_name.clone());

                let start_response = match protocol.as_str() {
                    "udp" => {
                        start_port_forward(vec![config.clone()], "udp", http_log_state.clone())
                            .await
                    }
                    "tcp" => {
                        start_port_forward(vec![config.clone()], "tcp", http_log_state.clone())
                            .await
                    }
                    _ => {
                        let _ = pods
                            .delete(&hashed_name, &kube::api::DeleteParams::default())
                            .await;
                        return Err("Unsupported proxy type".to_string());
                    }
                };

                match start_response {
                    Ok(mut port_forward_responses) => {
                        let response = port_forward_responses
                            .pop()
                            .ok_or("No response received from port forwarding")?;
                        responses.push(response);
                    }
                    Err(e) => {
                        let _ = pods
                            .delete(&hashed_name, &kube::api::DeleteParams::default())
                            .await;
                        return Err(format!("Failed to start port forwarding {}", e));
                    }
                }
            }
            Err(e) => return Err(e.to_string()),
        }
    }

    Ok(responses)
}

pub async fn stop_proxy_forward(
    config_id: String, namespace: &str, service_name: String,
) -> Result<CustomResponse, String> {
    log::info!(
        "Attempting to stop proxy forward for service: {}",
        service_name
    );

    let client = Client::try_default().await.map_err(|e| {
        log::error!("Failed to create Kubernetes client: {}", e);
        e.to_string()
    })?;

    let pods: Api<Pod> = Api::namespaced(client, namespace);

    let lp = ListParams::default().labels(&format!("config_id={}", config_id));

    let pod_list = pods.list(&lp).await.map_err(|e| {
        log::error!("Error listing pods: {}", e);
        e.to_string()
    })?;

    let username = whoami::username();

    let pod_prefix = format!("kftray-forward-{}", username);

    log::info!("Looking for pods with prefix: {}", pod_prefix);

    for pod in pod_list.items {
        if let Some(pod_name) = pod.metadata.name {
            if pod_name.starts_with(&pod_prefix) {
                log::info!("Found pod to stop: {}", pod_name);

                let delete_options = DeleteParams {
                    grace_period_seconds: Some(0),
                    propagation_policy: Some(kube::api::PropagationPolicy::Background),
                    ..Default::default()
                };

                match pods.delete(&pod_name, &delete_options).await {
                    Ok(_) => log::info!("Successfully deleted pod: {}", pod_name),
                    Err(e) => {
                        log::error!("Failed to delete pod: {} with error: {}", pod_name, e);
                        return Err(e.to_string());
                    }
                }

                break;
            } else {
                log::info!("Pod {} does not match prefix, skipping", pod_name);
            }
        }
    }

    log::info!("Stopping port forward for service: {}", service_name);

    let stop_result = stop_port_forward(config_id.clone()).await.map_err(|e| {
        log::error!(
            "Failed to stop port forwarding for service '{}': {}",
            service_name,
            e
        );
        e
    })?;

    log::info!("Proxy forward stopped for service: {}", service_name);

    Ok(stop_result)
}

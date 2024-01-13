use crate::kubecontext::create_client_with_specific_context;
use k8s_openapi::api::core::v1::Pod;
use kube::api::{DeleteParams, ListParams};
use kube::{api::Api, Client};
use kube_runtime::wait::conditions;
use rand::{distributions::Alphanumeric, Rng};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::port_forward::{
    start_port_forward, start_port_udp_forward, stop_port_forward, Config, CustomResponse,
};

#[tauri::command]
pub async fn deploy_and_forward_pod(configs: Vec<Config>) -> Result<Vec<CustomResponse>, String> {
    let mut responses: Vec<CustomResponse> = Vec::new();

    for mut config in configs {
        let client = if !config.context.is_empty() {
            create_client_with_specific_context(&config.context)
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
            .map(|b| char::from(b).to_ascii_lowercase())
            .collect();
        let username = whoami::username();

        let protocol = config.protocol.to_string();

        let hashed_name = format!(
            "kftray-forward-{}-{}-{}-{}",
            username, protocol, timestamp, random_string
        );

        let config_id_str = config
            .id
            .map_or_else(|| "default".into(), |id| id.to_string());

        if config
            .remote_address
            .as_ref()
            .map_or(true, String::is_empty)
        {
            config.remote_address = config.service.clone();
        }
        let pod_manifest = json!({
            "apiVersion": "v1",
            "kind": "Pod",
            "metadata": {
                "name": hashed_name,
                "labels": {
                    "app": hashed_name,
                    "config_id": config_id_str
                }
            },
            "spec": {
                "containers": [{
                    "name": hashed_name,
                    "image": "ghcr.io/hcavarsan/kftray-server:v0.5.2",
                    "env": [
                        {"name": "LOCAL_PORT", "value": config.remote_port.to_string()},
                        {"name": "REMOTE_PORT", "value": config.remote_port.to_string()},
                        {"name": "REMOTE_ADDRESS", "value": config.remote_address},
                        {"name": "PROXY_TYPE", "value": protocol},
                        {"name": "RUST_LOG", "value": "DEBUG"},
                    ],
                }],
            }
        });

        // Check if remote_address is empty and set config.service

        let pod: Pod = serde_json::from_value(pod_manifest).map_err(|e| e.to_string())?;
        let pods: Api<Pod> = Api::namespaced(client.clone(), &config.namespace);

        pods.create(&kube::api::PostParams::default(), &pod)
            .await
            .map_err(|e| e.to_string())?;
        kube_runtime::wait::await_condition(
            pods.clone(),
            &hashed_name,
            conditions::is_pod_running(),
        )
        .await
        .map_err(|e| e.to_string())?;

        config.service = Some(hashed_name.clone());

        let start_response = match protocol.as_str() {
            "udp" => start_port_udp_forward(vec![config.clone()]).await,
            "tcp" => start_port_forward(vec![config.clone()]).await,
            _ => return Err("Unsupported proxy type".to_string()),
        };

        match start_response {
            Ok(mut port_forward_responses) => {
                let response = port_forward_responses
                    .pop()
                    .ok_or("No response received from port forwarding")?;
                responses.push(response);
            }
            Err(e) => {
                return Err(format!(
                    "Failed to start port forwarding for {}: {}",
                    config.service.unwrap(),
                    e
                ));
            }
        }
    }

    Ok(responses)
}

#[tauri::command]
pub async fn stop_proxy_forward(
    config_id: String,
    namespace: &str,
    service_name: String,
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
    let stop_result = stop_port_forward(service_name.clone(), config_id)
        .await
        .map_err(|e| {
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

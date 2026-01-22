use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    pin::Pin,
    time::{
        SystemTime,
        UNIX_EPOCH,
    },
};

use futures::{
    Future,
    StreamExt,
    stream::FuturesUnordered,
};
use k8s_openapi::api::{
    apps::v1::Deployment,
    core::v1::Pod,
};
use kftray_commons::{
    models::{
        config_model::Config,
        response::CustomResponse,
    },
    utils::{
        config_dir::{
            get_pod_manifest_path,
            get_proxy_deployment_manifest_path,
        },
        db_mode::DatabaseMode,
        manifests::proxy_deployment_manifest_exists,
    },
};
use kube::Client;
use kube::api::ListParams;
use kube::api::{
    Api,
    DeleteParams,
    PostParams,
};
use kube_runtime::wait::conditions;
use log::{
    debug,
    error,
    info,
};
use rand::distr::{
    Alphanumeric,
    SampleString,
};

use crate::kube::shared_client::{
    SHARED_CLIENT_MANAGER,
    ServiceClientKey,
};

pub async fn deploy_and_forward_pod(configs: Vec<Config>) -> Result<Vec<CustomResponse>, String> {
    deploy_and_forward_pod_with_mode(configs, DatabaseMode::File, false).await
}

pub async fn deploy_and_forward_pod_with_mode(
    configs: Vec<Config>, mode: DatabaseMode, ssl_override: bool,
) -> Result<Vec<CustomResponse>, String> {
    if configs.is_empty() {
        return Ok(Vec::new());
    }

    type ProxyFuture = Pin<Box<dyn Future<Output = Result<CustomResponse, String>> + Send>>;
    let mut futures: FuturesUnordered<ProxyFuture> = FuturesUnordered::new();

    for config in configs {
        futures.push(Box::pin(process_single_proxy_config(
            config,
            mode,
            ssl_override,
        )));
    }

    // Collect results as they complete - allow partial success
    let mut responses: Vec<CustomResponse> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    while let Some(result) = futures.next().await {
        match result {
            Ok(response) => {
                responses.push(response);
            }
            Err(e) => {
                error!("Proxy config failed: {e}");
                errors.push(e);
            }
        }
    }

    if !errors.is_empty() && responses.is_empty() {
        return Err(errors.join("; "));
    }

    if !errors.is_empty() {
        error!(
            "Partial proxy deployment: {} succeeded, {} failed",
            responses.len(),
            errors.len()
        );
    }

    Ok(responses)
}

async fn process_single_proxy_config(
    mut config: Config, mode: DatabaseMode, ssl_override: bool,
) -> Result<CustomResponse, String> {
    let client_key = ServiceClientKey::new(config.context.clone(), config.kubeconfig.clone());

    let shared_client = SHARED_CLIENT_MANAGER
        .get_client(client_key)
        .await
        .map_err(|e| {
            error!("Failed to get shared Kubernetes client: {e}");
            e.to_string()
        })?;
    let client = Client::clone(&shared_client);

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();

    let random_string: String = Alphanumeric
        .sample_string(&mut rand::rng(), 6)
        .chars()
        .map(|c| c.to_ascii_lowercase())
        .collect();

    let username = whoami::username()
        .unwrap_or_else(|_| "unknown".to_string())
        .to_lowercase();
    let clean_username: String = username
        .chars()
        .filter(|c: &char| c.is_alphanumeric())
        .collect();

    let protocol = config.protocol.to_string().to_lowercase();

    let hashed_name =
        format!("kftray-forward-{clean_username}-{protocol}-{timestamp}-{random_string}")
            .to_lowercase();

    let config_id_str = config
        .id
        .map_or_else(|| "default".into(), |id| id.to_string());

    if config.remote_address.as_ref().is_none_or(|s| s.is_empty()) {
        config.remote_address.clone_from(&config.service);
    }

    let mut values: HashMap<String, String> = HashMap::new();
    values.insert("hashed_name".to_string(), hashed_name.clone());
    values.insert("config_id".to_string(), config_id_str.clone());
    values.insert(
        "service_name".to_string(),
        config.service.as_ref().unwrap().clone(),
    );
    values.insert(
        "remote_address".to_string(),
        config.remote_address.as_ref().unwrap().clone(),
    );
    values.insert(
        "remote_port".to_string(),
        config.remote_port.expect("None").to_string(),
    );
    let local_port_value = config
        .remote_port
        .unwrap_or(config.local_port.expect("None"))
        .to_string();
    values.insert("local_port".to_string(), local_port_value);
    values.insert("protocol".to_string(), protocol.clone());

    let use_deployment = should_use_deployment_manifest();

    if use_deployment {
        process_deployment_proxy(
            client,
            &mut config,
            &hashed_name,
            &config_id_str,
            &values,
            &protocol,
            mode,
            ssl_override,
        )
        .await
    } else {
        process_pod_proxy(
            client,
            &mut config,
            &hashed_name,
            &values,
            &protocol,
            mode,
            ssl_override,
        )
        .await
    }
}

#[allow(clippy::too_many_arguments)]
async fn process_deployment_proxy(
    client: Client, config: &mut Config, hashed_name: &str, config_id_str: &str,
    values: &HashMap<String, String>, protocol: &str, mode: DatabaseMode, ssl_override: bool,
) -> Result<CustomResponse, String> {
    let manifest_path = get_proxy_deployment_manifest_path().map_err(|e| e.to_string())?;
    let mut file = File::open(manifest_path).map_err(|e| e.to_string())?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|e| e.to_string())?;

    let rendered_json = render_json_template_owned(&contents, values);
    let deployment: Deployment = serde_json::from_str(&rendered_json).map_err(|e| e.to_string())?;

    let deployments: Api<Deployment> = Api::namespaced(client.clone(), &config.namespace);

    match deployments
        .create(&PostParams::default(), &deployment)
        .await
    {
        Ok(_) => {
            let pods: Api<Pod> = Api::namespaced(client.clone(), &config.namespace);
            let label_selector = format!("app={},config_id={}", hashed_name, config_id_str);
            let lp = ListParams::default().labels(&label_selector);

            let pod_name = wait_for_deployment_pod(&pods, &lp, hashed_name, &deployments).await?;

            if let Err(e) = kube_runtime::wait::await_condition(
                pods.clone(),
                &pod_name,
                conditions::is_pod_running(),
            )
            .await
            {
                let dp = DeleteParams {
                    grace_period_seconds: Some(0),
                    ..DeleteParams::default()
                };
                let _ = deployments.delete(hashed_name, &dp).await;
                return Err(e.to_string());
            }

            config.service = Some(hashed_name.to_string());

            let start_response = match protocol {
                "udp" => {
                    super::start::start_port_forward_with_mode(
                        vec![config.clone()],
                        "udp",
                        mode,
                        ssl_override,
                    )
                    .await
                }
                "tcp" => {
                    super::start::start_port_forward_with_mode(
                        vec![config.clone()],
                        "tcp",
                        mode,
                        ssl_override,
                    )
                    .await
                }
                _ => {
                    let _ = deployments
                        .delete(hashed_name, &DeleteParams::default())
                        .await;
                    return Err("Unsupported proxy type".to_string());
                }
            };

            match start_response {
                Ok(mut port_forward_responses) => match port_forward_responses.pop() {
                    Some(response) => Ok(response),
                    None => {
                        let _ = deployments
                            .delete(hashed_name, &DeleteParams::default())
                            .await;
                        Err("No response received from port forwarding".to_string())
                    }
                },
                Err(e) => {
                    let _ = deployments
                        .delete(hashed_name, &DeleteParams::default())
                        .await;
                    Err(format!("Failed to start port forwarding {e}"))
                }
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

async fn wait_for_deployment_pod(
    pods: &Api<Pod>, lp: &ListParams, hashed_name: &str, deployments: &Api<Deployment>,
) -> Result<String, String> {
    for attempt in 0..10 {
        let pod_list = pods.list(lp).await.map_err(|e| e.to_string())?;
        if let Some(pod) = pod_list.items.first()
            && let Some(name) = pod.metadata.name.clone()
        {
            return Ok(name);
        }
        if attempt < 9 {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
    }
    let dp = DeleteParams {
        grace_period_seconds: Some(0),
        ..DeleteParams::default()
    };
    let _ = deployments.delete(hashed_name, &dp).await;
    Err("No pod found for deployment after retries".to_string())
}

async fn process_pod_proxy(
    client: Client, config: &mut Config, hashed_name: &str, values: &HashMap<String, String>,
    protocol: &str, mode: DatabaseMode, ssl_override: bool,
) -> Result<CustomResponse, String> {
    let manifest_path = get_pod_manifest_path().map_err(|e| e.to_string())?;
    let mut file = File::open(manifest_path).map_err(|e| e.to_string())?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|e| e.to_string())?;

    let rendered_json = render_json_template_owned(&contents, values);
    let pod: Pod = serde_json::from_str(&rendered_json).map_err(|e| e.to_string())?;

    let pods: Api<Pod> = Api::namespaced(client.clone(), &config.namespace);

    match pods.create(&PostParams::default(), &pod).await {
        Ok(_) => {
            if let Err(e) = kube_runtime::wait::await_condition(
                pods.clone(),
                hashed_name,
                conditions::is_pod_running(),
            )
            .await
            {
                let dp = DeleteParams {
                    grace_period_seconds: Some(0),
                    ..DeleteParams::default()
                };
                let _ = pods.delete(hashed_name, &dp).await;
                return Err(e.to_string());
            }

            config.service = Some(hashed_name.to_string());

            let start_response = match protocol {
                "udp" => {
                    super::start::start_port_forward_with_mode(
                        vec![config.clone()],
                        "udp",
                        mode,
                        ssl_override,
                    )
                    .await
                }
                "tcp" => {
                    super::start::start_port_forward_with_mode(
                        vec![config.clone()],
                        "tcp",
                        mode,
                        ssl_override,
                    )
                    .await
                }
                _ => {
                    let _ = pods.delete(hashed_name, &DeleteParams::default()).await;
                    return Err("Unsupported proxy type".to_string());
                }
            };

            match start_response {
                Ok(mut port_forward_responses) => match port_forward_responses.pop() {
                    Some(response) => Ok(response),
                    None => {
                        let _ = pods.delete(hashed_name, &DeleteParams::default()).await;
                        Err("No response received from port forwarding".to_string())
                    }
                },
                Err(e) => {
                    let _ = pods.delete(hashed_name, &DeleteParams::default()).await;
                    Err(format!("Failed to start port forwarding {e}"))
                }
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

pub async fn stop_proxy_forward_with_mode(
    config_id: i64, namespace: &str, service_name: String,
    mode: kftray_commons::utils::db_mode::DatabaseMode,
) -> Result<CustomResponse, String> {
    info!("Attempting to stop proxy forward for service: {service_name}");

    let config = kftray_commons::utils::config::get_config_with_mode(config_id, mode)
        .await
        .map_err(|e| {
            error!("Failed to get config: {e}");
            e.to_string()
        })?;

    let client_key = ServiceClientKey::new(config.context.clone(), config.kubeconfig.clone());

    let shared_client = SHARED_CLIENT_MANAGER
        .get_client(client_key)
        .await
        .map_err(|e| {
            error!("Failed to get shared Kubernetes client: {e}");
            e.to_string()
        })?;
    let client = Client::clone(&shared_client);

    let pods: Api<Pod> = Api::namespaced(client, namespace);

    let lp = ListParams::default().labels(&format!("config_id={config_id}"));

    let pod_list = pods.list(&lp).await.map_err(|e| {
        error!("Error listing pods: {e}");
        e.to_string()
    })?;

    let username = whoami::username().unwrap_or_else(|_| "unknown".to_string());
    let pod_prefix = format!("kftray-forward-{username}");

    debug!("Looking for pods with prefix: {pod_prefix}");

    for pod in pod_list.items {
        if let Some(pod_name) = pod.metadata.name {
            if pod_name.starts_with(&pod_prefix) {
                info!("Found pod to stop: {pod_name}");

                let delete_options = DeleteParams {
                    grace_period_seconds: Some(0),
                    propagation_policy: Some(kube::api::PropagationPolicy::Background),
                    ..Default::default()
                };

                match pods.delete(&pod_name, &delete_options).await {
                    Ok(_) => info!("Successfully deleted pod: {pod_name}"),
                    Err(e) => {
                        error!("Failed to delete pod: {pod_name} with error: {e}");
                        return Err(e.to_string());
                    }
                }

                break;
            } else {
                info!("Pod {pod_name} does not match prefix, skipping");
            }
        }
    }

    info!("Stopping port forward for service: {service_name}");

    let stop_result = super::stop::stop_port_forward_with_mode(config_id.to_string(), mode)
        .await
        .map_err(|e| {
            error!("Failed to stop port forwarding for service '{service_name}': {e}");
            e
        })?;

    info!("Proxy forward stopped for service: {service_name}");

    Ok(stop_result)
}

pub async fn stop_proxy_forward(
    config_id: i64, namespace: &str, service_name: String,
) -> Result<CustomResponse, String> {
    info!("Attempting to stop proxy forward for service: {service_name}");

    let config = kftray_commons::config::get_config(config_id)
        .await
        .map_err(|e| {
            error!("Failed to get config: {e}");
            e.to_string()
        })?;

    let client_key = ServiceClientKey::new(config.context.clone(), config.kubeconfig.clone());

    let shared_client = SHARED_CLIENT_MANAGER
        .get_client(client_key)
        .await
        .map_err(|e| {
            error!("Failed to get shared Kubernetes client: {e}");
            e.to_string()
        })?;
    let client = Client::clone(&shared_client);

    let pods: Api<Pod> = Api::namespaced(client, namespace);

    let lp = ListParams::default().labels(&format!("config_id={config_id}"));

    let pod_list = pods.list(&lp).await.map_err(|e| {
        error!("Error listing pods: {e}");
        e.to_string()
    })?;

    let username = whoami::username().unwrap_or_else(|_| "unknown".to_string());
    let pod_prefix = format!("kftray-forward-{username}");

    debug!("Looking for pods with prefix: {pod_prefix}");

    for pod in pod_list.items {
        if let Some(pod_name) = pod.metadata.name {
            if pod_name.starts_with(&pod_prefix) {
                info!("Found pod to stop: {pod_name}");

                let delete_options = DeleteParams {
                    grace_period_seconds: Some(0),
                    propagation_policy: Some(kube::api::PropagationPolicy::Background),
                    ..Default::default()
                };

                match pods.delete(&pod_name, &delete_options).await {
                    Ok(_) => info!("Successfully deleted pod: {pod_name}"),
                    Err(e) => {
                        error!("Failed to delete pod: {pod_name} with error: {e}");
                        return Err(e.to_string());
                    }
                }

                break;
            } else {
                info!("Pod {pod_name} does not match prefix, skipping");
            }
        }
    }

    info!("Stopping port forward for service: {service_name}");

    let stop_result = super::stop::stop_port_forward_with_mode(
        config_id.to_string(),
        kftray_commons::utils::db_mode::DatabaseMode::File,
    )
    .await
    .map_err(|e| {
        error!("Failed to stop port forwarding for service '{service_name}': {e}");
        e
    })?;

    info!("Proxy forward stopped for service: {service_name}");

    Ok(stop_result)
}

fn is_custom_pod_manifest() -> bool {
    match get_pod_manifest_path() {
        Ok(path) if path.exists() => {
            // Read the current manifest
            if let Ok(mut file) = File::open(&path) {
                let mut contents = String::new();
                if file.read_to_string(&mut contents).is_ok() {
                    let size = contents.len();
                    if !(520..=780).contains(&size) {
                        debug!("Pod manifest appears customized (size: {} bytes)", size);
                        return true;
                    }
                    if contents.contains("# Custom") || contents.contains("# Modified") {
                        debug!("Pod manifest contains custom markers");
                        return true;
                    }
                    debug!("Pod manifest appears to be default template");
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}

fn should_use_deployment_manifest() -> bool {
    if is_custom_pod_manifest() {
        info!("Using legacy Pod manifest (custom detected)");
        return false;
    }

    if proxy_deployment_manifest_exists() {
        info!("Using new Deployment manifest");
        return true;
    }

    info!("Using legacy Pod manifest (Deployment not available)");
    false
}

fn render_json_template_owned(template: &str, values: &HashMap<String, String>) -> String {
    let mut rendered_template = template.to_string();

    for (key, value) in values.iter() {
        rendered_template = rendered_template.replace(&format!("{{{key}}}"), value);
    }

    rendered_template
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use kftray_commons::models::config_model::Config;

    use super::*;

    #[test]
    fn test_render_json_template_owned() {
        let template = r#"{
            "name": "{hashed_name}",
            "config_id": "{config_id}",
            "service": "{service_name}",
            "port": {remote_port}
        }"#;

        let mut values = HashMap::new();
        values.insert("hashed_name".to_string(), "test-pod".to_string());
        values.insert("config_id".to_string(), "123".to_string());
        values.insert("service_name".to_string(), "test-service".to_string());
        values.insert("remote_port".to_string(), "8080".to_string());

        let rendered = render_json_template_owned(template, &values);

        assert!(rendered.contains("\"name\": \"test-pod\""));
        assert!(rendered.contains("\"config_id\": \"123\""));
        assert!(rendered.contains("\"service\": \"test-service\""));
        assert!(rendered.contains("\"port\": 8080"));
    }

    #[test]
    fn test_render_json_template_owned_with_missing_values() {
        let template = r#"{
            "name": "{hashed_name}",
            "config_id": "{config_id}",
            "missing": "{missing_value}"
        }"#;

        let mut values = HashMap::new();
        values.insert("hashed_name".to_string(), "test-pod".to_string());
        values.insert("config_id".to_string(), "123".to_string());

        let rendered = render_json_template_owned(template, &values);

        assert!(rendered.contains("\"name\": \"test-pod\""));
        assert!(rendered.contains("\"config_id\": \"123\""));
        assert!(rendered.contains("\"missing\": \"{missing_value}\""));
    }

    #[test]
    fn test_render_json_template_owned_with_empty_values() {
        let template = r#"{"name": "{hashed_name}"}"#;
        let values = HashMap::new();

        let rendered = render_json_template_owned(template, &values);
        assert_eq!(rendered, r#"{"name": "{hashed_name}"}"#);
    }

    #[test]
    fn test_render_json_template_owned_complex() {
        let template = r#"{
            "apiVersion": "v1",
            "kind": "Pod",
            "metadata": {
                "name": "{hashed_name}",
                "labels": {
                    "app": "kftray-forward",
                    "config_id": "{config_id}"
                }
            },
            "spec": {
                "containers": [
                    {
                        "name": "proxy",
                        "image": "alpine:latest",
                        "command": ["/bin/sh"],
                        "args": ["-c", "while true; do sleep 60; done"],
                        "ports": [
                            {
                                "containerPort": {remote_port},
                                "protocol": "{protocol}"
                            }
                        ]
                    }
                ]
            }
        }"#;

        let mut values = HashMap::new();
        values.insert("hashed_name".to_string(), "test-pod-abc123".to_string());
        values.insert("config_id".to_string(), "456".to_string());
        values.insert("remote_port".to_string(), "9090".to_string());
        values.insert("protocol".to_string(), "TCP".to_string());

        let rendered = render_json_template_owned(template, &values);

        assert!(rendered.contains("\"name\": \"test-pod-abc123\""));
        assert!(rendered.contains("\"config_id\": \"456\""));
        assert!(rendered.contains("\"containerPort\": 9090"));
        assert!(rendered.contains("\"protocol\": \"TCP\""));
    }

    #[tokio::test]
    async fn test_deploy_and_forward_pod_empty_config() {
        let configs = Vec::new();

        let result = deploy_and_forward_pod(configs).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_deploy_and_forward_pod_invalid_kubeconfig() {
        let config = Config {
            id: Some(1),
            context: Some("invalid-context".to_string()),
            kubeconfig: Some("invalid-kubeconfig".to_string()),
            namespace: "default".to_string(),
            service: Some("test-service".to_string()),
            alias: None,
            local_port: Some(8080),
            remote_port: Some(8080),
            protocol: "tcp".to_string(),
            workload_type: Some("service".to_string()),
            target: None,
            local_address: None,
            auto_loopback_address: false,
            remote_address: None,
            domain_enabled: None,
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
        };

        let result = deploy_and_forward_pod(vec![config]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_stop_proxy_forward_invalid_config() {
        let result = stop_proxy_forward(999, "default", "nonexistent-service".to_string()).await;
        assert!(result.is_err());
    }
}

use std::{
    collections::HashMap,
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

use crate::kube::shared_client::ServiceClientKey;
use crate::port_forward_error::PortForwardError;
use crate::registry::PORT_FORWARD_REGISTRY;

pub async fn deploy_and_forward_pod(
    configs: Vec<Config>,
) -> Result<Vec<CustomResponse>, PortForwardError> {
    deploy_and_forward_pod_with_mode(configs, DatabaseMode::File, false).await
}

pub async fn deploy_and_forward_pod_with_mode(
    configs: Vec<Config>, mode: DatabaseMode, ssl_override: bool,
) -> Result<Vec<CustomResponse>, PortForwardError> {
    if configs.is_empty() {
        return Ok(Vec::new());
    }

    type ProxyFuture =
        Pin<Box<dyn Future<Output = Result<CustomResponse, PortForwardError>> + Send>>;
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
                let msg = e.to_string();
                error!("Proxy config failed: {msg}");
                errors.push(msg);
            }
        }
    }

    if !errors.is_empty() && responses.is_empty() {
        return Err(PortForwardError::Internal(errors.join("; ")));
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
) -> Result<CustomResponse, PortForwardError> {
    let client_key = ServiceClientKey::new(config.context.clone(), config.kubeconfig.clone());

    let shared_client = PORT_FORWARD_REGISTRY
        .acquire_client(client_key)
        .await
        .map_err(|e| {
            error!("Failed to get shared Kubernetes client: {e}");
            PortForwardError::KubeApi(e.to_string())
        })?;
    let client = Client::clone(&shared_client);

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| PortForwardError::Internal(e.to_string()))?
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

    let protocol = config.protocol.clone().to_lowercase();

    let hashed_name =
        format!("kftray-forward-{clean_username}-{protocol}-{timestamp}-{random_string}")
            .to_lowercase();

    let config_id_str = config
        .id
        .map_or_else(|| "default".into(), |id| id.to_string());

    if config.remote_address.as_ref().is_none_or(String::is_empty) {
        config.remote_address.clone_from(&config.service);
    }

    let service =
        config
            .service
            .as_deref()
            .ok_or_else(|| PortForwardError::ConfigurationError {
                message: "config missing service".to_string(),
            })?;
    let remote_address =
        config
            .remote_address
            .as_deref()
            .ok_or_else(|| PortForwardError::ConfigurationError {
                message: "config missing remote_address".to_string(),
            })?;
    let remote_port = config
        .remote_port
        .ok_or_else(|| PortForwardError::ConfigurationError {
            message: "config missing remote_port".to_string(),
        })?;
    let local_port = config
        .local_port
        .ok_or_else(|| PortForwardError::ConfigurationError {
            message: "config missing local_port".to_string(),
        })?;

    let mut values: HashMap<String, String> = HashMap::new();
    values.insert("hashed_name".to_string(), hashed_name.clone());
    values.insert("config_id".to_string(), config_id_str.clone());
    values.insert("service_name".to_string(), service.to_string());
    values.insert("remote_address".to_string(), remote_address.to_string());
    values.insert("remote_port".to_string(), remote_port.to_string());
    let local_port_value = config.remote_port.unwrap_or(local_port).to_string();
    values.insert("local_port".to_string(), local_port_value);
    values.insert("protocol".to_string(), protocol.clone());

    let use_deployment = should_use_deployment_manifest().await;

    // Both branches build very large futures (their state machines compose
    // multi-step k8s API calls). Boxing keeps the parent future small enough
    // to avoid blowing the tokio task stack on the spawn-site.
    if use_deployment {
        Box::pin(process_deployment_proxy(
            client,
            &mut config,
            &hashed_name,
            &config_id_str,
            &values,
            &protocol,
            mode,
            ssl_override,
        ))
        .await
    } else {
        Box::pin(process_pod_proxy(
            client,
            &mut config,
            &hashed_name,
            &values,
            &protocol,
            mode,
            ssl_override,
        ))
        .await
    }
}

#[allow(clippy::too_many_arguments)]
async fn process_deployment_proxy(
    client: Client, config: &mut Config, hashed_name: &str, config_id_str: &str,
    values: &HashMap<String, String>, protocol: &str, mode: DatabaseMode, ssl_override: bool,
) -> Result<CustomResponse, PortForwardError> {
    let manifest_path = get_proxy_deployment_manifest_path().map_err(PortForwardError::Internal)?;
    let contents = tokio::fs::read_to_string(&manifest_path)
        .await
        .map_err(PortForwardError::Io)?;

    let rendered_json = render_json_template_owned(&contents, values);
    let deployment: Deployment = serde_json::from_str(&rendered_json)
        .map_err(|e| PortForwardError::KubeApi(e.to_string()))?;

    let deployments: Api<Deployment> = Api::namespaced(client.clone(), &config.namespace);

    match deployments
        .create(&PostParams::default(), &deployment)
        .await
    {
        Ok(_) => {
            let pods: Api<Pod> = Api::namespaced(client.clone(), &config.namespace);
            let label_selector = format!("app={hashed_name},config_id={config_id_str}");
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
                return Err(PortForwardError::KubeApi(e.to_string()));
            }

            config.service = Some(hashed_name.to_string());

            let start_response = match protocol {
                "udp" => {
                    crate::kube::start::start_port_forward_with_mode(
                        vec![config.clone()],
                        "udp",
                        mode,
                        ssl_override,
                    )
                    .await
                }
                "tcp" => {
                    crate::kube::start::start_port_forward_with_mode(
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
                    return Err(PortForwardError::ConfigurationError {
                        message: "Unsupported proxy type".to_string(),
                    });
                }
            };

            match start_response {
                Ok(mut port_forward_responses) => match port_forward_responses.pop() {
                    Some(response) => {
                        // Spawn recovery manager for deployment proxy
                        super::recovery::spawn_recovery_manager(
                            config.clone(),
                            super::recovery::ProxyType::Deployment,
                        );
                        Ok(response)
                    }
                    None => {
                        let _ = deployments
                            .delete(hashed_name, &DeleteParams::default())
                            .await;
                        Err(PortForwardError::Internal(
                            "No response received from port forwarding".to_string(),
                        ))
                    }
                },
                Err(e) => {
                    let _ = deployments
                        .delete(hashed_name, &DeleteParams::default())
                        .await;
                    Err(PortForwardError::Internal(format!(
                        "Failed to start port forwarding {e}"
                    )))
                }
            }
        }
        Err(e) => Err(PortForwardError::KubeApi(e.to_string())),
    }
}

async fn wait_for_deployment_pod(
    pods: &Api<Pod>, lp: &ListParams, hashed_name: &str, deployments: &Api<Deployment>,
) -> Result<String, PortForwardError> {
    for attempt in 0..10 {
        let pod_list = pods
            .list(lp)
            .await
            .map_err(|e| PortForwardError::KubeApi(e.to_string()))?;
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
    Err(PortForwardError::KubeApi(
        "No pod found for deployment after retries".to_string(),
    ))
}

async fn process_pod_proxy(
    client: Client, config: &mut Config, hashed_name: &str, values: &HashMap<String, String>,
    protocol: &str, mode: DatabaseMode, ssl_override: bool,
) -> Result<CustomResponse, PortForwardError> {
    let manifest_path = get_pod_manifest_path().map_err(PortForwardError::Internal)?;
    let contents = tokio::fs::read_to_string(&manifest_path)
        .await
        .map_err(PortForwardError::Io)?;

    let rendered_json = render_json_template_owned(&contents, values);
    let pod: Pod = serde_json::from_str(&rendered_json)
        .map_err(|e| PortForwardError::KubeApi(e.to_string()))?;

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
                return Err(PortForwardError::KubeApi(e.to_string()));
            }

            config.service = Some(hashed_name.to_string());

            let start_response = match protocol {
                "udp" => {
                    crate::kube::start::start_port_forward_with_mode(
                        vec![config.clone()],
                        "udp",
                        mode,
                        ssl_override,
                    )
                    .await
                }
                "tcp" => {
                    crate::kube::start::start_port_forward_with_mode(
                        vec![config.clone()],
                        "tcp",
                        mode,
                        ssl_override,
                    )
                    .await
                }
                _ => {
                    let _ = pods.delete(hashed_name, &DeleteParams::default()).await;
                    return Err(PortForwardError::ConfigurationError {
                        message: "Unsupported proxy type".to_string(),
                    });
                }
            };

            match start_response {
                Ok(mut port_forward_responses) => match port_forward_responses.pop() {
                    Some(response) => {
                        // Spawn recovery manager for bare pod proxy
                        super::recovery::spawn_recovery_manager(
                            config.clone(),
                            super::recovery::ProxyType::BarePod,
                        );
                        Ok(response)
                    }
                    None => {
                        let _ = pods.delete(hashed_name, &DeleteParams::default()).await;
                        Err(PortForwardError::Internal(
                            "No response received from port forwarding".to_string(),
                        ))
                    }
                },
                Err(e) => {
                    let _ = pods.delete(hashed_name, &DeleteParams::default()).await;
                    Err(PortForwardError::Internal(format!(
                        "Failed to start port forwarding {e}"
                    )))
                }
            }
        }
        Err(e) => Err(PortForwardError::KubeApi(e.to_string())),
    }
}

pub async fn stop_proxy_forward_with_mode(
    config_id: i64, _namespace: &str, service_name: String, mode: DatabaseMode,
) -> Result<CustomResponse, PortForwardError> {
    info!("Stopping proxy forward for service: {service_name}");
    crate::kube::stop::stop_port_forward_with_mode(config_id.to_string(), mode)
        .await
        .map_err(|e| {
            error!("Failed to stop port forwarding for service '{service_name}': {e}");
            e
        })
}

pub async fn stop_proxy_forward(
    config_id: i64, _namespace: &str, service_name: String,
) -> Result<CustomResponse, PortForwardError> {
    info!("Stopping proxy forward for service: {service_name}");
    crate::kube::stop::stop_port_forward_with_mode(config_id.to_string(), DatabaseMode::File)
        .await
        .map_err(|e| {
            error!("Failed to stop port forwarding for service '{service_name}': {e}");
            e
        })
}

async fn is_custom_pod_manifest() -> bool {
    match get_pod_manifest_path() {
        Ok(path) if path.exists() => {
            // Read the current manifest using async I/O
            match tokio::fs::read_to_string(&path).await {
                Ok(contents) => {
                    let size = contents.len();
                    if !(520..=780).contains(&size) {
                        debug!("Pod manifest appears customized (size: {size} bytes)");
                        return true;
                    }
                    if contents.contains("# Custom") || contents.contains("# Modified") {
                        debug!("Pod manifest contains custom markers");
                        return true;
                    }
                    debug!("Pod manifest appears to be default template");
                    false
                }
                Err(_) => true,
            }
        }
        _ => false,
    }
}

async fn should_use_deployment_manifest() -> bool {
    if is_custom_pod_manifest().await {
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

    for (key, value) in values {
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

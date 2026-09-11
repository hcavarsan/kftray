use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    time::{
        SystemTime,
        UNIX_EPOCH,
    },
};

use futures::{
    StreamExt,
    TryStreamExt,
    stream,
};
use k8s_openapi::api::{
    apps::v1::Deployment,
    core::v1::{
        Pod,
        PodSpec,
        Probe,
        TCPSocketAction,
    },
};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
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
use kube_runtime::WatchStreamExt;
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
    let mut futures = stream::iter(configs)
        .map(|config| process_single_proxy_config(config, mode, ssl_override))
        .buffer_unordered(16);
    let mut responses = Vec::new();
    let mut errors = Vec::new();
    while let Some(result) = futures.next().await {
        match result {
            Ok(response) => responses.push(response),
            Err(error) => errors.push(error),
        }
    }
    if responses.is_empty() && !errors.is_empty() {
        Err(errors.join("; "))
    } else {
        for error in errors {
            error!("Proxy config failed: {error}");
        }
        Ok(responses)
    }
}

async fn process_single_proxy_config(
    config: Config, mode: DatabaseMode, ssl_override: bool,
) -> Result<CustomResponse, String> {
    let id = config.id.ok_or("Config has no ID")?;
    let lock = crate::kube::proxy_recovery::acquire_recovery_lock(id).await;
    let guard = lock.lock().await;
    let result = if crate::port_forward::CHILD_PROCESSES.contains_key(&id) {
        Err(format!(
            "Port forwarding is already running for config {id}"
        ))
    } else {
        start_proxy_config(config, mode, ssl_override).await
    };
    drop(guard);
    drop(lock);
    crate::kube::proxy_recovery::remove_recovery_lock(id);
    result
}

pub(super) async fn start_proxy_config(
    mut config: Config, mode: DatabaseMode, ssl_override: bool,
) -> Result<CustomResponse, String> {
    let protocol = config.protocol.to_ascii_lowercase();
    if !matches!(protocol.as_str(), "tcp" | "udp") {
        return Err(format!("Unsupported proxy protocol: {protocol}"));
    }
    let client_key = ServiceClientKey::new(config.context.clone(), config.kubeconfig.clone());

    let shared_client = SHARED_CLIENT_MANAGER
        .get_connection(client_key)
        .await
        .map_err(|e| {
            error!("Failed to get shared Kubernetes client: {e}");
            e.to_string()
        })?;
    let client = shared_client.client.clone();

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

    let hashed_name =
        format!("kftray-forward-{clean_username}-{protocol}-{timestamp}-{random_string}")
            .to_lowercase();

    let config_id_str = config
        .id
        .map_or_else(|| "default".into(), |id| id.to_string());

    let remote_address = config
        .remote_address
        .take()
        .filter(|address| !address.is_empty())
        .or_else(|| config.service.clone().filter(|service| !service.is_empty()))
        .ok_or("A proxy destination address or service is required")?;
    let remote_port = config
        .remote_port
        .filter(|port| *port > 0)
        .ok_or("A proxy destination port is required")?;
    let service_name = config
        .service
        .clone()
        .filter(|service| !service.is_empty())
        .unwrap_or_else(|| remote_address.clone());
    config.remote_address = Some(remote_address.clone());

    let mut values: HashMap<String, String> = HashMap::new();
    values.insert("hashed_name".to_string(), hashed_name.clone());
    values.insert("config_id".to_string(), config_id_str.clone());
    values.insert("service_name".to_string(), service_name);
    values.insert("remote_address".to_string(), remote_address);
    values.insert("remote_port".to_string(), remote_port.to_string());
    values.insert("local_port".to_string(), remote_port.to_string());
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

fn prepare_relay_startup(spec: &mut PodSpec, port: u16) -> Result<String, String> {
    let index = spec
        .containers
        .iter()
        .position(|container| {
            container
                .env
                .as_ref()
                .is_some_and(|env| env.iter().any(|variable| variable.name == "LOCAL_PORT"))
        })
        .unwrap_or(0);
    let container = spec
        .containers
        .get_mut(index)
        .ok_or("Proxy manifest must contain a container")?;
    container.startup_probe.get_or_insert_with(|| Probe {
        tcp_socket: Some(TCPSocketAction {
            port: IntOrString::Int(i32::from(port)),
            ..Default::default()
        }),
        period_seconds: Some(1),
        timeout_seconds: Some(1),
        failure_threshold: Some(30),
        ..Default::default()
    });
    Ok(container.name.clone())
}

fn relay_started(pod: Option<&Pod>, container_name: &str) -> bool {
    pod.is_some_and(|pod| {
        pod.metadata.deletion_timestamp.is_none()
            && pod.status.as_ref().is_some_and(|status| {
                status.phase.as_deref() == Some("Running")
                    && status
                        .container_statuses
                        .as_ref()
                        .is_some_and(|containers| {
                            containers.iter().any(|container| {
                                container.name == container_name && container.started == Some(true)
                            })
                        })
            })
    })
}

async fn wait_for_relay_startup(
    pods: &Api<Pod>, pod_name: &str, container_name: &str,
) -> Result<(), String> {
    tokio::time::timeout(
        std::time::Duration::from_secs(120),
        kube_runtime::wait::await_condition(pods.clone(), pod_name, |pod: Option<&Pod>| {
            relay_started(pod, container_name)
        }),
    )
    .await
    .map_err(|_| format!("Timed out waiting for proxy listener in pod {pod_name}"))?
    .map(|_| ())
    .map_err(|error| error.to_string())
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
    let mut deployment: Deployment =
        serde_json::from_str(&rendered_json).map_err(|e| e.to_string())?;
    let spec = deployment
        .spec
        .as_mut()
        .and_then(|spec| spec.template.spec.as_mut())
        .ok_or("Proxy deployment must contain a pod specification")?;
    let container_name = prepare_relay_startup(
        spec,
        config
            .remote_port
            .ok_or("A proxy destination port is required")?,
    )?;

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

            if let Err(e) = wait_for_relay_startup(&pods, &pod_name, &container_name).await {
                let dp = DeleteParams {
                    grace_period_seconds: Some(0),
                    ..DeleteParams::default()
                };
                let _ = deployments.delete(hashed_name, &dp).await;
                return Err(e.to_string());
            }

            config.service = Some(hashed_name.to_string());

            match super::start::start_config(config.clone(), protocol, mode, ssl_override).await {
                Ok(response) => {
                    crate::kube::proxy_recovery::spawn_recovery_manager(
                        config.clone(),
                        crate::kube::proxy_recovery::ProxyType::Deployment,
                        mode,
                    );
                    Ok(response)
                }
                Err(error) => {
                    let _ = deployments
                        .delete(hashed_name, &DeleteParams::default())
                        .await;
                    Err(format!("Failed to start port forwarding: {error}"))
                }
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

async fn wait_for_deployment_pod(
    pods: &Api<Pod>, lp: &ListParams, hashed_name: &str, deployments: &Api<Deployment>,
) -> Result<String, String> {
    let watcher = kube_runtime::watcher(
        pods.clone(),
        kube_runtime::watcher::Config::default()
            .labels(lp.label_selector.as_deref().unwrap_or_default()),
    )
    .applied_objects();
    futures::pin_mut!(watcher);
    let result =
        tokio::time::timeout(std::time::Duration::from_secs(120), watcher.try_next()).await;
    let error = match result {
        Ok(Ok(Some(pod))) => {
            if let Some(name) = pod.metadata.name {
                return Ok(name);
            }
            "Proxy pod has no name".to_string()
        }
        Ok(Ok(None)) => "Proxy pod watch ended before a pod was created".to_string(),
        Ok(Err(error)) => error.to_string(),
        Err(_) => "Timed out waiting for the proxy deployment pod".to_string(),
    };
    let dp = DeleteParams {
        grace_period_seconds: Some(0),
        ..DeleteParams::default()
    };
    let _ = deployments.delete(hashed_name, &dp).await;
    Err(error)
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
    let mut pod: Pod = serde_json::from_str(&rendered_json).map_err(|e| e.to_string())?;
    let spec = pod
        .spec
        .as_mut()
        .ok_or("Proxy pod must contain a pod specification")?;
    let container_name = prepare_relay_startup(
        spec,
        config
            .remote_port
            .ok_or("A proxy destination port is required")?,
    )?;

    let pods: Api<Pod> = Api::namespaced(client.clone(), &config.namespace);

    match pods.create(&PostParams::default(), &pod).await {
        Ok(_) => {
            if let Err(e) = wait_for_relay_startup(&pods, hashed_name, &container_name).await {
                let dp = DeleteParams {
                    grace_period_seconds: Some(0),
                    ..DeleteParams::default()
                };
                let _ = pods.delete(hashed_name, &dp).await;
                return Err(e.to_string());
            }

            config.service = Some(hashed_name.to_string());

            match super::start::start_config(config.clone(), protocol, mode, ssl_override).await {
                Ok(response) => {
                    crate::kube::proxy_recovery::spawn_recovery_manager(
                        config.clone(),
                        crate::kube::proxy_recovery::ProxyType::BarePod,
                        mode,
                    );
                    Ok(response)
                }
                Err(error) => {
                    let _ = pods.delete(hashed_name, &DeleteParams::default()).await;
                    Err(format!("Failed to start port forwarding: {error}"))
                }
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

pub async fn stop_proxy_forward_with_mode(
    config_id: i64, _namespace: &str, service_name: String,
    mode: kftray_commons::utils::db_mode::DatabaseMode,
) -> Result<CustomResponse, String> {
    info!("Stopping proxy forward for service: {service_name}");
    super::stop::stop_port_forward_with_mode(config_id.to_string(), mode)
        .await
        .map_err(|e| {
            error!("Failed to stop port forwarding for service '{service_name}': {e}");
            e
        })
}

pub async fn stop_proxy_forward(
    config_id: i64, _namespace: &str, service_name: String,
) -> Result<CustomResponse, String> {
    info!("Stopping proxy forward for service: {service_name}");
    super::stop::stop_port_forward_with_mode(
        config_id.to_string(),
        kftray_commons::utils::db_mode::DatabaseMode::File,
    )
    .await
    .map_err(|e| {
        error!("Failed to stop port forwarding for service '{service_name}': {e}");
        e
    })
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

    #[test]
    fn running_relay_waits_for_listener_startup() {
        let pod: Pod = serde_json::from_value(serde_json::json!({
            "metadata": {"name": "relay"},
            "status": {
                "phase": "Running",
                "containerStatuses": [{
                    "name": "relay", "started": false, "ready": false,
                    "restartCount": 0, "image": "relay", "imageID": "relay"
                }]
            }
        }))
        .unwrap();
        assert!(!relay_started(Some(&pod), "relay"));
    }

    #[test]
    fn started_relay_does_not_require_sidecar_readiness() {
        let pod: Pod = serde_json::from_value(serde_json::json!({
            "metadata": {"name": "relay"},
            "status": {
                "phase": "Running",
                "conditions": [{"type": "Ready", "status": "False"}],
                "containerStatuses": [
                    {"name": "relay", "started": true, "ready": true,
                     "restartCount": 0, "image": "relay", "imageID": "relay"},
                    {"name": "sidecar", "started": true, "ready": false,
                     "restartCount": 0, "image": "sidecar", "imageID": "sidecar"}
                ]
            }
        }))
        .unwrap();
        assert!(relay_started(Some(&pod), "relay"));
    }

    #[tokio::test]
    async fn test_stop_proxy_forward_invalid_config() {
        let result = stop_proxy_forward(999, "default", "nonexistent-service".to_string()).await;
        assert!(result.is_err());
    }
}

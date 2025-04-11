use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    sync::Arc,
    time::{
        SystemTime,
        UNIX_EPOCH,
    },
};

use k8s_openapi::api::core::v1::Pod;
use kftray_commons::{
    models::{
        config_model::Config,
        response::CustomResponse,
    },
    utils::config_dir::get_pod_manifest_path,
};
use kftray_http_logs::HttpLogState;
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

use crate::create_client_with_specific_context;

pub async fn deploy_and_forward_pod(
    configs: Vec<Config>, http_log_state: Arc<HttpLogState>,
) -> Result<Vec<CustomResponse>, String> {
    let mut responses: Vec<CustomResponse> = Vec::new();

    for mut config in configs.into_iter() {
        let context_name = Some(config.context.as_str());
        let kubeconfig_clone = config.kubeconfig.clone();
        let (client, _, _) = create_client_with_specific_context(kubeconfig_clone, context_name)
            .await
            .map_err(|e| {
                error!("Failed to create Kubernetes client: {}", e);
                e.to_string()
            })?;

        let client = client.ok_or_else(|| "Client not created".to_string())?;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs();

        let random_string: String = Alphanumeric
            .sample_string(&mut rand::rng(), 6)
            .chars()
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

        if config.remote_address.as_ref().is_none_or(String::is_empty) {
            config.remote_address.clone_from(&config.service);
        }

        let mut values: HashMap<&str, String> = HashMap::new();
        values.insert("hashed_name", hashed_name.clone());
        values.insert("config_id", config_id_str);
        values.insert("service_name", config.service.as_ref().unwrap().clone());
        values.insert(
            "remote_address",
            config.remote_address.as_ref().unwrap().clone(),
        );
        values.insert("remote_port", config.remote_port.expect("None").to_string());
        values.insert("local_port", config.remote_port.expect("None").to_string());
        values.insert("protocol", protocol.clone());

        let manifest_path = get_pod_manifest_path().map_err(|e| e.to_string())?;
        let mut file = File::open(manifest_path).map_err(|e| e.to_string())?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .map_err(|e| e.to_string())?;

        let rendered_json = render_json_template(&contents, &values);
        let pod: Pod = serde_json::from_str(&rendered_json).map_err(|e| e.to_string())?;

        let pods: Api<Pod> = Api::namespaced(client.clone(), &config.namespace);

        match pods.create(&PostParams::default(), &pod).await {
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
                        super::start::start_port_forward(
                            vec![config.clone()],
                            "udp",
                            http_log_state.clone(),
                        )
                        .await
                    }
                    "tcp" => {
                        super::start::start_port_forward(
                            vec![config.clone()],
                            "tcp",
                            http_log_state.clone(),
                        )
                        .await
                    }
                    _ => {
                        let _ = pods.delete(&hashed_name, &DeleteParams::default()).await;
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
                        let _ = pods.delete(&hashed_name, &DeleteParams::default()).await;
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
    config_id: i64, namespace: &str, service_name: String,
) -> Result<CustomResponse, String> {
    info!(
        "Attempting to stop proxy forward for service: {}",
        service_name
    );

    let config = kftray_commons::config::get_config(config_id)
        .await
        .map_err(|e| {
            error!("Failed to get config: {}", e);
            e.to_string()
        })?;

    let kubeconfig = config
        .kubeconfig
        .ok_or_else(|| "Kubeconfig not found".to_string())?;
    let context_name = &config.context;

    let (client, _, _) = create_client_with_specific_context(Some(kubeconfig), Some(context_name))
        .await
        .map_err(|e| {
            error!("Failed to create Kubernetes client: {}", e);
            e.to_string()
        })?;

    let client = client.ok_or_else(|| "Client not created".to_string())?;

    let pods: Api<Pod> = Api::namespaced(client, namespace);

    let lp = ListParams::default().labels(&format!("config_id={}", config_id));

    let pod_list = pods.list(&lp).await.map_err(|e| {
        error!("Error listing pods: {}", e);
        e.to_string()
    })?;

    let username = whoami::username();
    let pod_prefix = format!("kftray-forward-{}", username);

    debug!("Looking for pods with prefix: {}", pod_prefix);

    for pod in pod_list.items {
        if let Some(pod_name) = pod.metadata.name {
            if pod_name.starts_with(&pod_prefix) {
                info!("Found pod to stop: {}", pod_name);

                let delete_options = DeleteParams {
                    grace_period_seconds: Some(0),
                    propagation_policy: Some(kube::api::PropagationPolicy::Background),
                    ..Default::default()
                };

                match pods.delete(&pod_name, &delete_options).await {
                    Ok(_) => info!("Successfully deleted pod: {}", pod_name),
                    Err(e) => {
                        error!("Failed to delete pod: {} with error: {}", pod_name, e);
                        return Err(e.to_string());
                    }
                }

                break;
            } else {
                info!("Pod {} does not match prefix, skipping", pod_name);
            }
        }
    }

    info!("Stopping port forward for service: {}", service_name);

    let stop_result = super::stop::stop_port_forward(config_id.to_string())
        .await
        .map_err(|e| {
            error!(
                "Failed to stop port forwarding for service '{}': {}",
                service_name, e
            );
            e
        })?;

    info!("Proxy forward stopped for service: {}", service_name);

    Ok(stop_result)
}

fn render_json_template(template: &str, values: &HashMap<&str, String>) -> String {
    let mut rendered_template = template.to_string();

    for (key, value) in values.iter() {
        rendered_template = rendered_template.replace(&format!("{{{}}}", key), value);
    }

    rendered_template
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn test_render_json_template() {
        let template = r#"{
            "name": "{hashed_name}",
            "config_id": "{config_id}",
            "service": "{service_name}",
            "port": {remote_port}
        }"#;

        let mut values = HashMap::new();
        values.insert("hashed_name", "test-pod".to_string());
        values.insert("config_id", "123".to_string());
        values.insert("service_name", "test-service".to_string());
        values.insert("remote_port", "8080".to_string());

        let rendered = render_json_template(template, &values);

        assert!(rendered.contains("\"name\": \"test-pod\""));
        assert!(rendered.contains("\"config_id\": \"123\""));
        assert!(rendered.contains("\"service\": \"test-service\""));
        assert!(rendered.contains("\"port\": 8080"));
    }

    #[test]
    fn test_render_json_template_with_missing_values() {
        let template = r#"{
            "name": "{hashed_name}",
            "config_id": "{config_id}",
            "missing": "{missing_value}"
        }"#;

        let mut values = HashMap::new();
        values.insert("hashed_name", "test-pod".to_string());
        values.insert("config_id", "123".to_string());

        let rendered = render_json_template(template, &values);

        assert!(rendered.contains("\"name\": \"test-pod\""));
        assert!(rendered.contains("\"config_id\": \"123\""));
        assert!(rendered.contains("\"missing\": \"{missing_value}\""));
    }

    #[test]
    fn test_render_json_template_with_empty_values() {
        let template = r#"{"name": "{hashed_name}"}"#;
        let values = HashMap::new();

        let rendered = render_json_template(template, &values);
        assert_eq!(rendered, r#"{"name": "{hashed_name}"}"#);
    }
}

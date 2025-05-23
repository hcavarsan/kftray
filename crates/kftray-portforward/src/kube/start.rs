use std::sync::Arc;

use kftray_commons::{
    models::{
        config_model::Config,
        config_state_model::ConfigState,
        response::CustomResponse,
    },
    utils::config_state::update_config_state,
};
use kftray_http_logs::HttpLogState;
use log::{
    debug,
    error,
    info,
    warn,
};

use crate::{
    hostsfile::{
        add_host_entry,
        HostEntry,
    },
    kube::models::{
        Port,
        PortForward,
        Target,
        TargetSelector,
    },
    port_forward::CHILD_PROCESSES,
};

pub async fn start_port_forward(
    configs: Vec<Config>, protocol: &str, http_log_state: Arc<HttpLogState>,
) -> Result<Vec<CustomResponse>, String> {
    let mut responses = Vec::new();
    let mut errors = Vec::new();
    let mut child_handles = Vec::new();

    for config in configs.iter() {
        let selector = match config.workload_type.as_deref() {
            Some("pod") => TargetSelector::PodLabel(config.target.clone().unwrap_or_default()),
            _ => TargetSelector::ServiceName(config.service.clone().unwrap_or_default()),
        };

        let remote_port = Port::from(config.remote_port.unwrap_or_default() as i32);
        let context_name = Some(config.context.clone());
        let kubeconfig = Some(config.kubeconfig.clone());
        let namespace = config.namespace.clone();
        let target = Target::new(selector, remote_port, namespace.clone());

        debug!("Remote Port: {:?}", config.remote_port);
        debug!("Local Port: {:?}", config.local_port);

        if config.workload_type.as_deref() == Some("pod") {
            info!("Attempting to forward to pod label: {:?}", &config.target);
        } else {
            info!("Attempting to forward to service: {:?}", &config.service);
        }

        let local_address_clone = config.local_address.clone();

        let port_forward_result: Result<PortForward, anyhow::Error> = PortForward::new(
            target,
            config.local_port,
            local_address_clone,
            context_name,
            kubeconfig.flatten(),
            config.id.unwrap_or_default(),
            config.workload_type.clone().unwrap_or_default(),
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
                    _ => {
                        error!("Unsupported protocol: {protocol}");
                        Err(anyhow::anyhow!("Unsupported protocol: {}", protocol))
                    }
                };

                match forward_result {
                    Ok((actual_local_port, handle)) => {
                        info!(
                            "{} port forwarding is set up on local port: {:?} for {}: {:?}",
                            protocol.to_uppercase(),
                            actual_local_port,
                            if config.workload_type.as_deref() == Some("pod") {
                                "pod label"
                            } else {
                                "service"
                            },
                            &config.service
                        );

                        debug!("Port forwarding details: {port_forward:?}");
                        debug!("Actual local port: {actual_local_port:?}");

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
                            if let Some(service_name) = &config.service {
                                if let Some(local_address) = &config.local_address {
                                    match local_address.parse::<std::net::IpAddr>() {
                                        Ok(ip_addr) => {
                                            let entry_id =
                                                format!("{}", config.id.unwrap_or_default());

                                            let host_entry = HostEntry {
                                                ip: ip_addr,
                                                hostname: config.alias.clone().unwrap_or_default(),
                                            };

                                            if let Err(e) = add_host_entry(entry_id, host_entry) {
                                                let error_message = format!(
                                                    "Failed to write to the hostfile for {service_name}: {e}"
                                                );
                                                error!("{}", &error_message);
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
                                                "Invalid IP address format: {local_address}"
                                            );
                                            warn!("{}", &warning_message);
                                            errors.push(warning_message);
                                        }
                                    }
                                }
                            }
                        }

                        let config_state = ConfigState {
                            id: None,
                            config_id: config.id.unwrap(),
                            is_running: true,
                        };
                        if let Err(e) = update_config_state(&config_state).await {
                            error!("Failed to update config state: {e}");
                        }

                        responses.push(CustomResponse {
                            id: config.id,
                            service: config.service.clone().unwrap(),
                            namespace: namespace.clone(),
                            local_port: actual_local_port,
                            remote_port: config.remote_port.unwrap_or_default(),
                            context: config.context.clone(),
                            protocol: config.protocol.clone(),
                            stdout: format!(
                                "{} forwarding from 127.0.0.1:{} -> {:?}:{}",
                                protocol.to_uppercase(),
                                actual_local_port,
                                config.remote_port.unwrap_or_default(),
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
                            if config.workload_type.as_deref() == Some("pod") {
                                "pod label"
                            } else {
                                "service"
                            },
                            config.service.clone().unwrap_or_default(),
                            e
                        );
                        error!("{}", &error_message);
                        errors.push(error_message);
                    }
                }
            }
            Err(e) => {
                let error_message = format!(
                    "Failed to create PortForward for {} {}: {}",
                    if config.workload_type.as_deref() == Some("pod") {
                        "pod label"
                    } else {
                        "service"
                    },
                    config.service.clone().unwrap_or_default(),
                    e
                );
                error!("{}", &error_message);
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

    Ok(responses)
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;
    use std::sync::Arc;

    use kftray_http_logs::HttpLogState;

    use super::*;

    fn setup_test_config() -> Config {
        Config {
            id: Some(1),
            context: "test-context".to_string(),
            kubeconfig: None,
            namespace: "test-namespace".to_string(),
            service: Some("test-service".to_string()),
            alias: Some("test-alias".to_string()),
            local_port: Some(0),
            remote_port: Some(8080),
            protocol: "tcp".to_string(),
            workload_type: Some("service".to_string()),
            target: None,
            local_address: None,
            remote_address: None,
            domain_enabled: None,
        }
    }

    fn setup_pod_config() -> Config {
        let mut config = setup_test_config();
        config.workload_type = Some("pod".to_string());
        config.target = Some("app=test".to_string());
        config
    }

    fn setup_config_with_domain() -> Config {
        let mut config = setup_test_config();
        config.domain_enabled = Some(true);
        config.local_address = Some("127.0.0.1".to_string());
        config
    }

    fn setup_config_with_invalid_ip() -> Config {
        let mut config = setup_test_config();
        config.domain_enabled = Some(true);
        config.local_address = Some("invalid-ip".to_string());
        config
    }

    async fn test_protocol_validation(protocol: &str) -> Result<(), String> {
        match protocol {
            "tcp" | "udp" => Ok(()),
            _ => Err(format!("Unsupported protocol: {protocol}")),
        }
    }

    #[tokio::test]
    async fn test_start_port_forward_empty_configs() {
        let configs = Vec::new();
        let http_log_state = Arc::new(HttpLogState::new());

        let result = start_port_forward(configs, "tcp", http_log_state).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_start_port_forward_invalid_protocol() {
        let result = test_protocol_validation("invalid").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Unsupported protocol: invalid"));
    }

    #[tokio::test]
    async fn test_start_port_forward_with_pod_label() {
        let configs = vec![setup_pod_config()];
        let http_log_state = Arc::new(HttpLogState::new());

        let result = start_port_forward(configs, "tcp", http_log_state).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_start_port_forward_with_domain_enabled() {
        let configs = vec![setup_config_with_domain()];
        let http_log_state = Arc::new(HttpLogState::new());

        let result = start_port_forward(configs, "tcp", http_log_state).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_start_port_forward_with_invalid_ip() {
        let configs = vec![setup_config_with_invalid_ip()];
        let http_log_state = Arc::new(HttpLogState::new());

        let result = start_port_forward(configs, "tcp", http_log_state).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_port_selector_creation() {
        let config = setup_test_config();
        let selector = match config.workload_type.as_deref() {
            Some("pod") => TargetSelector::PodLabel(config.target.clone().unwrap_or_default()),
            _ => TargetSelector::ServiceName(config.service.clone().unwrap_or_default()),
        };

        match selector {
            TargetSelector::ServiceName(name) => {
                assert_eq!(name, "test-service");
            }
            TargetSelector::PodLabel(_) => {
                panic!("Should be ServiceName selector");
            }
        }

        let config = setup_pod_config();
        let selector = match config.workload_type.as_deref() {
            Some("pod") => TargetSelector::PodLabel(config.target.clone().unwrap_or_default()),
            _ => TargetSelector::ServiceName(config.service.clone().unwrap_or_default()),
        };

        match selector {
            TargetSelector::PodLabel(label) => {
                assert_eq!(label, "app=test");
            }
            TargetSelector::ServiceName(_) => {
                panic!("Should be PodLabel selector");
            }
        }
    }

    #[test]
    fn test_host_entry_creation() {
        let config = setup_config_with_domain();
        let _service_name = config.service.as_ref().unwrap();
        let local_address = config.local_address.as_ref().unwrap();
        let ip_addr = local_address.parse::<IpAddr>().unwrap();

        let entry_id = format!("{}", config.id.unwrap_or_default());
        let host_entry = HostEntry {
            ip: ip_addr,
            hostname: config.alias.clone().unwrap_or_default(),
        };

        assert_eq!(host_entry.ip.to_string(), "127.0.0.1");
        assert_eq!(host_entry.hostname, "test-alias");
        assert_eq!(entry_id, "1");
    }
}

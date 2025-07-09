use std::sync::Arc;

use kftray_commons::{
    models::{
        config_model::Config,
        config_state_model::ConfigState,
        hostfile::HostEntry,
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
use once_cell::sync::Lazy;
use tokio::sync::Mutex as TokioMutex;

use crate::{
    hostsfile::add_host_entry,
    kube::models::{
        Port,
        PortForward,
        Target,
        TargetSelector,
    },
    port_forward::CHILD_PROCESSES,
};

static FALLBACK_ALLOCATION_MUTEX: Lazy<TokioMutex<()>> = Lazy::new(|| TokioMutex::new(()));

async fn allocate_local_address_for_config(config: &mut Config) -> Result<String, String> {
    if !config.auto_loopback_address {
        let address = config
            .local_address
            .clone()
            .unwrap_or_else(|| "127.0.0.1".to_string());

        if crate::network_utils::is_custom_loopback_address(&address) {
            info!("Configuring custom loopback address: {address}");
            if let Err(config_err) = crate::network_utils::ensure_loopback_address(&address).await {
                let error_msg = config_err.to_string();
                if error_msg.contains("cancelled") || error_msg.contains("canceled") {
                    return Err(format!(
                        "Custom loopback address configuration cancelled: {error_msg}"
                    ));
                }
                warn!("Failed to configure custom loopback address {address}: {config_err}");
            }
        }

        return Ok(address);
    }

    let service_name = config
        .service
        .clone()
        .unwrap_or_else(|| format!("service-{}", config.id.unwrap_or_default()));

    match try_allocate_address(&service_name).await {
        Ok(allocated_address) => {
            info!("Auto-allocated address {allocated_address} for service {service_name}");
            config.local_address = Some(allocated_address.clone());

            info!(
                "Setting config.local_address to {} for config_id {}",
                allocated_address,
                config.id.unwrap_or_default()
            );
            if let Err(e) = save_allocated_address_to_db(config).await {
                error!(
                    "Failed to save allocated address {} to database for config {}: {}",
                    allocated_address,
                    config.id.unwrap_or_default(),
                    e
                );
            } else {
                info!(
                    "Successfully updated database with allocated address {} for config {}",
                    allocated_address,
                    config.id.unwrap_or_default()
                );
            }

            Ok(allocated_address)
        }
        Err(e) => {
            warn!("Failed to auto-allocate address for service {service_name} via helper: {e}. Trying fallback allocation");

            match try_fallback_allocate_and_save(&service_name, config).await {
                Ok(allocated_address) => {
                    info!(
                        "Fallback-allocated address {allocated_address} for service {service_name}"
                    );
                    Ok(allocated_address)
                }
                Err(fallback_err) => {
                    if fallback_err.contains("cancelled") || fallback_err.contains("canceled") {
                        error!("Address allocation cancelled by user: {fallback_err}");
                        return Err(fallback_err);
                    }

                    warn!("Fallback allocation also failed for service {service_name}: {fallback_err}. Using default 127.0.0.1");
                    let default_address = "127.0.0.1".to_string();
                    config.local_address = Some(default_address.clone());
                    Ok(default_address)
                }
            }
        }
    }
}

async fn try_allocate_address(service_name: &str) -> Result<String, String> {
    let app_id = "com.kftray.app".to_string();

    let socket_path =
        kftray_helper::communication::get_default_socket_path().map_err(|e| e.to_string())?;

    if !kftray_helper::client::socket_comm::is_socket_available(&socket_path) {
        return Err("Helper service is not available".to_string());
    }

    let command = kftray_helper::messages::RequestCommand::Address(
        kftray_helper::messages::AddressCommand::Allocate {
            service_name: service_name.to_string(),
        },
    );

    match kftray_helper::client::socket_comm::send_request(&socket_path, &app_id, command) {
        Ok(response) => match response.result {
            kftray_helper::messages::RequestResult::StringSuccess(address) => Ok(address),
            kftray_helper::messages::RequestResult::Error(error) => Err(error),
            _ => Err("Unexpected response format".to_string()),
        },
        Err(e) => Err(e.to_string()),
    }
}

async fn try_fallback_allocate_and_save(
    service_name: &str, config: &mut Config,
) -> Result<String, String> {
    let _lock = FALLBACK_ALLOCATION_MUTEX.lock().await;

    debug!("Acquired fallback allocation lock for service: {service_name}");

    let allocated_addresses = get_allocated_loopback_addresses().await;

    for octet in 2..255 {
        let address = format!("127.0.0.{octet}");

        if allocated_addresses.contains(&address) {
            debug!("Address {address} already allocated to another config, skipping");
            continue;
        }

        if crate::network_utils::is_address_accessible(&address).await {
            debug!("Address {address} is already in use on system, skipping");
            continue;
        }

        match crate::network_utils::ensure_loopback_address(&address).await {
            Ok(_) => {
                debug!("Successfully allocated and configured fallback address: {address} for service: {service_name}");

                config.local_address = Some(address.clone());
                info!(
                    "Setting config.local_address to {} (fallback) for config_id {}",
                    address,
                    config.id.unwrap_or_default()
                );

                if let Err(e) = save_allocated_address_to_db(config).await {
                    error!("Failed to save fallback allocated address {} to database for config {}: {}", address, config.id.unwrap_or_default(), e);
                } else {
                    info!("Successfully updated database with fallback allocated address {} for config {}", address, config.id.unwrap_or_default());
                }

                return Ok(address);
            }
            Err(e) => {
                let error_msg = e.to_string();
                debug!("Failed to configure fallback address {address}: {error_msg}");

                if error_msg.contains("User cancelled")
                    || error_msg.contains("user cancelled")
                    || error_msg.contains("cancelled")
                    || error_msg.contains("User canceled")
                    || error_msg.contains("canceled")
                {
                    return Err(format!("Address allocation cancelled by user: {error_msg}"));
                }

                continue;
            }
        }
    }

    Err("No available addresses found in fallback allocation".to_string())
}

async fn get_allocated_loopback_addresses() -> std::collections::HashSet<String> {
    use std::collections::HashSet;

    let mut allocated = HashSet::new();

    if let Ok(configs) = kftray_commons::config::get_configs().await {
        for config in configs {
            if let Some(addr) = &config.local_address {
                if crate::network_utils::is_custom_loopback_address(addr)
                    && config.auto_loopback_address
                {
                    allocated.insert(addr.clone());
                    debug!(
                        "Found allocated address {} for config {}",
                        addr,
                        config.id.unwrap_or_default()
                    );
                }
            }
        }
    }

    debug!("Currently allocated loopback addresses: {allocated:?}");
    allocated
}

async fn save_allocated_address_to_db(config: &Config) -> Result<(), String> {
    use kftray_commons::utils::config::update_config;

    match update_config(config.clone()).await {
        Ok(_) => {
            info!(
                "Successfully saved allocated address to database for config {}",
                config.id.unwrap_or_default()
            );
            Ok(())
        }
        Err(e) => {
            error!("Failed to update config in database: {e}");
            Err(e)
        }
    }
}

pub async fn start_port_forward(
    mut configs: Vec<Config>, protocol: &str, http_log_state: Arc<HttpLogState>,
) -> Result<Vec<CustomResponse>, String> {
    let mut responses = Vec::new();
    let mut errors = Vec::new();
    let mut child_handles = Vec::new();

    for config in configs.iter_mut() {
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

        let final_local_address = match allocate_local_address_for_config(config).await {
            Ok(address) => address,
            Err(e) => {
                error!("Failed to allocate local address: {e}");
                return Err(format!("Address allocation failed: {e}"));
            }
        };

        if config.domain_enabled.unwrap_or_default() {
            if let Some(service_name) = &config.service {
                match final_local_address.parse::<std::net::IpAddr>() {
                    Ok(ip_addr) => {
                        let entry_id = format!("{}", config.id.unwrap_or_default());
                        let host_entry = HostEntry {
                            ip: ip_addr,
                            hostname: config.alias.clone().unwrap_or_default(),
                        };

                        if let Err(e) = add_host_entry(entry_id, host_entry) {
                            let error_message = format!(
                                "Failed to write to the hostfile for {service_name}: {e}. Domain alias feature requires hostfile access."
                            );
                            error!("{}", &error_message);
                            errors.push(error_message);
                            continue;
                        }
                    }
                    Err(_) => {
                        let error_message = format!(
                            "Invalid IP address format for domain alias: {final_local_address}"
                        );
                        error!("{}", &error_message);
                        errors.push(error_message);
                        continue;
                    }
                }
            }
        }

        let local_address_clone = Some(final_local_address);

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
            auto_loopback_address: false,
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

    #[tokio::test]
    async fn test_allocate_local_address_for_config_disabled() {
        let mut config = setup_test_config();
        config.auto_loopback_address = false;
        config.local_address = Some("192.168.1.1".to_string());

        let result = allocate_local_address_for_config(&mut config)
            .await
            .unwrap();
        assert_eq!(result, "192.168.1.1");
        assert_eq!(config.local_address, Some("192.168.1.1".to_string()));
    }

    // Mock function for testing address allocation
    async fn mock_allocate_local_address_for_config(config: &mut Config) -> String {
        if !config.auto_loopback_address {
            return config
                .local_address
                .clone()
                .unwrap_or_else(|| "127.0.0.1".to_string());
        }

        // Mock allocation - return predictable address based on service name
        let service_name = config
            .service
            .clone()
            .unwrap_or_else(|| format!("service-{}", config.id.unwrap_or_default()));

        let mock_address = format!("127.0.0.{}", 100 + (service_name.len() % 155));
        config.local_address = Some(mock_address.clone());
        mock_address
    }

    #[tokio::test]
    async fn test_allocate_local_address_for_config_mocked() {
        let mut config = setup_test_config();
        config.auto_loopback_address = true;
        config.local_address = None;

        let result = mock_allocate_local_address_for_config(&mut config).await;

        assert!(result.starts_with("127.0.0."));
        assert_ne!(result, "127.0.0.1");
        assert_eq!(config.local_address, Some(result.clone()));

        // Test deterministic behavior
        let mut config2 = setup_test_config();
        config2.auto_loopback_address = true;
        config2.local_address = None;
        let result2 = mock_allocate_local_address_for_config(&mut config2).await;
        assert_eq!(result, result2);
    }
}

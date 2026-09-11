use std::sync::Arc;

use anyhow::Result;
use dashmap::DashSet;
use futures::stream::{
    self,
    StreamExt,
};
use kftray_commons::{
    models::{
        config_model::Config,
        config_state_model::ConfigState,
        hostfile::HostEntry,
        response::CustomResponse,
    },
    utils::{
        config_state::update_config_state_with_mode,
        db_mode::DatabaseMode,
        settings::get_app_settings,
        timeout_manager::start_timeout_for_forward,
    },
};
use log::{
    debug,
    error,
    info,
    warn,
};
use once_cell::sync::Lazy;
use tokio::sync::Mutex as TokioMutex;

use crate::{
    hostsfile::{
        add_host_entry,
        add_ssl_host_entry,
    },
    kube::models::{
        Port,
        PortForward,
        Target,
        TargetSelector,
    },
    port_forward::CHILD_PROCESSES,
};

pub static STOPPED_BY_TIMEOUT: Lazy<DashSet<i64>> = Lazy::new(DashSet::new);

pub fn clear_stopped_by_timeout(config_id: i64) {
    STOPPED_BY_TIMEOUT.remove(&config_id);
}

pub fn is_stopped_by_timeout(config_id: i64) -> bool {
    STOPPED_BY_TIMEOUT.contains(&config_id)
}

pub async fn cleanup_stale_timeout_entries() {
    use kftray_commons::utils::config::get_configs;

    if let Ok(configs) = get_configs().await {
        let valid_ids: std::collections::HashSet<i64> =
            configs.iter().filter_map(|c| c.id).collect();

        STOPPED_BY_TIMEOUT.retain(|id| valid_ids.contains(id));
        debug!(
            "Cleaned up stale timeout entries, {} remaining",
            STOPPED_BY_TIMEOUT.len()
        );
    }
}

async fn handle_timeout_callback(id: i64, mode: DatabaseMode) {
    info!("User-configured timeout reached for config {id}, stopping port forward");

    STOPPED_BY_TIMEOUT.insert(id);

    if let Err(e) = crate::kube::stop::stop_port_forward_with_mode(id.to_string(), mode).await {
        error!("Failed to stop port forward {id} on timeout: {e}");
        STOPPED_BY_TIMEOUT.remove(&id);
    } else {
        info!("Port forward {id} stopped due to user-configured timeout");
    }
}

fn create_static_timeout_callback(mode: DatabaseMode) -> Arc<dyn Fn(i64) + Send + Sync> {
    Arc::new(move |id: i64| {
        tokio::spawn(async move {
            handle_timeout_callback(id, mode).await;
        });
    })
}

async fn build_tls_acceptor(
    _config: &Config, settings: &kftray_commons::models::settings_model::AppSettings,
) -> Result<tokio_rustls::TlsAcceptor> {
    crate::ssl::ensure_crypto_provider_installed();

    let cert_manager = crate::ssl::CertificateManager::new(settings)?;
    let cert_pair = cert_manager.load_global_certificate().await?;

    let server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            cert_pair.certificate.clone(),
            cert_pair.private_key.clone_key(),
        )
        .map_err(|e| anyhow::anyhow!("Failed to create server config with certificate: {}", e))?;

    Ok(tokio_rustls::TlsAcceptor::from(Arc::new(server_config)))
}

async fn update_hosts_with_ssl(config: &Config) -> Result<(), String> {
    let alias = config
        .alias
        .as_ref()
        .ok_or("Alias required for SSL hosts entry")?;

    let config_id = &config.id.unwrap_or(-1).to_string();
    let port = config.local_port.unwrap_or(8080);

    add_ssl_host_entry(config_id, alias, port)
        .map_err(|e| format!("Failed to add HTTPS hosts entries: {}", e))?;

    Ok(())
}

fn workload_type_description(workload_type: Option<&str>) -> &'static str {
    match workload_type {
        Some("pod") => "pod label",
        Some("proxy") => "proxy pod",
        _ => "service",
    }
}

static FALLBACK_ALLOCATION_MUTEX: Lazy<TokioMutex<()>> = Lazy::new(|| TokioMutex::new(()));

async fn allocate_local_address_for_config(
    config: &mut Config, mode: DatabaseMode,
) -> Result<String, String> {
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

    let service = service_name.clone();
    let allocation = tokio::task::spawn_blocking(move || try_allocate_address(&service))
        .await
        .map_err(|error| error.to_string())
        .and_then(|result| result);
    match allocation {
        Ok(allocated_address) => {
            info!("Auto-allocated address {allocated_address} for service {service_name}");
            config.local_address = Some(allocated_address.clone());

            info!(
                "Setting config.local_address to {} for config_id {}",
                allocated_address,
                config.id.unwrap_or_default()
            );
            if let Err(e) = save_allocated_address_to_db(config, mode).await {
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
            warn!(
                "Failed to auto-allocate address for service {service_name} via helper: {e}. Trying fallback allocation"
            );

            match try_fallback_allocate_and_save(&service_name, config, mode).await {
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

                    warn!(
                        "Fallback allocation also failed for service {service_name}: {fallback_err}. Using default 127.0.0.1"
                    );
                    let default_address = "127.0.0.1".to_string();
                    config.local_address = Some(default_address.clone());
                    Ok(default_address)
                }
            }
        }
    }
}

fn try_allocate_address(service_name: &str) -> Result<String, String> {
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
    service_name: &str, config: &mut Config, mode: DatabaseMode,
) -> Result<String, String> {
    let _lock = FALLBACK_ALLOCATION_MUTEX.lock().await;

    debug!("Acquired fallback allocation lock for service: {service_name}");

    let allocated_addresses = get_allocated_loopback_addresses(mode).await;

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
                debug!(
                    "Successfully allocated and configured fallback address: {address} for service: {service_name}"
                );

                config.local_address = Some(address.clone());
                info!(
                    "Setting config.local_address to {} (fallback) for config_id {}",
                    address,
                    config.id.unwrap_or_default()
                );

                match save_allocated_address_to_db(config, mode).await {
                    Ok(_) => {
                        info!(
                            "Successfully updated database with fallback allocated address {} for config {}",
                            address,
                            config.id.unwrap_or_default()
                        );
                        return Ok(address);
                    }
                    Err(e) => {
                        error!(
                            "Failed to save fallback allocated address {} to database for config {}: {}",
                            address,
                            config.id.unwrap_or_default(),
                            e
                        );
                        if let Err(cleanup_err) =
                            crate::network_utils::remove_loopback_address(&address).await
                        {
                            error!(
                                "Failed to cleanup address {} after DB save failure: {}",
                                address, cleanup_err
                            );
                        }
                        continue;
                    }
                }
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

async fn get_allocated_loopback_addresses(mode: DatabaseMode) -> std::collections::HashSet<String> {
    use std::collections::HashSet;

    let mut allocated = HashSet::new();

    if let Ok(configs) = kftray_commons::config::get_configs_with_mode(mode).await {
        for config in configs {
            if let Some(addr) = &config.local_address
                && crate::network_utils::is_custom_loopback_address(addr)
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

    debug!("Currently allocated loopback addresses: {allocated:?}");
    allocated
}

async fn save_allocated_address_to_db(config: &Config, mode: DatabaseMode) -> Result<(), String> {
    use kftray_commons::utils::config::update_config_with_mode;

    match update_config_with_mode(config.clone(), mode).await {
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
    configs: Vec<Config>, protocol: &str,
) -> Result<Vec<CustomResponse>, String> {
    start_port_forward_with_mode(configs, protocol, DatabaseMode::File, false).await
}

pub(super) async fn start_config(
    mut config: Config, protocol: &str, mode: DatabaseMode, ssl_override: bool,
) -> Result<CustomResponse, String> {
    let config_id = config.id.ok_or("Config has no ID")?;
    if !matches!(protocol, "tcp" | "udp") {
        return Err(format!("Unsupported protocol: {protocol}"));
    }
    if config.auto_loopback_address || config.local_address.is_none() {
        allocate_local_address_for_config(&mut config, mode).await?;
    }
    if let Some(config_id) = config.id {
        clear_stopped_by_timeout(config_id);
    }

    let selector = match (config.workload_type.as_deref(), config.protocol.as_str()) {
        (Some("pod"), "tcp") => TargetSelector::PodLabel(config.target.clone().unwrap_or_default()),
        (Some("pod"), "udp") => TargetSelector::PodLabel(config.target.clone().unwrap_or_default()),
        (Some("service"), "tcp") => {
            TargetSelector::ServiceName(config.service.clone().unwrap_or_default())
        }
        (Some("service"), "udp") => TargetSelector::PodLabel(format!(
            "app={},config_id={}",
            config.service.clone().unwrap_or_default(),
            config.id.unwrap_or_default()
        )),
        (Some("proxy"), "udp") => TargetSelector::PodLabel(format!(
            "app={},config_id={}",
            config.service.clone().unwrap_or_default(),
            config.id.unwrap_or_default()
        )),
        (Some("proxy"), "tcp") => TargetSelector::PodLabel(format!(
            "app={},config_id={}",
            config.service.clone().unwrap_or_default(),
            config.id.unwrap_or_default()
        )),
        _ => TargetSelector::ServiceName(config.service.clone().unwrap_or_default()),
    };

    let remote_port = Port::from(config.remote_port.unwrap_or_default() as i32);
    let context_name = Some(config.context.clone());
    let kubeconfig = Some(config.kubeconfig.clone());
    let namespace = config.namespace.clone();
    let target = Target::new(selector, remote_port, namespace.clone());

    debug!("Remote Port: {:?}", config.remote_port);
    debug!("Local Port: {:?}", config.local_port);

    match config.workload_type.as_deref() {
        Some("pod") => info!("Attempting to forward to pod label: {:?}", config.target),
        Some("proxy") => info!("Attempting to forward to proxy pod: {:?}", config.service),
        _ => info!("Attempting to forward to service: {:?}", config.service),
    }

    let final_local_address = config
        .local_address
        .clone()
        .unwrap_or_else(|| "127.0.0.1".to_string());

    if config.domain_enabled.unwrap_or_default()
        && let Some(service_name) = &config.service
    {
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
                    error!("{}", error_message);
                    return Err(error_message);
                }
            }
            Err(_) => {
                let error_message =
                    format!("Invalid IP address format for domain alias: {final_local_address}");
                error!("{}", error_message);
                return Err(error_message);
            }
        }
    }

    let local_address_clone = Some(final_local_address);

    let settings = get_app_settings().await.ok();
    let should_use_ssl = (settings
        .as_ref()
        .is_some_and(|settings| settings.ssl_enabled)
        || ssl_override)
        && config.alias.is_some();

    let actual_config = config.clone();

    let port_forward = PortForward::new(
        target,
        actual_config.local_port,
        local_address_clone,
        context_name.clone().flatten(),
        kubeconfig.flatten(),
        actual_config.id.unwrap_or_default(),
        actual_config.workload_type.clone().unwrap_or_default(),
    );

    let tls_acceptor = if protocol == "tcp" && should_use_ssl {
        if let Some(settings) = &settings {
            match build_tls_acceptor(&actual_config, settings).await {
                Ok(acceptor) => Some(acceptor),
                Err(e) => {
                    warn!("Failed to create TLS acceptor: {}", e);
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    let forward_result = match protocol {
        "udp" => port_forward.clone().port_forward_udp().await,
        "tcp" => port_forward.clone().port_forward_tcp(tls_acceptor).await,
        _ => {
            error!("Unsupported protocol: {protocol}");
            Err(anyhow::anyhow!("Unsupported protocol: {}", protocol))
        }
    };

    match forward_result {
        Ok((actual_local_port, handle)) => {
            let protocol_upper = protocol.to_uppercase();
            info!(
                "{} port forwarding is set up on local port: {:?} for {}: {:?}",
                protocol_upper,
                actual_local_port,
                workload_type_description(config.workload_type.as_deref()),
                config.service
            );

            debug!(
                "Port forwarding established for config_id: {}",
                port_forward.config_id
            );
            debug!("Actual local port: {actual_local_port}");

            let config_state = ConfigState::new(config_id, true);
            if let Err(error) = update_config_state_with_mode(&config_state, mode).await {
                handle.cleanup_and_abort().await;
                let _ = port_forward.cleanup_resources().await;
                return Err(error);
            }

            CHILD_PROCESSES.insert(config_id, handle);
            let timeout_callback = create_static_timeout_callback(mode);

            if let Err(e) = start_timeout_for_forward(config_id, timeout_callback).await {
                error!("Failed to start timeout for config {config_id}: {e}");
            }

            if should_use_ssl
                && protocol == "tcp"
                && let Err(e) = update_hosts_with_ssl(&config).await
            {
                warn!("Failed to update hosts file for SSL: {}", e);
            }

            let target_name =
                config
                    .service
                    .as_deref()
                    .unwrap_or_else(|| match &port_forward.target.selector {
                        TargetSelector::ServiceName(name) | TargetSelector::PodLabel(name) => name,
                    });

            Ok(CustomResponse {
                id: config.id,
                service: target_name.to_owned(),
                namespace: namespace.clone(),
                local_port: actual_local_port,
                remote_port: config.remote_port.unwrap_or_default(),
                context: config.context.clone().unwrap_or_default(),
                protocol: config.protocol.clone(),
                stdout: {
                    let protocol_display = if should_use_ssl && protocol == "tcp" {
                        "HTTPS".to_string()
                    } else {
                        protocol.to_uppercase()
                    };
                    format!(
                        "{} forwarding from 127.0.0.1:{} -> {:?}:{}{}",
                        protocol_display,
                        actual_local_port,
                        config.remote_port.unwrap_or_default(),
                        target_name,
                        if should_use_ssl && protocol == "tcp" {
                            " (HTTP redirects to HTTPS)"
                        } else {
                            ""
                        }
                    )
                },
                stderr: String::new(),
                status: 0,
            })
        }
        Err(e) => {
            let protocol_upper = protocol.to_uppercase();
            let error_message = format!(
                "Failed to start {} port forwarding for {} {}: {}",
                protocol_upper,
                workload_type_description(config.workload_type.as_deref()),
                config.service.clone().unwrap_or_default(),
                e
            );
            error!("{}", error_message);

            if let Err(cleanup_err) = port_forward.cleanup_resources().await {
                error!(
                    "Failed to cleanup resources for failed port forward: {}",
                    cleanup_err
                );
            }

            Err(error_message)
        }
    }
}

pub(super) async fn start_config_locked(
    config: Config, protocol: &str, mode: DatabaseMode, ssl_override: bool,
) -> Result<CustomResponse, String> {
    let id = config.id.ok_or("Config has no ID")?;
    let lock = crate::kube::proxy_recovery::acquire_recovery_lock(id).await;
    let guard = lock.lock().await;
    let result = if CHILD_PROCESSES.contains_key(&id) {
        Err(format!(
            "Port forwarding is already running for config {id}"
        ))
    } else if config.workload_type.as_deref() == Some("expose") {
        crate::expose::start_single_expose(config, mode).await
    } else {
        start_config(config, protocol, mode, ssl_override).await
    };
    drop(guard);
    drop(lock);
    crate::kube::proxy_recovery::remove_recovery_lock(id);
    result
}

pub async fn start_port_forward_with_mode(
    configs: Vec<Config>, protocol: &str, mode: DatabaseMode, ssl_override: bool,
) -> Result<Vec<CustomResponse>, String> {
    let mut futures = stream::iter(configs)
        .map(|config| start_config_locked(config, protocol, mode, ssl_override))
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
        Err(errors.join("\n"))
    } else {
        for error in errors {
            warn!("Partial failure: {error}");
        }
        Ok(responses)
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    fn setup_test_config() -> Config {
        Config {
            id: Some(1),
            context: Some("test-context".to_string()),
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

    #[tokio::test]
    async fn test_start_port_forward_empty_configs() {
        let configs = Vec::new();

        let result = start_port_forward(configs, "tcp").await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_start_port_forward_invalid_protocol() {
        let result = start_port_forward_with_mode(
            vec![setup_test_config()],
            "invalid",
            DatabaseMode::Memory,
            false,
        )
        .await;
        assert!(result.is_err());
        assert!(!CHILD_PROCESSES.contains_key(&1));
    }

    #[tokio::test]
    async fn test_start_port_forward_with_pod_label() {
        let configs = vec![setup_pod_config()];

        let result = start_port_forward(configs, "tcp").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_start_port_forward_with_domain_enabled() {
        let configs = vec![setup_config_with_domain()];

        let result = start_port_forward(configs, "tcp").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_start_port_forward_with_invalid_ip() {
        let configs = vec![setup_config_with_invalid_ip()];

        let result = start_port_forward(configs, "tcp").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_allocate_local_address_for_config_disabled() {
        let mut config = setup_test_config();
        config.auto_loopback_address = false;
        config.local_address = Some("192.168.1.1".to_string());

        let result = allocate_local_address_for_config(&mut config, DatabaseMode::Memory)
            .await
            .unwrap();
        assert_eq!(result, "192.168.1.1");
        assert_eq!(config.local_address, Some("192.168.1.1".to_string()));
    }

    #[tokio::test]
    async fn rejected_duplicate_start_keeps_the_existing_listener_alive() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let id = 410_031;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let _listener = listener;
            std::future::pending::<anyhow::Result<()>>().await
        });
        CHILD_PROCESSES.insert(
            id,
            crate::port_forward::PortForwardProcess::new(task, id.to_string()),
        );
        let config = Config {
            id: Some(id),
            kubeconfig: Some("/nonexistent/isolated-test-kubeconfig".to_string()),
            ..setup_test_config()
        };
        let result =
            start_port_forward_with_mode(vec![config], "tcp", DatabaseMode::Memory, false).await;
        assert!(result.is_err());
        assert!(tokio::net::TcpListener::bind(address).await.is_err());
        super::super::stop::stop_port_forward_with_mode(id.to_string(), DatabaseMode::Memory)
            .await
            .unwrap();
        let _listener = tokio::net::TcpListener::bind(address).await.unwrap();
    }
}

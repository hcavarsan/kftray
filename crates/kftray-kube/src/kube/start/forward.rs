use std::sync::Arc;

use anyhow::Result;
use kftray_commons::{
    models::config_state_model::ConfigState,
    utils::{
        config_state::update_config_state_with_mode,
        db_mode::DatabaseMode,
        timeout_manager::start_timeout_for_forward,
    },
};
use kftray_commons::{
    models::{
        config_model::Config,
        hostfile::HostEntry,
        response::CustomResponse,
    },
    utils::settings::get_app_settings,
};
use kftray_hosts::hostsfile::{
    add_host_entry,
    add_ssl_host_entry,
};
use log::{
    debug,
    error,
    info,
    warn,
};

use super::timeout::{
    clear_stopped_by_timeout,
    create_static_timeout_callback,
};
use crate::{
    kube::models::{
        Port,
        PortForward,
        Target,
        TargetSelector,
    },
    kube::shared_client::ServiceClientKey,
    port_forward_error::PortForwardError,
    registry::{
        PORT_FORWARD_REGISTRY,
        PortForwardKey,
    },
};

pub(super) async fn build_tls_acceptor(
    _config: &Config, settings: &kftray_commons::models::settings_model::AppSettings,
) -> Result<tokio_rustls::TlsAcceptor> {
    kftray_ssl::ensure_crypto_provider_installed();

    let cert_manager = kftray_ssl::CertificateManager::new(settings)?;
    let cert_pair = cert_manager.load_global_certificate().await?;

    let server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            cert_pair.certificate.clone(),
            cert_pair.private_key.clone_key(),
        )
        .map_err(|e| anyhow::anyhow!("Failed to create server config with certificate: {e}"))?;

    Ok(tokio_rustls::TlsAcceptor::from(Arc::new(server_config)))
}

pub(super) async fn update_hosts_with_ssl(config: &Config) -> Result<(), PortForwardError> {
    let alias = config
        .alias
        .as_ref()
        .ok_or_else(|| PortForwardError::ConfigurationError {
            message: "Alias required for SSL hosts entry".to_string(),
        })?;

    let config_id = &config.id.unwrap_or(-1).to_string();
    let port = config.local_port.unwrap_or(8080);

    add_ssl_host_entry(config_id, alias, port).map_err(|e| {
        PortForwardError::HostsFile(format!("Failed to add HTTPS hosts entries: {e}"))
    })?;

    Ok(())
}

pub(super) fn workload_type_description(workload_type: Option<&str>) -> &'static str {
    match workload_type {
        Some("pod") => "pod label",
        Some("proxy") => "proxy pod",
        _ => "service",
    }
}

pub(super) enum SingleConfigResult {
    Success(CustomResponse),
    Error {
        message: String,
        failed_handle: Option<String>,
    },
}

pub(super) async fn process_single_config_with_address(
    config: Config, protocol: String, mode: DatabaseMode, ssl_override: bool,
) -> SingleConfigResult {
    if let Some(config_id) = config.id {
        clear_stopped_by_timeout(config_id);
    }

    let pod_label = || {
        format!(
            "app={},config_id={}",
            config.service.clone().unwrap_or_default(),
            config.id.unwrap_or_default()
        )
    };

    let selector = match (config.workload_type.as_deref(), config.protocol.as_str()) {
        (Some("pod"), _) => TargetSelector::PodLabel(config.target.clone().unwrap_or_default()),
        (Some("service"), "tcp") => {
            TargetSelector::ServiceName(config.service.clone().unwrap_or_default())
        }
        (Some("service"), "udp") | (Some("proxy"), _) => TargetSelector::PodLabel(pod_label()),
        _ => TargetSelector::ServiceName(config.service.clone().unwrap_or_default()),
    };

    let remote_port = Port::from(i32::from(config.remote_port.unwrap_or_default()));
    let context_name = Some(config.context.clone());
    let kubeconfig = Some(config.kubeconfig.clone());
    let namespace = config.namespace.clone();
    let target = Target::new(selector, remote_port, namespace.clone());

    debug!("Remote Port: {:?}", config.remote_port);
    debug!("Local Port: {:?}", config.local_port);

    match config.workload_type.as_deref() {
        Some("pod") => info!("Attempting to forward to pod label: {:?}", &config.target),
        Some("proxy") => info!("Attempting to forward to proxy pod: {:?}", &config.service),
        _ => info!("Attempting to forward to service: {:?}", &config.service),
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
                    error!("{}", &error_message);
                    return SingleConfigResult::Error {
                        message: error_message,
                        failed_handle: None,
                    };
                }
            }
            Err(_) => {
                let error_message =
                    format!("Invalid IP address format for domain alias: {final_local_address}");
                error!("{}", &error_message);
                return SingleConfigResult::Error {
                    message: error_message,
                    failed_handle: None,
                };
            }
        }
    }

    let local_address_clone = Some(final_local_address);

    let should_use_ssl = if let Ok(settings) = get_app_settings().await {
        (settings.ssl_enabled || ssl_override) && config.alias.is_some()
    } else {
        ssl_override && config.alias.is_some()
    };

    let actual_config = config.clone();

    let port_forward_result: Result<PortForward, anyhow::Error> = PortForward::new(
        target,
        actual_config.local_port,
        local_address_clone,
        context_name.clone().flatten(),
        kubeconfig.flatten(),
        actual_config.id.unwrap_or_default(),
        actual_config.workload_type.clone().unwrap_or_default(),
    )
    .await;

    match port_forward_result {
        Ok(port_forward) => {
            let tls_acceptor = if protocol == "tcp" && should_use_ssl {
                match get_app_settings().await {
                    Ok(settings) => match build_tls_acceptor(&actual_config, &settings).await {
                        Ok(acceptor) => Some(acceptor),
                        Err(e) => {
                            warn!("Failed to create TLS acceptor: {e}");
                            None
                        }
                    },
                    Err(e) => {
                        warn!("Failed to get app settings for SSL: {e}");
                        None
                    }
                }
            } else {
                None
            };

            let forward_result = match protocol.as_str() {
                "udp" => port_forward.clone().port_forward_udp().await,
                "tcp" => port_forward.clone().port_forward_tcp(tls_acceptor).await,
                _ => {
                    error!("Unsupported protocol: {protocol}");
                    Err(anyhow::anyhow!("Unsupported protocol: {protocol}"))
                }
            };

            match forward_result {
                Ok((actual_local_port, handle)) => {
                    let config_id = config
                        .id
                        .ok_or_else(|| "config missing id".to_string())
                        .map_err(|e| SingleConfigResult::Error {
                            message: e,
                            failed_handle: None,
                        });
                    let config_id = match config_id {
                        Ok(id) => id,
                        Err(err) => return err,
                    };
                    let service_name = config.service.clone().unwrap_or_default();

                    let protocol_upper = protocol.to_uppercase();
                    info!(
                        "{} port forwarding is set up on local port: {:?} for {}: {:?}",
                        protocol_upper,
                        actual_local_port,
                        workload_type_description(config.workload_type.as_deref()),
                        &config.service
                    );

                    debug!(
                        "Port forwarding established for config_id: {}",
                        port_forward.config_id
                    );
                    debug!("Actual local port: {actual_local_port}");

                    let pf_key = PortForwardKey::named(config_id, &service_name);
                    let client_key =
                        ServiceClientKey::new(config.context.clone(), config.kubeconfig.clone());

                    PORT_FORWARD_REGISTRY.insert_process(pf_key.clone(), handle, client_key);

                    let config_state = ConfigState::new(config_id, true);
                    if let Err(e) = update_config_state_with_mode(&config_state, mode).await {
                        error!("Failed to update config state: {e}");
                    }

                    let timeout_callback = create_static_timeout_callback();

                    if let Err(e) = start_timeout_for_forward(config_id, timeout_callback).await {
                        error!("Failed to start timeout for config {config_id}: {e}");
                    }

                    if should_use_ssl
                        && protocol == "tcp"
                        && let Err(e) = update_hosts_with_ssl(&config).await
                    {
                        warn!("Failed to update hosts file for SSL: {e}");
                    }

                    SingleConfigResult::Success(CustomResponse {
                        id: config.id,
                        service: service_name.clone(),
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
                                service_name,
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
                    error!("{}", &error_message);

                    if let Err(cleanup_err) = port_forward.cleanup_resources().await {
                        error!(
                            "Failed to cleanup resources for failed port forward: {cleanup_err}"
                        );
                    }

                    let failed_handle = config.id.map(|config_id| {
                        format!(
                            "config:{}:service:{}",
                            config_id,
                            config.service.clone().unwrap_or_default()
                        )
                    });

                    SingleConfigResult::Error {
                        message: error_message,
                        failed_handle,
                    }
                }
            }
        }
        Err(e) => {
            let error_message = format!(
                "Failed to create PortForward for {} {}: {}",
                workload_type_description(config.workload_type.as_deref()),
                config.service.clone().unwrap_or_default(),
                e
            );
            error!("{}", &error_message);

            if let Some(local_addr) = &config.local_address
                && kftray_hosts::loopback::is_custom_loopback_address(local_addr)
                && let Err(cleanup_err) =
                    kftray_hosts::loopback::remove_loopback_address(local_addr).await
            {
                error!(
                    "Failed to cleanup loopback address {local_addr} after PortForward creation failure: {cleanup_err}"
                );
            }

            let failed_handle = config.id.map(|config_id| {
                format!(
                    "config:{}:service:{}",
                    config_id,
                    config.service.clone().unwrap_or_default()
                )
            });

            SingleConfigResult::Error {
                message: error_message,
                failed_handle,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use kftray_commons::models::{
        config_model::Config,
        hostfile::HostEntry,
    };

    use crate::kube::models::TargetSelector;

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

    #[test]
    fn test_port_selector_creation() {
        let config = setup_test_config();
        let selector = match config.workload_type.as_deref() {
            Some("pod") => TargetSelector::PodLabel(config.target.clone().unwrap_or_default()),
            Some("proxy") => TargetSelector::PodLabel(format!(
                "app={},config_id={}",
                config.service.clone().unwrap_or_default(),
                config.id.unwrap_or_default()
            )),
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
            Some("proxy") => TargetSelector::PodLabel(format!(
                "app={},config_id={}",
                config.service.clone().unwrap_or_default(),
                config.id.unwrap_or_default()
            )),
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

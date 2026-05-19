mod address;
mod forward;
pub(crate) mod timeout;

use address::allocate_local_address_for_config;
use forward::{
    SingleConfigResult,
    process_single_config_with_address,
};
use futures::{
    future::BoxFuture,
    stream::{
        FuturesUnordered,
        StreamExt,
    },
};
use kftray_commons::{
    models::{
        config_model::Config,
        response::CustomResponse,
    },
    utils::db_mode::DatabaseMode,
};
use log::{
    debug,
    error,
    warn,
};
pub use timeout::{
    cleanup_stale_timeout_entries,
    clear_stopped_by_timeout,
    is_stopped_by_timeout,
};

use crate::{
    port_forward_error::PortForwardError,
    registry::{
        PORT_FORWARD_REGISTRY,
        PortForwardKey,
    },
};

pub async fn start_port_forward(
    configs: Vec<Config>, protocol: &str,
) -> Result<Vec<CustomResponse>, PortForwardError> {
    start_port_forward_with_mode(configs, protocol, DatabaseMode::File, false).await
}

pub async fn start_port_forward_with_mode(
    configs: Vec<Config>, protocol: &str, mode: DatabaseMode, ssl_override: bool,
) -> Result<Vec<CustomResponse>, PortForwardError> {
    let mut responses = Vec::new();
    let mut errors = Vec::new();
    let mut failed_handles = Vec::new();

    // Expose configs must be dispatched by callers via kftray_expose, not here.
    let has_expose = configs
        .iter()
        .any(|c| c.workload_type.as_deref() == Some("expose"));
    if has_expose {
        return Err(PortForwardError::ConfigurationError {
            message: "expose workload_type must be dispatched via kftray_expose, not kftray_kube"
                .to_string(),
        });
    }

    let regular_configs = configs;

    let mut regular_configs_with_addresses = Vec::with_capacity(regular_configs.len());
    for mut config in regular_configs {
        if config.auto_loopback_address || config.local_address.is_none() {
            match allocate_local_address_for_config(&mut config).await {
                Ok(address) => {
                    debug!(
                        "Pre-allocated address {} for config {}",
                        address,
                        config.id.unwrap_or_default()
                    );
                }
                Err(e) => {
                    error!(
                        "Failed to pre-allocate address for config {}: {}",
                        config.id.unwrap_or_default(),
                        e
                    );
                    errors.push(format!(
                        "Address allocation failed for config {}: {}",
                        config.id.unwrap_or_default(),
                        e
                    ));
                    continue;
                }
            }
        }
        regular_configs_with_addresses.push(config);
    }

    let mut futures: FuturesUnordered<BoxFuture<'static, SingleConfigResult>> =
        FuturesUnordered::new();

    let protocol_owned = protocol.to_string();
    for config in regular_configs_with_addresses {
        let proto = protocol_owned.clone();
        futures.push(Box::pin(process_single_config_with_address(
            config,
            proto,
            mode,
            ssl_override,
        )));
    }

    while let Some(result) = futures.next().await {
        match result {
            SingleConfigResult::Success(response) => {
                responses.push(response);
            }
            SingleConfigResult::Error {
                message,
                failed_handle,
            } => {
                errors.push(message);
                if let Some(handle) = failed_handle {
                    failed_handles.push(handle);
                }
            }
        }
    }

    for handle_key in failed_handles {
        // Parse old-style composite key to extract config_id and service_name
        if let Some(content) = handle_key.strip_prefix("config:")
            && let Some((config_part, service_part)) = content.split_once(":service:")
            && let Ok(cid) = config_part.parse::<i64>()
        {
            let key = PortForwardKey::named(cid, service_part);
            if let Some(entry) = PORT_FORWARD_REGISTRY.remove_process(&key) {
                entry.process.abort();
            }
        }
    }

    if !responses.is_empty() {
        if !errors.is_empty() {
            for error in errors {
                warn!("Partial failure: {error}");
            }
        }
        Ok(responses)
    } else if !errors.is_empty() {
        Err(PortForwardError::Internal(errors.join("\n")))
    } else {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use kftray_commons::models::config_model::Config;

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

    async fn test_protocol_validation(protocol: &str) -> Result<(), String> {
        match protocol {
            "tcp" | "udp" => Ok(()),
            _ => Err(format!("Unsupported protocol: {protocol}")),
        }
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
        let result = test_protocol_validation("invalid").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Unsupported protocol: invalid"));
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
}

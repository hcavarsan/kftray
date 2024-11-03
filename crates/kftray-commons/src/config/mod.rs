//! Port forwarding configuration management
//!
//! This module provides functionality for managing port forwarding
//! configurations, including validation and preparation for saving to the
//! database.

use serde::{
    Deserialize,
    Serialize,
};

use crate::error::{
    Error,
    Result,
};

mod builder;
pub use builder::ConfigBuilder;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    #[serde(default)]
    pub namespace: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_port: Option<u16>,
    #[serde(default)]
    pub context: String,
    pub workload_type: Option<String>,
    pub protocol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kubeconfig: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            id: None,
            service: Some("default-service".to_string()),
            namespace: "default".to_string(),
            local_port: Some(0),
            remote_port: Some(0),
            context: "default".to_string(),
            workload_type: Some("default-workload".to_string()),
            protocol: "TCP".to_string(),
            remote_address: Some("default-remote".to_string()),
            local_address: Some("127.0.0.1".to_string()),
            alias: Some("default-alias".to_string()),
            domain_enabled: Some(false),
            kubeconfig: Some("default".to_string()),
            target: Some("default-target".to_string()),
        }
    }
}

impl Config {
    pub fn builder() -> ConfigBuilder {
        ConfigBuilder::new()
    }

    pub fn validate(&self) -> Result<()> {
        if let Some(port) = self.local_port {
            if port == 0 {
                return Err(Error::invalid_local_port());
            }
        }

        if self.protocol.is_empty() {
            return Err(Error::empty_protocol());
        }

        if self.namespace.is_empty() {
            return Err(Error::empty_namespace());
        }

        Ok(())
    }

    pub fn prepare_for_save(mut self) -> Self {
        if self.alias.as_deref() == Some("") || self.alias.is_none() {
            let workload_type = self.workload_type.clone().unwrap_or_default();
            let alias = format!(
                "{}-{}-{}",
                workload_type,
                self.protocol,
                self.local_port.unwrap_or_default()
            );
            self.alias = Some(alias);
        }

        if self.kubeconfig.as_deref() == Some("") || self.kubeconfig.is_none() {
            self.kubeconfig = Some("default".to_string());
        }

        if self.local_port == Some(0) || self.local_port.is_none() {
            self.local_port = match portpicker::pick_unused_port() {
                Some(port) => Some(port),
                None => {
                    log::error!("Failed to find unused port, using remote_port as local_port");
                    self.remote_port
                }
            };
        }

        self
    }

    // Getter methods
    pub fn get_id(&self) -> Option<i64> {
        self.id
    }

    pub fn set_id(&mut self, id: i64) {
        self.id = Some(id);
    }

    pub fn get_service_name(&self) -> Option<&str> {
        self.service.as_deref()
    }

    pub fn get_local_port(&self) -> Option<u16> {
        self.local_port
    }

    pub fn get_remote_port(&self) -> Option<u16> {
        self.remote_port
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_validation() {
        // Test valid config
        let valid_config = Config::builder()
            .namespace("test")
            .protocol("TCP")
            .local_port(8080)
            .build()
            .unwrap();
        assert!(valid_config.validate().is_ok());

        // Test invalid config - expect build to fail
        let invalid_result = Config::builder()
            .namespace("")
            .protocol("TCP")
            .local_port(8080)
            .build();

        let err = invalid_result.unwrap_err();
        assert!(matches!(err, Error::Validation(_)));
        assert_eq!(
            err.to_string(),
            "Validation error: Namespace must be specified"
        );
    }

    #[test]
    fn test_config_preparation() {
        let config = Config::builder()
            .namespace("test")
            .workload_type("test")
            .protocol("TCP")
            .local_port(8080)
            .build()
            .unwrap()
            .prepare_for_save();

        assert!(config.alias.is_some());
        assert_eq!(config.kubeconfig, Some("default".to_string()));
    }

    #[test]
    fn test_config_getters() {
        let mut config = Config::builder()
            .id(1)
            .namespace("test")
            .service("test-service")
            .local_port(8080)
            .remote_port(80)
            .build()
            .unwrap();

        assert_eq!(config.get_id(), Some(1));
        assert_eq!(config.get_service_name(), Some("test-service"));
        assert_eq!(config.get_local_port(), Some(8080));
        assert_eq!(config.get_remote_port(), Some(80));

        config.set_id(2);
        assert_eq!(config.get_id(), Some(2));
    }
}

//! Builder for creating and validating port forwarding configurations
//!
//! This module provides a builder for creating and validating port forwarding
//! configurations. It ensures that the configuration is valid before returning
//! a `Config` object.

use super::Config;
use crate::error::Result;

#[derive(Default)]
pub struct ConfigBuilder {
    id: Option<i64>,
    service: Option<String>,
    namespace: String,
    local_port: Option<u16>,
    remote_port: Option<u16>,
    context: String,
    workload_type: Option<String>,
    protocol: String,
    remote_address: Option<String>,
    local_address: Option<String>,
    alias: Option<String>,
    domain_enabled: Option<bool>,
    kubeconfig: Option<String>,
    target: Option<String>,
}

impl ConfigBuilder {
    pub fn new() -> Self {
        Self {
            protocol: String::from("TCP"),
            ..Default::default()
        }
    }

    pub fn id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    pub fn service<S: Into<String>>(mut self, service: S) -> Self {
        self.service = Some(service.into());
        self
    }

    pub fn namespace<S: Into<String>>(mut self, namespace: S) -> Self {
        self.namespace = namespace.into();
        self
    }

    pub fn local_port(mut self, port: u16) -> Self {
        self.local_port = Some(port);
        self
    }

    pub fn remote_port(mut self, port: u16) -> Self {
        self.remote_port = Some(port);
        self
    }

    pub fn context<S: Into<String>>(mut self, context: S) -> Self {
        self.context = context.into();
        self
    }

    pub fn workload_type<S: Into<String>>(mut self, workload_type: S) -> Self {
        self.workload_type = Some(workload_type.into());
        self
    }

    pub fn protocol<S: Into<String>>(mut self, protocol: S) -> Self {
        self.protocol = protocol.into();
        self
    }

    pub fn remote_address<S: Into<String>>(mut self, address: S) -> Self {
        self.remote_address = Some(address.into());
        self
    }

    pub fn local_address<S: Into<String>>(mut self, address: S) -> Self {
        self.local_address = Some(address.into());
        self
    }

    pub fn alias<S: Into<String>>(mut self, alias: S) -> Self {
        self.alias = Some(alias.into());
        self
    }

    pub fn domain_enabled(mut self, enabled: bool) -> Self {
        self.domain_enabled = Some(enabled);
        self
    }

    pub fn kubeconfig<S: Into<String>>(mut self, kubeconfig: S) -> Self {
        self.kubeconfig = Some(kubeconfig.into());
        self
    }

    pub fn target<S: Into<String>>(mut self, target: S) -> Self {
        self.target = Some(target.into());
        self
    }

    pub fn build(self) -> Result<Config> {
        let config = Config {
            id: self.id,
            service: self.service,
            namespace: self.namespace,
            local_port: self.local_port,
            remote_port: self.remote_port,
            context: self.context,
            workload_type: self.workload_type,
            protocol: self.protocol,
            remote_address: self.remote_address,
            local_address: self.local_address,
            alias: self.alias,
            domain_enabled: self.domain_enabled,
            kubeconfig: self.kubeconfig,
            target: self.target,
        };

        config.validate()?;
        Ok(config)
    }
}

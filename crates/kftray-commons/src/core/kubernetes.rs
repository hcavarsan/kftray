//! Kubernetes client for cluster operations and port forwarding
//!
//! This module provides functionality for interacting with Kubernetes clusters,
//! including listing namespaces, services, and managing port forwards.
//!

use std::collections::HashMap;
use std::sync::Arc;
use k8s_openapi::api::core::v1::{Namespace, Service};
use kube::{
    api::{Api, ListParams},
    Client, Config, ResourceExt,
};
use tokio::sync::RwLock;

use crate::error::{Error, Result};
use crate::config::Config as PortConfig;

pub struct KubernetesClient {
    client: Client,
    context: String,
}
use kube::config::KubeconfigError;
#[derive(Clone)]
pub struct PortForwardManager {
    active_forwards: Arc<RwLock<HashMap<String, tokio::task::JoinHandle<()>>>>,
}

impl KubernetesClient {
    pub async fn new(context: &str, kubeconfig: Option<String>) -> Result<Self> {
        let config = match kubeconfig {
            Some(_path) => Config::from_kubeconfig(
                &kube::config::KubeConfigOptions {
                    context: Some(context.to_string()),
                    ..Default::default()
                }
            ).await,
            None => Config::infer().await.map_err(|e| KubeconfigError::LoadContext(e.to_string())),
        }
        .map_err(|e| Error::kubernetes(format!("Failed to create kubernetes config: {}", e)))?;

        let client = Client::try_from(config)
            .map_err(|e| Error::kubernetes(format!("Failed to create kubernetes client: {}", e)))?;

        Ok(Self {
            client,
            context: context.to_string(),
        })
    }

    pub async fn list_namespaces(&self) -> Result<Vec<String>> {
        let api: Api<Namespace> = Api::all(self.client.clone());
        let namespaces = api
            .list(&ListParams::default())
            .await
            .map_err(|e| Error::kubernetes(format!("Failed to list namespaces: {}", e)))?;

        Ok(namespaces.iter().map(|ns| ns.name_any()).collect())
    }

    pub async fn list_services(&self, namespace: &str) -> Result<Vec<String>> {
        let api: Api<Service> = Api::namespaced(self.client.clone(), namespace);
        let services = api
            .list(&ListParams::default())
            .await
            .map_err(|e| Error::kubernetes(format!("Failed to list services: {}", e)))?;

        Ok(services.iter().map(|svc| svc.name_any()).collect())
    }

    pub async fn get_service_configs(&self, namespace: &str) -> Result<Vec<PortConfig>> {
        let api: Api<Service> = Api::namespaced(self.client.clone(), namespace);
        let services = api
            .list(&ListParams::default())
            .await
            .map_err(|e| Error::kubernetes(format!("Failed to list services: {}", e)))?;

        let mut configs = Vec::new();
        for service in services {
            if let Some(annotations) = service.metadata.annotations {
                if let Some(config_str) = annotations.get("port-forward.config") {
                    let mut config: PortConfig = serde_json::from_str(config_str)
                        .map_err(|e| Error::kubernetes(format!("Invalid config annotation: {}", e)))?;
                    config.namespace = namespace.to_string();
                    config.context = self.context.clone();
                    configs.push(config);
                }
            }
        }

        Ok(configs)
    }
}

impl Default for PortForwardManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PortForwardManager {
    pub fn new() -> Self {
        Self {
            active_forwards: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn start_forward(&self, config: &PortConfig) -> Result<()> {
        let key = format!("{}_{}", config.namespace, config.service.as_deref().unwrap_or_default());

        let mut forwards = self.active_forwards.write().await;
        if forwards.contains_key(&key) {
            return Err(Error::port(format!("Port forward already exists for {}", key)));
        }

        // Implementation for starting port forward would go here
        // This is a placeholder for the actual implementation
        let handle = tokio::spawn(async move {
            // Actual port forwarding logic would go here
        });

        forwards.insert(key, handle);
        Ok(())
    }

    pub async fn stop_forward(&self, namespace: &str, service: &str) -> Result<()> {
        let key = format!("{}_{}", namespace, service);
        let mut forwards = self.active_forwards.write().await;

        if let Some(handle) = forwards.remove(&key) {
            handle.abort();
        }

        Ok(())
    }

    pub async fn stop_all(&self) -> Result<()> {
        let mut forwards = self.active_forwards.write().await;
        for (_, handle) in forwards.drain() {
            handle.abort();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_port_forward_manager() {
        let manager = PortForwardManager::new();
        let config = PortConfig {
            id: Some(1),
            namespace: "default".to_string(),
            service: Some("test-service".to_string()),
            local_port: Some(8080),
            remote_port: Some(8080),
            workload_type: Some("service".to_string()),
            protocol: "tcp".to_string(),
            remote_address: Some("localhost".to_string()),
            local_address: Some("127.0.0.1".to_string()),
            alias: None,
            domain_enabled: Some(false),
            kubeconfig: None,
            context: String::new(),
            target: None,
        };

        assert!(manager.start_forward(&config).await.is_ok());
        assert!(manager.stop_forward("default", "test-service").await.is_ok());
    }
}

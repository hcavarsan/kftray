use kftray_commons::config_model::Config;
use kube::Client;
use serde_json::json;
use serde_json::Value;

use super::{
    manifest::ManifestLoader,
    resources::{
        KubeResource,
        PodResource,
        SecretResource,
        ServiceResource,
    },
};
use crate::error::Error;

pub struct ResourceManager {
    client: Client,
    namespace: String,
    manifest_loader: ManifestLoader,
}

impl ResourceManager {
    pub async fn new(client: Client, namespace: String) -> Result<Self, Error> {
        let manifest_loader = ManifestLoader::new().await?;

        Ok(Self {
            client,
            namespace,
            manifest_loader,
        })
    }

    pub async fn create_resources(
        &self, values: &serde_json::Map<String, Value>,
    ) -> Result<(), Error> {
        let manifest = self.manifest_loader.load_and_render(values)?;

        // Create pod (required)
        let pod = PodResource::from_manifest(&manifest)?;
        pod.create(self.client.clone(), &self.namespace).await?;

        // Create service if present in manifest
        if manifest.get("service").is_some() {
            let service = ServiceResource::from_manifest(&manifest)?;
            service.create(self.client.clone(), &self.namespace).await?;
        }

        // Create secret if present in manifest
        if manifest.get("secret").is_some() {
            let secret = SecretResource::from_manifest(&manifest)?;
            secret.create(self.client.clone(), &self.namespace).await?;
        }

        // Wait for pod to be ready
        self.wait_for_pod_ready(&pod).await?;

        Ok(())
    }

    async fn wait_for_pod_ready(&self, pod: &PodResource) -> Result<(), Error> {
        let max_retries = 30;
        let retry_interval = std::time::Duration::from_secs(2);

        for _ in 0..max_retries {
            if pod.is_ready(self.client.clone(), &self.namespace).await? {
                return Ok(());
            }
            tokio::time::sleep(retry_interval).await;
        }

        Err(Error::PodNotReady(
            "Pod failed to become ready within timeout".into(),
        ))
    }

    pub async fn cleanup_resources(&self) -> Result<(), Error> {
        let manifest = self
            .manifest_loader
            .load_and_render(&serde_json::Map::new())?;

        // Delete pod (required)
        let pod = PodResource::from_manifest(&manifest)?;
        pod.delete(self.client.clone(), &self.namespace).await?;

        // Delete service if present
        if manifest.get("service").is_some() {
            let service = ServiceResource::from_manifest(&manifest)?;
            service.delete(self.client.clone(), &self.namespace).await?;
        }

        // Delete secret if present
        if manifest.get("secret").is_some() {
            let secret = SecretResource::from_manifest(&manifest)?;
            secret.delete(self.client.clone(), &self.namespace).await?;
        }

        Ok(())
    }

    pub async fn create_proxy_resources(
        &self, pod_name: &str, config: &Config,
    ) -> Result<(), Error> {
        let mut values = serde_json::Map::new();
        values.insert("hashed_name".to_string(), json!(pod_name));
        values.insert("namespace".to_string(), json!(config.namespace));
        values.insert(
            "config_id".to_string(),
            json!(config.id.unwrap_or_default().to_string()),
        );

        // Convert port values to integers before inserting
        if let Some(local_port) = config.local_port {
            values.insert("local_port".to_string(), json!(local_port));
        }
        if let Some(remote_port) = config.remote_port {
            values.insert("remote_port".to_string(), json!(remote_port));
        }
        values.insert(
            "remote_address".to_string(),
            json!(config
                .remote_address
                .clone()
                .unwrap_or_else(|| config.service.clone().unwrap_or_default())),
        );
        values.insert(
            "protocol".to_string(),
            json!(config.protocol.to_uppercase()),
        );

        let manifest = self
            .manifest_loader
            .create_proxy_pod_manifest(pod_name, config)?;

        // Create service if present in manifest
        if manifest.get("service").is_some() {
            let service = ServiceResource::from_manifest(&manifest)?;
            self.create_service(&service).await?;
        }

        // Create secret if present in manifest
        if manifest.get("secret").is_some() {
            let secret = SecretResource::from_manifest(&manifest)?;
            self.create_secret(&secret).await?;
        }

        Ok(())
    }

    async fn create_service(&self, service: &ServiceResource) -> Result<(), Error> {
        service.create(self.client.clone(), &self.namespace).await
    }

    async fn create_secret(&self, secret: &SecretResource) -> Result<(), Error> {
        secret.create(self.client.clone(), &self.namespace).await
    }

    pub async fn delete_proxy_resources(&self, manifest: &Value) -> Result<(), Error> {
        // Delete service if present in manifest
        if manifest.get("service").is_some() {
            let service = ServiceResource::from_manifest(manifest)?;
            self.delete_service(&service).await?;
        }

        // Delete secret if present in manifest
        if manifest.get("secret").is_some() {
            let secret = SecretResource::from_manifest(manifest)?;
            self.delete_secret(&secret).await?;
        }

        Ok(())
    }

    async fn delete_service(&self, service: &ServiceResource) -> Result<(), Error> {
        service.delete(self.client.clone(), &self.namespace).await
    }

    async fn delete_secret(&self, secret: &SecretResource) -> Result<(), Error> {
        secret.delete(self.client.clone(), &self.namespace).await
    }
}

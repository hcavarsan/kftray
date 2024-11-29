use std::future::Future;
use std::time::Duration;

use k8s_openapi::NamespaceResourceScope;
use k8s_openapi::Resource;
use kftray_commons::config_model::Config;
use kube::Client;
use serde_json::{
    Map,
    Value,
};
use tokio::try_join;

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
use crate::kubernetes::resources::ResourceOperations;

const POD_READY_TIMEOUT: Duration = Duration::from_secs(60);
const POD_READY_INTERVAL: Duration = Duration::from_secs(2);
const RESOURCE_CREATION_TIMEOUT: Duration = Duration::from_secs(30);

#[allow(dead_code)]
const RESOURCE_DELETION_TIMEOUT: Duration = Duration::from_secs(30);

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

    pub async fn create_resources(&self, values: &Map<String, Value>) -> Result<(), Error> {
        let manifest = self.manifest_loader.load_and_render(values)?;

        // Create pod first
        let pod = PodResource::from_manifest(&manifest)?;
        self.with_timeout(
            pod.create(self.client.clone(), &self.namespace),
            RESOURCE_CREATION_TIMEOUT,
        )
        .await?;

        // Create other resources concurrently
        let service_future = self.create_optional_resource::<ServiceResource>(&manifest);
        let secret_future = self.create_optional_resource::<SecretResource>(&manifest);

        try_join!(service_future, secret_future)?;

        self.wait_for_pod_ready(&pod).await
    }

    async fn create_optional_resource<T>(&self, manifest: &Value) -> Result<(), Error>
    where
        T: KubeResource + Send + Sync,
        T::ApiType: Resource<Scope = NamespaceResourceScope>,
    {
        if manifest.get(T::resource_type()).is_some() {
            let resource = T::from_manifest(manifest)?;
            resource
                .create(self.client.clone(), &self.namespace)
                .await?;
        }
        Ok(())
    }

    async fn wait_for_pod_ready(&self, pod: &PodResource) -> Result<(), Error> {
        let start = std::time::Instant::now();

        while start.elapsed() < POD_READY_TIMEOUT {
            if pod.is_ready(self.client.clone(), &self.namespace).await? {
                return Ok(());
            }
            tokio::time::sleep(POD_READY_INTERVAL).await;
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
        let manifest = self
            .manifest_loader
            .create_proxy_pod_manifest(pod_name, config)?;

        match config.workload_type.as_deref() {
            Some("proxy") => {
                // For proxy type, only create the pod
                let pod = PodResource::from_manifest(&manifest)?;
                self.with_timeout(
                    pod.create(self.client.clone(), &self.namespace),
                    RESOURCE_CREATION_TIMEOUT,
                )
                .await?;
            }
            Some("expose") => {
                // For expose type, create all resources
                let pod = PodResource::from_manifest(&manifest)?;
                self.with_timeout(
                    pod.create(self.client.clone(), &self.namespace),
                    RESOURCE_CREATION_TIMEOUT,
                )
                .await?;

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
            }
            _ => return Err(Error::Config("Invalid workload type".into())),
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

    async fn with_timeout<F, T>(&self, operation: F, timeout: Duration) -> Result<T, Error>
    where
        F: Future<Output = Result<T, Error>>,
    {
        tokio::time::timeout(timeout, operation)
            .await
            .map_err(|_| Error::Timeout("Operation timed out".into()))?
    }

    pub async fn cleanup_proxy_resources(&self, config: &Config) -> Result<(), Error> {
        let manifest = self
            .manifest_loader
            .load_and_render(&serde_json::Map::new())?;
        let label_selector = format!(
            "app=kftray-server,config_id={}",
            config.id.unwrap_or_default()
        );

        match config.workload_type.as_deref() {
            Some("proxy") => {
                // For proxy type, only delete the pod
                let pod = PodResource::from_manifest(&manifest)?;
                pod.delete_by_label(self.client.clone(), &self.namespace, &label_selector)
                    .await?;
            }
            Some("expose") => {
                // For expose type, delete all resources
                let pod = PodResource::from_manifest(&manifest)?;
                pod.delete_by_label(self.client.clone(), &self.namespace, &label_selector)
                    .await?;

                // Delete service if present
                if manifest.get("service").is_some() {
                    let service = ServiceResource::from_manifest(&manifest)?;
                    service
                        .delete_by_label(self.client.clone(), &self.namespace, &label_selector)
                        .await?;
                }

                // Delete secret if present
                if manifest.get("secret").is_some() {
                    let secret = SecretResource::from_manifest(&manifest)?;
                    secret
                        .delete_by_label(self.client.clone(), &self.namespace, &label_selector)
                        .await?;
                }
            }
            _ => return Err(Error::Config("Invalid workload type".into())),
        }

        Ok(())
    }
}

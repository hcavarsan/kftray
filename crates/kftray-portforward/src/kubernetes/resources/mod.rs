mod pod;
mod secret;
mod service;

use std::fmt::Debug;
use std::time::{
    Duration,
    Instant,
};

use async_trait::async_trait;
use k8s_openapi::NamespaceResourceScope;
use kube::{
    Client,
    Resource,
};
// Re-export the resource types
pub use pod::PodResource;
pub use secret::SecretResource;
use serde::{
    de::DeserializeOwned,
    Serialize,
};
use serde_json::Value;
pub use service::ServiceResource;

use crate::error::Error;

// Common trait for basic resource operations
#[async_trait]
pub trait ResourceOperations: Send + Sync {
    type ApiType: Clone
        + DeserializeOwned
        + Debug
        + Resource<Scope = NamespaceResourceScope>
        + Serialize;

    fn get_manifest(&self) -> &Value;
    fn get_name(&self) -> Option<&str> {
        self.get_manifest()
            .pointer("/metadata/name")
            .and_then(Value::as_str)
    }

    async fn create_resource(&self, client: Client, namespace: &str) -> Result<(), Error>;
    async fn delete_resource(&self, client: Client, namespace: &str) -> Result<(), Error>;
    async fn delete_by_label(
        &self, client: Client, namespace: &str, label_selector: &str,
    ) -> Result<(), Error>;
}

// Add resource status trait
#[async_trait]
pub trait ResourceStatus {
    async fn get_status(&self, client: Client, namespace: &str) -> Result<bool, Error>;
    fn is_terminal_status(&self) -> bool;
}

// Enhance the KubeResource trait
#[async_trait]
pub trait KubeResource: ResourceOperations + ResourceStatus {
    fn from_manifest(manifest: &Value) -> Result<Self, Error>
    where
        Self: Sized;
    fn resource_type() -> &'static str;

    async fn wait_until_ready(
        &self, client: Client, namespace: &str, timeout: Duration,
    ) -> Result<(), Error> {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if self.get_status(client.clone(), namespace).await? {
                return Ok(());
            }
            if self.is_terminal_status() {
                return Err(Error::ResourceFailed(format!(
                    "{} failed to become ready",
                    Self::resource_type()
                )));
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        Err(Error::Timeout(format!(
            "{} failed to become ready within timeout",
            Self::resource_type()
        )))
    }

    async fn create(&self, client: Client, namespace: &str) -> Result<(), Error> {
        self.create_resource(client, namespace).await
    }

    async fn delete(&self, client: Client, namespace: &str) -> Result<(), Error> {
        self.delete_resource(client, namespace).await
    }

    async fn is_ready(&self, client: Client, namespace: &str) -> Result<bool, Error>;
}

pub trait ManifestResource {
    fn get_manifest_section(&self) -> &str;
}

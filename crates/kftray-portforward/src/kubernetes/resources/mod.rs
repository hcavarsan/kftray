mod pod;
mod secret;
mod service;

use async_trait::async_trait;
use kube::Client;
pub use pod::PodResource;
pub use secret::SecretResource;
use serde::de::DeserializeOwned;
use serde_json::Value;
pub use service::ServiceResource;

use crate::error::Error;

#[async_trait]
pub trait KubeResource {
    type Resource: DeserializeOwned + Clone;

    fn from_manifest(manifest: &Value) -> Result<Self, Error>
    where
        Self: Sized;
    fn get_name(&self) -> Option<&str>;

    async fn create(&self, client: Client, namespace: &str) -> Result<(), Error>;
    async fn delete(&self, client: Client, namespace: &str) -> Result<(), Error>;
    async fn is_ready(&self, client: Client, namespace: &str) -> Result<bool, Error>;
}

pub trait ManifestResource {
    fn get_manifest(&self) -> &Value;
    fn get_manifest_section(&self) -> &str;
}

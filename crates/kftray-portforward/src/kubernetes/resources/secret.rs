use async_trait::async_trait;
use k8s_openapi::api::core::v1::Secret;
use kube::{
    api::ListParams,
    api::PostParams,
    Api,
    Client,
};
use serde_json::Value;

use super::{
    KubeResource,
    ManifestResource,
    ResourceOperations,
    ResourceStatus,
};
use crate::error::Error;

pub struct SecretResource {
    manifest: Value,
}

#[async_trait]
impl ResourceOperations for SecretResource {
    type ApiType = Secret;

    fn get_manifest(&self) -> &Value {
        &self.manifest
    }

    async fn create_resource(&self, client: Client, namespace: &str) -> Result<(), Error> {
        let api: Api<Self::ApiType> = Api::namespaced(client, namespace);
        let resource: Self::ApiType = serde_json::from_value(self.get_manifest().clone())?;
        api.create(&PostParams::default(), &resource).await?;
        Ok(())
    }

    async fn delete_resource(&self, client: Client, namespace: &str) -> Result<(), Error> {
        if let Some(name) = self.get_name() {
            let api: Api<Self::ApiType> = Api::namespaced(client, namespace);
            api.delete(name, &Default::default()).await?;
        }
        Ok(())
    }

    async fn delete_by_label(
        &self, client: Client, namespace: &str, label_selector: &str,
    ) -> Result<(), Error> {
        let api: Api<Self::ApiType> = Api::namespaced(client, namespace);
        let lp = ListParams::default().labels(label_selector);
        let secrets = api.list(&lp).await?;

        for secret in secrets.items {
            if let Some(name) = secret.metadata.name {
                api.delete(&name, &Default::default()).await?;
            }
        }
        Ok(())
    }
}

impl ManifestResource for SecretResource {
    fn get_manifest_section(&self) -> &str {
        "secret"
    }
}

#[async_trait]
impl KubeResource for SecretResource {
    fn from_manifest(manifest: &Value) -> Result<Self, Error> {
        Ok(Self {
            manifest: manifest["secret"].clone(),
        })
    }

    fn resource_type() -> &'static str {
        "secret"
    }

    async fn is_ready(&self, _client: Client, _namespace: &str) -> Result<bool, Error> {
        Ok(true)
    }
}

#[async_trait]
impl ResourceStatus for SecretResource {
    async fn get_status(&self, _client: Client, _namespace: &str) -> Result<bool, Error> {
        // Secrets are considered ready as soon as they're created
        Ok(true)
    }

    fn is_terminal_status(&self) -> bool {
        false
    }
}

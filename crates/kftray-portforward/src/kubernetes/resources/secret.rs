use async_trait::async_trait;
use k8s_openapi::api::core::v1::Secret;
use kube::{
    api::{
        Api,
        PostParams,
    },
    Client,
};
use serde_json::Value;

use super::{
    KubeResource,
    ManifestResource,
};
use crate::error::Error;

pub struct SecretResource {
    manifest: Value,
}

impl ManifestResource for SecretResource {
    fn get_manifest(&self) -> &Value {
        &self.manifest
    }

    fn get_manifest_section(&self) -> &str {
        "secret"
    }
}

#[async_trait]
impl KubeResource for SecretResource {
    type Resource = Secret;

    fn from_manifest(manifest: &Value) -> Result<Self, Error> {
        Ok(Self {
            manifest: manifest["secret"].clone(),
        })
    }

    fn get_name(&self) -> Option<&str> {
        self.manifest["metadata"]["name"].as_str()
    }

    async fn create(&self, client: Client, namespace: &str) -> Result<(), Error> {
        let secrets: Api<Secret> = Api::namespaced(client, namespace);
        let secret: Secret = serde_json::from_value(self.manifest.clone())?;
        secrets.create(&PostParams::default(), &secret).await?;
        Ok(())
    }

    async fn delete(&self, client: Client, namespace: &str) -> Result<(), Error> {
        if let Some(name) = self.get_name() {
            let secrets: Api<Secret> = Api::namespaced(client, namespace);
            secrets.delete(name, &Default::default()).await?;
        }
        Ok(())
    }

    async fn is_ready(&self, _client: Client, _namespace: &str) -> Result<bool, Error> {
        Ok(true)
    }
}

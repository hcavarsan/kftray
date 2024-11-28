use async_trait::async_trait;
use k8s_openapi::api::core::v1::Pod;
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

pub struct PodResource {
    manifest: Value,
}

impl ManifestResource for PodResource {
    fn get_manifest(&self) -> &Value {
        &self.manifest
    }

    fn get_manifest_section(&self) -> &str {
        "pod"
    }
}

#[async_trait]
impl KubeResource for PodResource {
    type Resource = Pod;

    fn from_manifest(manifest: &Value) -> Result<Self, Error> {
        Ok(Self {
            manifest: manifest["pod"].clone(),
        })
    }

    fn get_name(&self) -> Option<&str> {
        self.manifest["metadata"]["name"].as_str()
    }

    async fn create(&self, client: Client, namespace: &str) -> Result<(), Error> {
        let pods: Api<Pod> = Api::namespaced(client, namespace);
        let pod: Pod = serde_json::from_value(self.manifest.clone())?;
        pods.create(&PostParams::default(), &pod).await?;
        Ok(())
    }

    async fn delete(&self, client: Client, namespace: &str) -> Result<(), Error> {
        if let Some(name) = self.get_name() {
            let pods: Api<Pod> = Api::namespaced(client, namespace);
            pods.delete(name, &Default::default()).await?;
        }
        Ok(())
    }

    async fn is_ready(&self, client: Client, namespace: &str) -> Result<bool, Error> {
        if let Some(name) = self.get_name() {
            let pods: Api<Pod> = Api::namespaced(client, namespace);
            if let Ok(pod) = pods.get(name).await {
                if let Some(status) = pod.status {
                    if let Some(conditions) = status.conditions {
                        return Ok(conditions
                            .iter()
                            .any(|c| c.type_ == "Ready" && c.status == "True"));
                    }
                }
            }
        }
        Ok(false)
    }
}

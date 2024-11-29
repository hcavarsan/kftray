use async_trait::async_trait;
use k8s_openapi::api::core::v1::Pod;
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

pub struct PodResource {
    manifest: Value,
}

#[async_trait]
impl ResourceOperations for PodResource {
    type ApiType = Pod;

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
        let pods = api.list(&lp).await?;

        for pod in pods.items {
            if let Some(name) = pod.metadata.name {
                api.delete(&name, &Default::default()).await?;
            }
        }
        Ok(())
    }
}

impl ManifestResource for PodResource {
    fn get_manifest_section(&self) -> &str {
        "pod"
    }
}

#[async_trait]
impl KubeResource for PodResource {
    fn from_manifest(manifest: &Value) -> Result<Self, Error> {
        Ok(Self {
            manifest: manifest["pod"].clone(),
        })
    }

    fn resource_type() -> &'static str {
        "pod"
    }

    async fn is_ready(&self, client: Client, namespace: &str) -> Result<bool, Error> {
        if let Some(name) = self.get_name() {
            let pods: Api<Pod> = Api::namespaced(client, namespace);
            return Ok(pods
                .get(name)
                .await
                .map(|pod| Self::check_pod_ready(&pod))
                .unwrap_or(false));
        }
        Ok(false)
    }
}

impl PodResource {
    fn check_pod_ready(pod: &Pod) -> bool {
        pod.status
            .as_ref()
            .and_then(|status| status.conditions.as_ref())
            .map(|conditions| {
                conditions
                    .iter()
                    .any(|c| c.type_ == "Ready" && c.status == "True")
            })
            .unwrap_or(false)
    }
}

#[async_trait]
impl ResourceStatus for PodResource {
    async fn get_status(&self, client: Client, namespace: &str) -> Result<bool, Error> {
        self.is_ready(client, namespace).await
    }

    fn is_terminal_status(&self) -> bool {
        false // Pods can recover from failed states
    }
}

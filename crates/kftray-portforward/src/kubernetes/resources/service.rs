use async_trait::async_trait;
use k8s_openapi::api::core::v1::Service;
use kube::{
    api::ListParams,
    api::PostParams,
    Api,
    Client,
};
use serde_json::{
    json,
    Value,
};

use super::{
    KubeResource,
    ManifestResource,
    ResourceOperations,
    ResourceStatus,
};
use crate::error::Error;

pub struct ServiceResource {
    manifest: Value,
}

#[async_trait]
impl ResourceOperations for ServiceResource {
    type ApiType = Service;

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
        let services = api.list(&lp).await?;

        for service in services.items {
            if let Some(name) = service.metadata.name {
                api.delete(&name, &Default::default()).await?;
            }
        }
        Ok(())
    }
}

impl ManifestResource for ServiceResource {
    fn get_manifest_section(&self) -> &str {
        "service"
    }
}

#[async_trait]
impl KubeResource for ServiceResource {
    fn from_manifest(manifest: &Value) -> Result<Self, Error> {
        let mut service_manifest = manifest["service"].clone();
        Self::process_ports(&mut service_manifest)?;
        Ok(Self {
            manifest: service_manifest,
        })
    }

    fn resource_type() -> &'static str {
        "service"
    }

    async fn is_ready(&self, _client: Client, _namespace: &str) -> Result<bool, Error> {
        Ok(true)
    }
}

impl ServiceResource {
    fn parse_port_value(value: &Value) -> Result<i32, Error> {
        match value {
            Value::String(s) if s.is_empty() => Ok(0),
            Value::String(s) => s.parse::<i32>().map_err(|_| Error::InvalidPort),
            Value::Number(n) => n.as_i64().map(|n| n as i32).ok_or(Error::InvalidPort),
            Value::Null => Ok(0),
            _ => Err(Error::InvalidPort),
        }
    }

    fn process_ports(service_manifest: &mut Value) -> Result<(), Error> {
        let ports = match service_manifest.pointer_mut("/spec/ports") {
            Some(Value::Array(ports)) => ports,
            _ => return Ok(()),
        };

        for port in ports {
            let port_obj = match port.as_object_mut() {
                Some(obj) => obj,
                None => continue,
            };

            for key in ["port", "targetPort"] {
                if let Some(port_value) = port_obj.get(key) {
                    let port_int = Self::parse_port_value(port_value)?;
                    port_obj.insert(key.to_string(), json!(port_int));
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl ResourceStatus for ServiceResource {
    async fn get_status(&self, _client: Client, _namespace: &str) -> Result<bool, Error> {
        // Services are considered ready as soon as they're created
        Ok(true)
    }

    fn is_terminal_status(&self) -> bool {
        false
    }
}

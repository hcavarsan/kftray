use async_trait::async_trait;
use k8s_openapi::api::core::v1::Service;
use kube::{
    api::{
        Api,
        PostParams,
    },
    Client,
};
use serde_json::{
    json,
    Value,
};

use super::{
    KubeResource,
    ManifestResource,
};
use crate::error::Error;

pub struct ServiceResource {
    manifest: Value,
}

impl ManifestResource for ServiceResource {
    fn get_manifest(&self) -> &Value {
        &self.manifest
    }

    fn get_manifest_section(&self) -> &str {
        "service"
    }
}

#[async_trait]
impl KubeResource for ServiceResource {
    type Resource = Service;

    fn from_manifest(manifest: &Value) -> Result<Self, Error> {
        let mut service_manifest = manifest["service"].clone();

        if let Some(spec) = service_manifest.get_mut("spec") {
            if let Some(ports) = spec.get_mut("ports") {
                if let Some(ports_array) = ports.as_array_mut() {
                    for port in ports_array {
                        if let Some(port_obj) = port.as_object_mut() {
                            for key in ["port", "targetPort"] {
                                if let Some(port_value) = port_obj.get(key) {
                                    let port_int = match port_value {
                                        Value::String(s) if s.is_empty() => 0,
                                        Value::String(s) => {
                                            s.parse::<i32>().map_err(|_| Error::InvalidPort)?
                                        }
                                        Value::Number(n) => {
                                            n.as_i64().ok_or(Error::InvalidPort)? as i32
                                        }
                                        Value::Null => 0,
                                        _ => return Err(Error::InvalidPort),
                                    };
                                    port_obj.insert(key.to_string(), json!(port_int));
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(Self {
            manifest: service_manifest,
        })
    }

    fn get_name(&self) -> Option<&str> {
        self.manifest["metadata"]["name"].as_str()
    }

    async fn create(&self, client: Client, namespace: &str) -> Result<(), Error> {
        let services: Api<Service> = Api::namespaced(client, namespace);
        let service: Service = serde_json::from_value(self.manifest.clone())?;
        services.create(&PostParams::default(), &service).await?;
        Ok(())
    }

    async fn delete(&self, client: Client, namespace: &str) -> Result<(), Error> {
        if let Some(name) = self.get_name() {
            let services: Api<Service> = Api::namespaced(client, namespace);
            services.delete(name, &Default::default()).await?;
        }
        Ok(())
    }

    async fn is_ready(&self, _client: Client, _namespace: &str) -> Result<bool, Error> {
        Ok(true)
    }
}

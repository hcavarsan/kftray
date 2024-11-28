use kftray_commons::models::config_model::Config;
use kube::Client as KubeClient;
use log::info;
use serde_json::json;

use crate::error::Error;
use crate::expose::config::TunnelConfig;
use crate::kubernetes::ResourceManager;

pub struct KubernetesManager {
    client: KubeClient,
    config: Config,
}

impl KubernetesManager {
    pub fn new(client: KubeClient, config: Config) -> Self {
        Self { client, config }
    }

    pub async fn setup_resources(&self, tunnel_config: &TunnelConfig) -> Result<(), Error> {
        info!(
            "Creating Kubernetes resources in namespace: {}",
            self.config.namespace
        );

        let resource_manager =
            ResourceManager::new(self.client.clone(), self.config.namespace.clone()).await?;

        let values = json!({
            "name": "kftray-server",
            "namespace": self.config.namespace,
            "local_port": tunnel_config.local_port.to_string(),
            "remote_port": tunnel_config.remote_port.to_string(),
            "ssh_authorized_keys": std::fs::read_to_string(
                tunnel_config.ssh_key_path.with_extension("pub")
            )?,
        });

        resource_manager
            .create_resources(values.as_object().unwrap())
            .await?;

        Ok(())
    }
}

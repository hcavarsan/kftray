mod config;
mod connection;
mod kubernetes;

pub use config::TunnelConfig;
pub use connection::SshTunnel;
use kftray_commons::models::config_model::Config;
use kube::Client as KubeClient;
pub use kubernetes::KubernetesManager;

use crate::error::Error;

pub async fn handle_expose(kube_client: KubeClient, config: &Config) -> Result<(), Error> {
    let tunnel_config = TunnelConfig::from_common_config(config)?;
    let k8s_manager = KubernetesManager::new(kube_client, config.clone());
    k8s_manager.setup_resources(&tunnel_config).await?;

    let mut tunnel = SshTunnel::new(tunnel_config);
    tunnel.connect_and_forward().await?;

    Ok(())
}

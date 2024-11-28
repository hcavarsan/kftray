use std::time::Duration;

use log::info;
use tokio::time::sleep;

mod config;
mod error;
mod kubernetes;
mod ssh;

use crate::{
    config::TunnelConfig,
    error::*,
    kubernetes::resources::KubernetesManager,
    ssh::tunnel::{
        SshTunnel,
        TunnelService,
    },
};

async fn setup_tunnel(config: TunnelConfig) -> TunnelResult<()> {
    info!("Starting SSH tunnel setup process");

    let k8s_manager = KubernetesManager::new(config.namespace.clone()).await?;
    k8s_manager.setup_resources(&config).await?;

    let mut retry_count = 0;
    let mut tunnel = SshTunnel::new(config.clone());

    while retry_count < config.max_retries {
        match tunnel.connect().await {
            Ok(()) => match tunnel.setup_forward().await {
                Ok(()) => {
                    info!("SSH tunnel established successfully");
                    return tunnel.run().await;
                }
                Err(e) => {
                    retry_count += 1;
                    if retry_count < config.max_retries {
                        info!("Retrying in {} seconds...", config.retry_delay.as_secs());
                        sleep(config.retry_delay).await;
                        continue;
                    }
                    return Err(e);
                }
            },
            Err(e) => {
                retry_count += 1;
                if retry_count < config.max_retries {
                    info!("Retrying in {} seconds...", config.retry_delay.as_secs());
                    sleep(config.retry_delay).await;
                    continue;
                }
                return Err(e);
            }
        }
    }

    Err(TunnelError::Other(anyhow::anyhow!(
        "Failed to establish SSH tunnel after {} retries",
        config.max_retries
    )))
}

fn setup_logger() {
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("debug"));
}

#[tokio::main]
async fn main() -> TunnelResult<()> {
    setup_logger();

    // Use the builder pattern for configuration
    let config = TunnelConfig::builder()
        .local_port(2222)
        .remote_port(8085)
        .namespace("default".to_string())
        .buffer_size(16384)
        .retry_delay(Duration::from_secs(3))
        .max_retries(3)
        .keepalive_interval(Duration::from_secs(30))
        .pod_ready_timeout(Duration::from_secs(60))
        .build()?;

    // Override with environment variables if present
    if let Ok(env_config) = TunnelConfig::from_env() {
        setup_tunnel(env_config).await
    } else {
        setup_tunnel(config).await
    }
}

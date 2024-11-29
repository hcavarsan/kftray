mod config;
mod connection;

pub use config::TunnelConfig;
pub use connection::SshTunnel;
use kftray_commons::models::config_model::Config;

use crate::error::Error;

pub async fn handle_expose(config: &Config) -> Result<(), Error> {
    let tunnel_config = TunnelConfig::from_common_config(config)?;

    let mut tunnel = SshTunnel::new(tunnel_config);
    tunnel.connect_and_forward().await?;

    Ok(())
}

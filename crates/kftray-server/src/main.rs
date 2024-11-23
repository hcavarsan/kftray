mod proxy;

use std::{
    env,
    sync::Arc,
};

use proxy::{
    config::{
        ProxyConfig,
        ProxyType,
    },
    error::ProxyError,
    http,
    tcp,
    udp,
};
use tokio::sync::Notify;

#[tokio::main]
async fn main() -> Result<(), ProxyError> {
    env_logger::init();

    let config = load_config()?;
    let shutdown = Arc::new(Notify::new());

    match config.proxy_type {
        ProxyType::Http => {
            http::start_proxy(config, shutdown.clone()).await?;
        }
        ProxyType::Tcp => {
            tcp::start_proxy(config, shutdown.clone()).await?;
        }
        ProxyType::Udp => {
            udp::start_proxy(config, shutdown.clone()).await?;
        }
    }

    shutdown.notified().await;
    Ok(())
}

fn load_config() -> Result<ProxyConfig, ProxyError> {
    let target_host = env::var("REMOTE_ADDRESS")
        .map_err(|_| ProxyError::Configuration("REMOTE_ADDRESS not set".into()))?;

    let target_port = env::var("REMOTE_PORT")
        .map_err(|_| ProxyError::Configuration("REMOTE_PORT not set".into()))?
        .parse()
        .map_err(|_| ProxyError::Configuration("Invalid REMOTE_PORT".into()))?;

    let proxy_port = env::var("LOCAL_PORT")
        .map_err(|_| ProxyError::Configuration("LOCAL_PORT not set".into()))?
        .parse()
        .map_err(|_| ProxyError::Configuration("Invalid LOCAL_PORT".into()))?;

    let proxy_type = match env::var("PROXY_TYPE")
        .map_err(|_| ProxyError::Configuration("PROXY_TYPE not set".into()))?
        .as_str()
    {
        "tcp" => ProxyType::Tcp,
        "http" => ProxyType::Http,
        "udp" => ProxyType::Udp,
        t => {
            return Err(ProxyError::Configuration(format!(
                "Invalid proxy type: {}",
                t
            )))
        }
    };

    Ok(ProxyConfig::new(
        target_host,
        target_port,
        proxy_port,
        proxy_type,
    ))
}

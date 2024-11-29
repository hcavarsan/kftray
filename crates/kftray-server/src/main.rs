mod proxy;

use std::env;
use std::net::ToSocketAddrs;
use std::sync::Arc;

use log::{
    error,
    info,
};
use tokio::signal;
use url::Url;

use crate::proxy::{
    config::{
        ProxyConfig,
        ProxyType,
    },
    error::ProxyError,
    server::ProxyServer,
};

/// Loads proxy configuration from environment variables
///
/// # Returns
/// * `Result<ProxyConfig, ProxyError>` - Parsed configuration or error details
///
/// # Environment Variables
/// * `REMOTE_ADDRESS` - Target server hostname/IP
/// * `REMOTE_PORT` - Target server port
/// * `LOCAL_PORT` - Local proxy listening port
/// * `PROXY_TYPE` - Protocol type ("tcp" or "udp")
/// * `SSH_AUTH` - Enable/disable SSH authentication (true/false)
/// * `SSH_AUTHORIZED_KEYS` - SSH authorized keys (comma-separated)
fn load_config() -> Result<ProxyConfig, ProxyError> {
    let target_host = env::var("REMOTE_ADDRESS")
        .map_err(|_| ProxyError::Configuration("REMOTE_ADDRESS not set".into()))?;

    let resolved_host = if target_host.contains("://") {
        let url = Url::parse(&target_host)
            .map_err(|e| ProxyError::Configuration(format!("Invalid URL: {}", e)))?;

        url.host_str()
            .ok_or_else(|| ProxyError::Configuration("No host found in URL".into()))?
            .to_string()
    } else {
        let test_url = format!("http://{}", target_host);
        if let Ok(url) = Url::parse(&test_url) {
            if let Some(host) = url.host_str() {
                host.to_string()
            } else {
                target_host
            }
        } else {
            target_host
        }
    };

    let socket_addr = format!("{}:0", resolved_host)
        .to_socket_addrs()
        .map_err(|e| ProxyError::Configuration(format!("Failed to resolve hostname: {}", e)))?
        .next()
        .ok_or_else(|| ProxyError::Configuration("No IP addresses found for hostname".into()))?;

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
        .to_lowercase()
        .as_str()
    {
        "tcp" => ProxyType::Tcp,
        "udp" => ProxyType::Udp,
        "ssh" => ProxyType::Ssh,
        t => {
            return Err(ProxyError::Configuration(format!(
                "Invalid proxy type: {}. Must be 'tcp', 'udp', or 'ssh'",
                t
            )))
        }
    };

    let ssh_auth_enabled = env::var("SSH_AUTH")
        .map(|v| v.to_lowercase() == "true")
        .unwrap_or(false);

    let ssh_authorized_keys = if ssh_auth_enabled {
        match env::var("SSH_AUTHORIZED_KEYS") {
            Ok(keys) => Some(keys.split(',').map(String::from).collect()),
            Err(_) => {
                return Err(ProxyError::Configuration(
                    "SSH_AUTHORIZED_KEYS required when SSH_AUTH=true".into(),
                ))
            }
        }
    } else {
        None
    };

    Ok(ProxyConfig::builder()
        .target_host(socket_addr.ip().to_string())
        .target_port(target_port)
        .proxy_port(proxy_port)
        .proxy_type(proxy_type)
        .ssh_auth_enabled(ssh_auth_enabled)
        .ssh_authorized_keys(ssh_authorized_keys)
        .build()?)
}

/// Main entry point for the proxy server application
///
/// Sets up logging, loads configuration, starts the proxy server,
/// and handles shutdown signals (Ctrl+C and SIGTERM)
#[tokio::main]
async fn main() -> Result<(), ProxyError> {
    env_logger::init();

    let config = load_config()?;
    let server = Arc::new(ProxyServer::new(config));
    let server_clone = Arc::clone(&server);

    let server_handle = tokio::spawn(async move { server_clone.run().await });

    tokio::select! {
        _ = signal::ctrl_c() => {
            info!("Received Ctrl+C signal");
        }
        _ = async {
            if let Ok(mut sigterm) = signal::unix::signal(signal::unix::SignalKind::terminate()) {
                let _ = sigterm.recv().await;
                info!("Received SIGTERM signal");
            }
        } => {}
    }

    server.shutdown();

    if let Err(e) = tokio::time::timeout(tokio::time::Duration::from_secs(5), server_handle).await {
        error!("Server shutdown timed out: {}", e);
    }

    info!("Server shutdown complete");
    Ok(())
}

#![allow(clippy::literal_string_with_formatting_args)]

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
fn load_config() -> Result<ProxyConfig, ProxyError> {
    let target_host = env::var("REMOTE_ADDRESS")
        .map_err(|_| ProxyError::Configuration("REMOTE_ADDRESS not set".into()))?;

    let original_host = if target_host.contains("://") {
        let url = Url::parse(&target_host)
            .map_err(|e| ProxyError::Configuration(format!("Invalid URL: {e}")))?;

        url.host_str()
            .ok_or_else(|| ProxyError::Configuration("No host found in URL".into()))?
            .to_string()
    } else {
        target_host.clone()
    };

    let resolved_ip = if original_host.parse::<std::net::IpAddr>().is_ok() {
        None
    } else {
        match format!("{}:0", original_host).to_socket_addrs() {
            Ok(mut addrs) => addrs.next().map(|addr| addr.ip().to_string()),
            Err(e) => {
                log::warn!(
                    "Failed to resolve hostname '{}': {}. Will use hostname directly.",
                    original_host,
                    e
                );
                None
            }
        }
    };

    let target_port = env::var("REMOTE_PORT")
        .map_err(|_| ProxyError::Configuration("REMOTE_PORT not set".into()))?
        .parse()
        .map_err(|_| ProxyError::Configuration("Invalid REMOTE_PORT".into()))?;

    let proxy_port = env::var("LOCAL_PORT")
        .map_err(|_| ProxyError::Configuration("LOCAL_PORT not set".into()))?
        .parse()
        .map_err(|_| ProxyError::Configuration("Invalid LOCAL_PORT".into()))?;

    let proxy_type_str = env::var("PROXY_TYPE")
        .map_err(|_| ProxyError::Configuration("PROXY_TYPE not set".into()))?;

    println!("Raw PROXY_TYPE value: '{proxy_type_str}'");
    let proxy_type_lower = proxy_type_str.to_lowercase();
    println!("Lowercased PROXY_TYPE value: '{proxy_type_lower}'");

    let proxy_type = match proxy_type_lower.as_str() {
        "tcp" => ProxyType::Tcp,
        "udp" => ProxyType::Udp,
        invalid_type => {
            println!("Invalid proxy type encountered: '{invalid_type}'");
            return Err(ProxyError::Configuration(format!(
                "Invalid proxy type: {invalid_type}"
            )));
        }
    };

    println!("Selected proxy type: {proxy_type:?}");

    let config = ProxyConfig::builder()
        .target_host(original_host)
        .resolved_ip(resolved_ip)
        .target_port(target_port)
        .proxy_port(proxy_port)
        .proxy_type(proxy_type)
        .build()?;

    println!("Final config proxy type: {:?}", config.proxy_type);
    Ok(config)
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

    #[cfg(unix)]
    {
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
    }

    #[cfg(not(unix))]
    {
        signal::ctrl_c().await.ok();
        info!("Received Ctrl+C signal");
    }

    server.shutdown();

    if let Err(e) = tokio::time::timeout(tokio::time::Duration::from_secs(5), server_handle).await {
        error!("Server shutdown timed out: {e}");
    }

    info!("Server shutdown complete");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::net::IpAddr;
    use std::sync::Mutex;

    use lazy_static::lazy_static;

    use super::*;

    lazy_static! {
        static ref ENV_TEST_MUTEX: Mutex<()> = Mutex::new(());
    }

    struct EnvVarGuard {
        key: String,
        original_value: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &str, value: &str) -> Self {
            let key = key.to_string();
            let original_value = env::var(&key).ok();
            unsafe { env::set_var(&key, value) };
            EnvVarGuard {
                key,
                original_value,
            }
        }

        fn remove(key: &str) -> Self {
            let key = key.to_string();
            let original_value = env::var(&key).ok();
            unsafe { env::remove_var(&key) };
            EnvVarGuard {
                key,
                original_value,
            }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.original_value {
                Some(val) => unsafe { env::set_var(&self.key, val) },

                None => unsafe { env::remove_var(&self.key) },
            }
        }
    }

    #[test]
    fn test_load_config_valid_tcp() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let _guard_addr = EnvVarGuard::set("REMOTE_ADDRESS", "127.0.0.1");
        let _guard_rport = EnvVarGuard::set("REMOTE_PORT", "8080");
        let _guard_lport = EnvVarGuard::set("LOCAL_PORT", "9090");
        let _guard_type = EnvVarGuard::set("PROXY_TYPE", "tcp");
        let _guard_home = EnvVarGuard::remove("HOME");
        let _guard_xdg = EnvVarGuard::remove("XDG_CONFIG_HOME");

        let result = load_config();
        assert!(result.is_ok(), "load_config failed: {:?}", result.err());

        let config = result.unwrap();
        let ip = config.target_host.parse::<IpAddr>().unwrap();
        assert!(ip.to_string() == "127.0.0.1" || config.target_host == "127.0.0.1");
        assert_eq!(config.target_port, 8080);
        assert_eq!(config.proxy_port, 9090);

        match config.proxy_type {
            ProxyType::Tcp => {}
            ProxyType::Udp => panic!("Expected TCP proxy type, got UDP"),
        }
    }

    #[test]
    fn test_load_config_valid_udp() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let _guard_addr = EnvVarGuard::set("REMOTE_ADDRESS", "127.0.0.1");
        let _guard_rport = EnvVarGuard::set("REMOTE_PORT", "8080");
        let _guard_lport = EnvVarGuard::set("LOCAL_PORT", "9090");
        let _guard_type = EnvVarGuard::set("PROXY_TYPE", "udp");
        let _guard_home = EnvVarGuard::remove("HOME");
        let _guard_xdg = EnvVarGuard::remove("XDG_CONFIG_HOME");

        let result = load_config();
        assert!(result.is_ok(), "load_config failed: {:?}", result.err());

        let config = result.unwrap();
        assert_eq!(config.target_port, 8080);
        assert_eq!(config.proxy_port, 9090);

        match config.proxy_type {
            ProxyType::Udp => {}
            ProxyType::Tcp => panic!("Expected UDP proxy type, got TCP"),
        }
    }

    #[test]
    fn test_load_config_with_url() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let _guard_addr = EnvVarGuard::set("REMOTE_ADDRESS", "http://example.com");
        let _guard_rport = EnvVarGuard::set("REMOTE_PORT", "8080");
        let _guard_lport = EnvVarGuard::set("LOCAL_PORT", "9090");
        let _guard_type = EnvVarGuard::set("PROXY_TYPE", "tcp");
        let _guard_home = EnvVarGuard::remove("HOME");
        let _guard_xdg = EnvVarGuard::remove("XDG_CONFIG_HOME");

        let result = load_config();
        assert!(result.is_ok(), "load_config failed: {:?}", result.err());

        let config = result.unwrap();
        assert_eq!(config.target_port, 8080);
        assert_eq!(config.proxy_port, 9090);

        match config.proxy_type {
            ProxyType::Tcp => {}
            ProxyType::Udp => panic!("Expected TCP proxy type, got UDP"),
        }
    }

    #[test]
    fn test_load_config_missing_remote_address() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let _guard_addr = EnvVarGuard::remove("REMOTE_ADDRESS");
        let _guard_rport = EnvVarGuard::set("REMOTE_PORT", "8080");
        let _guard_lport = EnvVarGuard::set("LOCAL_PORT", "9090");
        let _guard_type = EnvVarGuard::set("PROXY_TYPE", "tcp");
        let _guard_home = EnvVarGuard::remove("HOME");
        let _guard_xdg = EnvVarGuard::remove("XDG_CONFIG_HOME");

        assert!(load_config().is_err());
    }

    #[test]
    fn test_load_config_invalid_proxy_type() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let _guard_addr = EnvVarGuard::set("REMOTE_ADDRESS", "127.0.0.1");
        let _guard_rport = EnvVarGuard::set("REMOTE_PORT", "8080");
        let _guard_lport = EnvVarGuard::set("LOCAL_PORT", "9090");
        let _guard_type = EnvVarGuard::set("PROXY_TYPE", "invalid");
        let _guard_home = EnvVarGuard::remove("HOME");
        let _guard_xdg = EnvVarGuard::remove("XDG_CONFIG_HOME");

        let result = load_config();
        assert!(result.is_err());

        match result {
            Err(ProxyError::Configuration(_)) => {}
            _ => panic!("Expected ProxyError::Configuration"),
        }
    }

    #[test]
    fn test_load_config_invalid_ports() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let _guard_addr = EnvVarGuard::set("REMOTE_ADDRESS", "127.0.0.1");
        let _guard_lport = EnvVarGuard::set("LOCAL_PORT", "9090");
        let _guard_type = EnvVarGuard::set("PROXY_TYPE", "tcp");
        let _guard_home = EnvVarGuard::remove("HOME");
        let _guard_xdg = EnvVarGuard::remove("XDG_CONFIG_HOME");

        {
            let _guard_rport = EnvVarGuard::set("REMOTE_PORT", "invalid");
            assert!(load_config().is_err());
        }

        {
            let _guard_rport = EnvVarGuard::set("REMOTE_PORT", "8080");
            let _guard_lport_invalid = EnvVarGuard::set("LOCAL_PORT", "invalid");
            assert!(load_config().is_err());
        }
    }
}

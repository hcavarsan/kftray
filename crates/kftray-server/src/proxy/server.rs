use std::sync::Arc;

use log::info;
use tokio::sync::Notify;

use crate::proxy::{
    config::{
        ProxyConfig,
        ProxyType,
    },
    error::ProxyError,
    ssh::SshProxy,
    tcp::TcpProxy,
    traits::ProxyHandler,
    udp::UdpProxy,
};

/// Main proxy server that manages the lifecycle of proxy connections
pub struct ProxyServer {
    /// Server configuration
    config: ProxyConfig,
    /// Shutdown signal notifier
    shutdown: Arc<Notify>,
    /// Protocol-specific proxy handler (TCP or UDP)
    handler: Box<dyn ProxyHandler>,
}

impl ProxyServer {
    /// Creates a new proxy server instance with the given configuration
    ///
    /// # Parameters
    /// * `config` - Server configuration including proxy type and port settings
    pub fn new(config: ProxyConfig) -> Self {
        let handler: Box<dyn ProxyHandler> = match config.proxy_type {
            ProxyType::Tcp => Box::new(TcpProxy::new()),
            ProxyType::Udp => Box::new(UdpProxy::new()),
            ProxyType::Ssh => Box::new(SshProxy::new()),
        };

        Self {
            config,
            shutdown: Arc::new(Notify::new()),
            handler,
        }
    }

    /// Starts the proxy server and begins handling connections
    ///
    /// # Returns
    /// * `Result<(), ProxyError>` - Success if server runs and shuts down
    ///   cleanly
    pub async fn run(&self) -> Result<(), ProxyError> {
        self.handler
            .start(self.config.clone(), self.shutdown.clone())
            .await
    }

    /// Initiates a graceful shutdown of the proxy server
    pub fn shutdown(&self) {
        info!("Initiating server shutdown");
        self.shutdown.notify_waiters();
    }
}

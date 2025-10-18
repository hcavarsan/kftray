use std::sync::Arc;

use log::info;
use tokio::sync::Notify;

use crate::proxy::{
    config::{
        ProxyConfig,
        ProxyType,
    },
    error::ProxyError,
    reverse::ReverseProxy,
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
            ProxyType::ReverseHttp => Box::new(ReverseProxy::new()),
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{
        AtomicBool,
        Ordering,
    };

    use tokio::time::Duration;

    use super::*;

    struct MockHandler {
        started: Arc<AtomicBool>,
        shutdown_requested: Arc<AtomicBool>,
    }

    impl MockHandler {
        fn new() -> Self {
            Self {
                started: Arc::new(AtomicBool::new(false)),
                shutdown_requested: Arc::new(AtomicBool::new(false)),
            }
        }
    }

    #[async_trait::async_trait]
    impl ProxyHandler for MockHandler {
        async fn start(
            &self, _config: ProxyConfig, shutdown: Arc<Notify>,
        ) -> Result<(), ProxyError> {
            self.started.store(true, Ordering::SeqCst);
            shutdown.notified().await;
            self.shutdown_requested.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_server_lifecycle() {
        let config = ProxyConfig::builder()
            .target_host("127.0.0.1".to_string())
            .target_port(8080)
            .proxy_port(9090)
            .proxy_type(ProxyType::Tcp)
            .build()
            .unwrap();

        let mock_handler = MockHandler::new();
        let started = mock_handler.started.clone();
        let shutdown_requested = mock_handler.shutdown_requested.clone();

        let mut server = ProxyServer::new(config);
        server.handler = Box::new(mock_handler);

        let server = Arc::new(server);
        let server_clone = Arc::clone(&server);

        let handle = tokio::spawn(async move {
            server_clone.run().await.unwrap();
        });

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(started.load(Ordering::SeqCst));
        assert!(!shutdown_requested.load(Ordering::SeqCst));

        server.shutdown();
        handle.await.unwrap();

        assert!(shutdown_requested.load(Ordering::SeqCst));
    }

    #[test]
    fn test_server_creation_tcp() {
        let config = ProxyConfig::builder()
            .target_host("127.0.0.1".to_string())
            .target_port(8080)
            .proxy_port(9090)
            .proxy_type(ProxyType::Tcp)
            .build()
            .unwrap();

        let server = ProxyServer::new(config);
        assert!(matches!(server.config.proxy_type, ProxyType::Tcp));
    }

    #[test]
    fn test_server_creation_udp() {
        let config = ProxyConfig::builder()
            .target_host("127.0.0.1".to_string())
            .target_port(8080)
            .proxy_port(9090)
            .proxy_type(ProxyType::Udp)
            .build()
            .unwrap();

        let server = ProxyServer::new(config);
        assert!(matches!(server.config.proxy_type, ProxyType::Udp));
    }
}

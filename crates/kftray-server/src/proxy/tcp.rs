use std::{
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use log::{
    error,
    info,
};
use tokio::{
    io::copy_bidirectional,
    net::{
        TcpListener,
        TcpStream,
    },
    sync::Notify,
    time::timeout,
};

use crate::proxy::{
    config::ProxyConfig,
    error::ProxyError,
    traits::ProxyHandler,
};

/// TCP proxy implementation that forwards TCP connections to a target server
#[derive(Clone)]
pub struct TcpProxy;

impl TcpProxy {
    /// Creates a new TCP proxy instance
    pub fn new() -> Self {
        Self
    }

    /// Establishes connection to the target server with timeout
    /// Tries resolved IP first, then falls back to hostname
    ///
    /// # Parameters
    /// * `config` - Proxy configuration containing target details
    ///
    /// # Returns
    /// * `Result<TcpStream, ProxyError>` - Connected stream or error
    async fn connect_to_target(&self, config: &ProxyConfig) -> Result<TcpStream, ProxyError> {
        const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);

        if let Some(ref resolved_ip) = config.resolved_ip {
            info!(
                "Attempting connection to resolved IP {}:{}",
                resolved_ip, config.target_port
            );
            match timeout(
                CONNECTION_TIMEOUT,
                TcpStream::connect(format!("{}:{}", resolved_ip, config.target_port)),
            )
            .await
            {
                Ok(Ok(stream)) => {
                    info!(
                        "Connected to target via IP {}:{}",
                        resolved_ip, config.target_port
                    );
                    return Ok(stream);
                }
                Ok(Err(e)) => {
                    log::warn!(
                        "Failed to connect to resolved IP {}: {}. Trying hostname fallback.",
                        resolved_ip,
                        e
                    );
                }
                Err(_) => {
                    log::warn!(
                        "Connection timeout to resolved IP {}. Trying hostname fallback.",
                        resolved_ip
                    );
                }
            }
        }

        info!(
            "Attempting connection to hostname {}:{}",
            config.target_host, config.target_port
        );
        match timeout(
            CONNECTION_TIMEOUT,
            TcpStream::connect(format!("{}:{}", config.target_host, config.target_port)),
        )
        .await
        {
            Ok(Ok(stream)) => {
                info!(
                    "Connected to target via hostname {}:{}",
                    config.target_host, config.target_port
                );
                Ok(stream)
            }
            Ok(Err(e)) => {
                error!("Failed to connect to target: {e}");
                Err(ProxyError::Connection(format!(
                    "Failed to connect to target: {e}"
                )))
            }
            Err(_) => Err(ProxyError::Connection("Connection timeout".into())),
        }
    }

    /// Handles an individual TCP proxy connection
    ///
    /// Copies data bidirectionally between the client and target server
    ///
    /// # Parameters
    /// * `inbound` - Client connection stream
    /// * `config` - Proxy configuration
    async fn handle_tcp_connection(
        &self, inbound: TcpStream, config: &ProxyConfig,
    ) -> Result<(), ProxyError> {
        let outbound = self.connect_to_target(config).await?;
        let (mut inbound, mut outbound) = (inbound, outbound);

        match copy_bidirectional(&mut inbound, &mut outbound).await {
            Ok((from_client, from_server)) => {
                info!(
                    "Connection closed. Bytes from client: {from_client}, from server: {from_server}"
                );
                Ok(())
            }
            Err(e) if Self::is_connection_reset(&e) => {
                info!("Connection closed by peer");
                Ok(())
            }
            Err(e) => {
                error!("Connection error: {e}");
                Err(ProxyError::Io(e))
            }
        }
    }

    fn is_connection_reset(error: &std::io::Error) -> bool {
        matches!(
            error.kind(),
            std::io::ErrorKind::BrokenPipe
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::ConnectionAborted
        )
    }
}

#[async_trait]
impl ProxyHandler for TcpProxy {
    /// Starts the proxy server with the given configuration and shutdown
    /// signal.
    ///
    /// # Parameters
    /// * `config` - Configuration containing proxy settings like ports and
    ///   target details
    /// * `shutdown` - Notification mechanism to signal when the proxy should
    ///   stop
    ///
    /// # Returns
    /// * `Result<(), ProxyError>` - Success if proxy runs and shuts down
    ///   cleanly, or error details
    async fn start(&self, config: ProxyConfig, shutdown: Arc<Notify>) -> Result<(), ProxyError> {
        let addr: SocketAddr = format!("0.0.0.0:{}", config.proxy_port).parse()?;
        let listener = TcpListener::bind(addr).await?;

        info!("TCP Proxy started on port {}", config.proxy_port);

        loop {
            tokio::select! {
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, addr)) => {
                            info!("Accepted connection from {addr}");
                            let config = config.clone();
                            let proxy = self.clone();

                            tokio::spawn(async move {
                                if let Err(e) = proxy.handle_tcp_connection(stream, &config).await {
                                    error!("Connection error for {addr}: {e}");
                                }
                            });
                        }
                        Err(e) => error!("Failed to accept connection: {e}"),
                    }
                }
                _ = shutdown.notified() => {
                    info!("Shutdown signal received, stopping TCP proxy");
                    break;
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::SocketAddr,
        time::Duration,
    };

    use tokio::{
        io::{
            AsyncReadExt,
            AsyncWriteExt,
        },
        net::TcpStream,
    };

    use super::*;
    use crate::proxy::{
        config::ProxyType,
        test_utils::{
            self,
            TestServer,
        },
    };

    const TEST_TIMEOUT: Duration = Duration::from_secs(5);

    async fn setup_proxy() -> (TestServer, Arc<Notify>, SocketAddr) {
        let echo_server = test_utils::setup_test_tcp_echo_server().await;
        let proxy = TcpProxy::new();
        let shutdown = Arc::new(Notify::new());
        let shutdown_clone = shutdown.clone();

        // Bind to 127.0.0.1:0 first to get an available port
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let config = ProxyConfig::builder()
            .target_host(echo_server.addr().ip().to_string())
            .target_port(echo_server.addr().port())
            .proxy_port(addr.port())
            .proxy_type(ProxyType::Tcp)
            .build()
            .unwrap();

        tokio::spawn(async move {
            let _ = proxy.start(config, shutdown).await;
        });

        // Wait for the proxy to be ready
        assert!(
            test_utils::wait_for_port(addr).await,
            "Proxy failed to start"
        );

        (echo_server, shutdown_clone, addr)
    }

    #[tokio::test]
    async fn test_tcp_proxy_echo() {
        // Arrange
        let (echo_server, shutdown, proxy_addr) = setup_proxy().await;
        let test_data = b"Hello, proxy!";
        let mut response = vec![0; test_data.len()];

        // Act
        let mut stream = TcpStream::connect(proxy_addr).await.unwrap();
        stream.write_all(test_data).await.unwrap();
        stream.flush().await.unwrap();

        let n = tokio::time::timeout(TEST_TIMEOUT, stream.read_exact(&mut response))
            .await
            .unwrap()
            .unwrap();

        // Assert
        assert_eq!(n, test_data.len());
        assert_eq!(&response, test_data);

        // Cleanup
        shutdown.notify_one();
        echo_server.shutdown();
    }

    #[tokio::test]
    async fn test_tcp_proxy_large_data() {
        // Arrange
        let (echo_server, shutdown, proxy_addr) = setup_proxy().await;
        let test_data = vec![0x55; 1024 * 1024]; // 1MB of data
        let mut response = vec![0; test_data.len()];

        // Act
        let mut stream = TcpStream::connect(proxy_addr).await.unwrap();
        stream.write_all(&test_data).await.unwrap();
        stream.flush().await.unwrap();

        let n = tokio::time::timeout(TEST_TIMEOUT, stream.read_exact(&mut response))
            .await
            .unwrap()
            .unwrap();

        // Assert
        assert_eq!(n, test_data.len());
        assert_eq!(response, test_data);

        // Cleanup
        shutdown.notify_one();
        echo_server.shutdown();
    }

    #[tokio::test]
    async fn test_tcp_proxy_multiple_clients() {
        // Arrange
        let (echo_server, shutdown, proxy_addr) = setup_proxy().await;
        let test_data = b"Hello from client";
        let client_count = 5;
        let mut handles = Vec::new();

        // Act
        for i in 0..client_count {
            let addr = proxy_addr;
            let data = test_data.to_vec();
            handles.push(tokio::spawn(async move {
                let mut stream = TcpStream::connect(addr).await.unwrap();
                stream.write_all(&data).await.unwrap();
                stream.flush().await.unwrap();

                let mut response = vec![0; data.len()];
                let n = stream.read_exact(&mut response).await.unwrap();
                (i, n, response)
            }));
        }

        // Assert
        for handle in handles {
            let (client_id, n, response) = handle.await.unwrap();
            assert_eq!(
                n,
                test_data.len(),
                "Client {client_id} received wrong data length"
            );
            assert_eq!(
                &response, test_data,
                "Client {client_id} received incorrect data"
            );
        }

        // Cleanup
        shutdown.notify_one();
        echo_server.shutdown();
    }
}

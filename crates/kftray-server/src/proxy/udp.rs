use std::{
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use log::{
    debug,
    error,
    info,
};
use tokio::{
    io::{
        AsyncReadExt,
        AsyncWriteExt,
    },
    net::{
        TcpListener,
        TcpStream,
        UdpSocket,
    },
    sync::Notify,
};

use crate::proxy::{
    config::ProxyConfig,
    error::ProxyError,
    traits::ProxyHandler,
};

const UDP_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_UDP_PAYLOAD_SIZE: usize = 65507;

/// UDP proxy implementation that tunnels UDP traffic over TCP connections
#[derive(Clone)]
pub struct UdpProxy;

impl UdpProxy {
    /// Creates a new UDP proxy instance
    pub fn new() -> Self {
        Self
    }

    /// Creates and connects a UDP socket to the target server
    ///
    /// # Parameters
    /// * `config` - Proxy configuration containing target details
    ///
    /// # Returns
    /// * `Result<UdpSocket, ProxyError>` - Connected socket or error
    async fn create_udp_socket(&self, config: &ProxyConfig) -> Result<UdpSocket, ProxyError> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        socket
            .connect((config.target_host.as_str(), config.target_port))
            .await?;

        debug!(
            "Connected UDP socket to {}:{}",
            config.target_host, config.target_port
        );
        Ok(socket)
    }

    /// Handles a TCP connection carrying tunneled UDP traffic
    ///
    /// Forwards UDP packets between the TCP client and target UDP server
    ///
    /// # Parameters
    /// * `tcp_stream` - Client TCP connection
    /// * `config` - Proxy configuration
    async fn handle_udp_connection(
        &self, mut tcp_stream: TcpStream, config: &ProxyConfig,
    ) -> Result<(), ProxyError> {
        let udp_socket = self.create_udp_socket(config).await?;
        let mut size_buf = [0u8; 4];

        loop {
            match tcp_stream.read_exact(&mut size_buf).await {
                Ok(_) => {
                    let size = u32::from_be_bytes(size_buf);
                    debug!("Read size: {}", size);

                    if size as usize > MAX_UDP_PAYLOAD_SIZE {
                        let err = ProxyError::InvalidData(format!(
                            "UDP packet size {} exceeds maximum allowed {}",
                            size, MAX_UDP_PAYLOAD_SIZE
                        ));
                        tcp_stream.write_all(&0u32.to_be_bytes()).await?;
                        tcp_stream.flush().await?;
                        return Err(err);
                    }

                    let mut buffer = vec![0u8; size as usize];
                    match tcp_stream.read_exact(&mut buffer).await {
                        Ok(_) => {
                            debug!("Received {} bytes from TCP", size);
                            udp_socket.send(&buffer).await?;
                            debug!("Sent {} bytes to UDP", size);

                            self.handle_udp_response(&udp_socket, &mut tcp_stream)
                                .await?;
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                            debug!("TCP connection closed while reading payload");
                            break;
                        }
                        Err(e) => {
                            error!("Error reading TCP payload: {}", e);
                            return Err(ProxyError::Io(e));
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    debug!("TCP connection closed");
                    break;
                }
                Err(e) => {
                    error!("TCP read error: {}", e);
                    return Err(ProxyError::Io(e));
                }
            }
        }

        Ok(())
    }

    async fn handle_udp_response(
        &self, udp_socket: &UdpSocket, tcp_stream: &mut TcpStream,
    ) -> Result<(), ProxyError> {
        let mut response = vec![0u8; MAX_UDP_PAYLOAD_SIZE];

        match tokio::time::timeout(UDP_TIMEOUT, udp_socket.recv(&mut response)).await {
            Ok(Ok(n)) => {
                debug!("Received {} bytes from UDP", n);
                tcp_stream.write_all(&(n as u32).to_be_bytes()).await?;
                tcp_stream.write_all(&response[..n]).await?;
                tcp_stream.flush().await?;
                debug!("Sent response back to TCP client");
                Ok(())
            }
            Ok(Err(e)) => {
                error!("UDP receive error: {}", e);
                Err(ProxyError::Io(e))
            }
            Err(_) => {
                debug!("UDP response timeout");
                tcp_stream.write_all(&0u32.to_be_bytes()).await?;
                tcp_stream.flush().await?;
                Ok(())
            }
        }
    }
}

#[async_trait]
impl ProxyHandler for UdpProxy {
    async fn start(&self, config: ProxyConfig, shutdown: Arc<Notify>) -> Result<(), ProxyError> {
        let listener = TcpListener::bind(format!("0.0.0.0:{}", config.proxy_port)).await?;
        info!("UDP-over-TCP Proxy started on port {}", config.proxy_port);

        loop {
            tokio::select! {
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, addr)) => {
                            info!("Accepted connection from {}", addr);
                            let config = config.clone();
                            let proxy = self.clone();

                            tokio::spawn(async move {
                                if let Err(e) = proxy.handle_udp_connection(stream, &config).await {
                                    error!("Error handling client: {}", e);
                                }
                            });
                        }
                        Err(e) => error!("Failed to accept connection: {}", e),
                    }
                }
                _ = shutdown.notified() => {
                    info!("Shutdown signal received, stopping UDP proxy");
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
        let echo_server = test_utils::setup_test_udp_echo_server().await;
        let proxy = UdpProxy::new();
        let shutdown = Arc::new(Notify::new());
        let shutdown_clone = shutdown.clone();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let config = ProxyConfig::builder()
            .target_host(echo_server.addr().ip().to_string())
            .target_port(echo_server.addr().port())
            .proxy_port(addr.port())
            .proxy_type(ProxyType::Udp)
            .build()
            .unwrap();

        tokio::spawn(async move {
            let _ = proxy.start(config, shutdown).await;
        });

        assert!(
            test_utils::wait_for_port(addr).await,
            "Proxy failed to start"
        );

        (echo_server, shutdown_clone, addr)
    }

    async fn send_udp_packet(stream: &mut TcpStream, data: &[u8]) -> Result<Vec<u8>, ProxyError> {
        // Send packet size and data
        stream.write_all(&(data.len() as u32).to_be_bytes()).await?;
        stream.write_all(data).await?;
        stream.flush().await?;

        // Read response size
        let mut size_buf = [0u8; 4];
        stream.read_exact(&mut size_buf).await?;
        let response_size = u32::from_be_bytes(size_buf) as usize;

        // If response size is 0, this indicates an error
        if response_size == 0 {
            return Err(ProxyError::InvalidData("Oversized packet".into()));
        }

        // Read response data
        let mut response = vec![0u8; response_size];
        stream.read_exact(&mut response).await?;

        Ok(response)
    }

    #[tokio::test]
    async fn test_udp_proxy_echo() {
        // Arrange
        let (echo_server, shutdown, proxy_addr) = setup_proxy().await;
        let test_data = b"Hello, UDP proxy!";

        // Act
        let mut stream = TcpStream::connect(proxy_addr).await.unwrap();
        let response = tokio::time::timeout(TEST_TIMEOUT, send_udp_packet(&mut stream, test_data))
            .await
            .unwrap()
            .unwrap();

        // Assert
        assert_eq!(response, test_data);

        // Cleanup
        shutdown.notify_one();
        echo_server.shutdown();
    }

    #[tokio::test]
    async fn test_udp_proxy_large_packet() {
        // Arrange
        let (echo_server, shutdown, proxy_addr) = setup_proxy().await;
        let test_data = vec![0x55; 1024]; // 1KB packet

        // Act
        let mut stream = TcpStream::connect(proxy_addr).await.unwrap();
        let response = tokio::time::timeout(TEST_TIMEOUT, send_udp_packet(&mut stream, &test_data))
            .await
            .unwrap()
            .unwrap();

        // Assert
        assert_eq!(response, test_data);

        // Cleanup
        shutdown.notify_one();
        echo_server.shutdown();
    }

    #[tokio::test]
    async fn test_udp_proxy_multiple_packets() {
        // Arrange
        let (echo_server, shutdown, proxy_addr) = setup_proxy().await;
        let test_data = b"Packet";
        let packet_count = 5;

        // Act
        let mut stream = TcpStream::connect(proxy_addr).await.unwrap();

        for i in 0..packet_count {
            let response =
                tokio::time::timeout(TEST_TIMEOUT, send_udp_packet(&mut stream, test_data))
                    .await
                    .unwrap()
                    .unwrap();

            // Assert
            assert_eq!(response, test_data, "Packet {} was not echoed correctly", i);
        }

        // Cleanup
        shutdown.notify_one();
        echo_server.shutdown();
    }

    #[tokio::test]
    async fn test_udp_proxy_oversized_packet() {
        // Arrange
        let (echo_server, shutdown, proxy_addr) = setup_proxy().await;
        let oversized_data = vec![0; MAX_UDP_PAYLOAD_SIZE + 1];

        // Act
        let mut stream = TcpStream::connect(proxy_addr).await.unwrap();
        let result = send_udp_packet(&mut stream, &oversized_data).await;

        // Assert
        assert!(matches!(result, Err(ProxyError::InvalidData(_))));

        // Cleanup
        shutdown.notify_one();
        echo_server.shutdown();
    }
}

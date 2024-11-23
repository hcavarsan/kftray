use std::net::SocketAddr;
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
    time::Duration,
};

use crate::proxy::{
    config::ProxyConfig,
    error::ProxyError,
};

const UDP_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_UDP_PAYLOAD_SIZE: usize = 65507;

pub async fn start_proxy(
    config: ProxyConfig, shutdown: std::sync::Arc<Notify>,
) -> Result<SocketAddr, ProxyError> {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", config.proxy_port)).await?;
    let local_addr = listener.local_addr()?;
    info!("UDP-over-TCP Proxy started on port {}", config.proxy_port);

    loop {
        tokio::select! {
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, addr)) => {
                        info!("Accepted TCP connection from {}", addr);
                        let config = config.clone();
                        let shutdown = shutdown.clone();

                        tokio::spawn(async move {
                            if let Err(e) = handle_client(stream, config, shutdown).await {
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

    Ok(local_addr)
}

async fn handle_client(
    mut tcp_stream: TcpStream, config: ProxyConfig, shutdown: std::sync::Arc<Notify>,
) -> Result<(), ProxyError> {
    let udp_socket = UdpSocket::bind("0.0.0.0:0").await?;
    udp_socket
        .connect((config.target_host.as_str(), config.target_port))
        .await?;
    debug!(
        "Connected UDP socket to {}:{}",
        config.target_host, config.target_port
    );

    let mut size_buf = [0u8; 4];
    loop {
        tokio::select! {
            read_result = tcp_stream.read_exact(&mut size_buf) => {
                match read_result {
                    Ok(_) => {
                        let size = u32::from_be_bytes(size_buf);
                        debug!("Read size: {}", size);

                        if size as usize > MAX_UDP_PAYLOAD_SIZE {
                            return Err(ProxyError::InvalidData(format!(
                                "UDP packet size {} exceeds maximum allowed {}",
                                size, MAX_UDP_PAYLOAD_SIZE
                            )));
                        }

                        let mut buffer = vec![0u8; size as usize];
                        match tcp_stream.read_exact(&mut buffer).await {
                            Ok(_) => {
                                debug!("Received {} bytes from TCP", size);
                                udp_socket.send(&buffer).await?;
                                debug!("Sent {} bytes to UDP", size);

                                let mut response = vec![0u8; MAX_UDP_PAYLOAD_SIZE];
                                match tokio::time::timeout(UDP_TIMEOUT, udp_socket.recv(&mut response)).await {
                                    Ok(Ok(n)) => {
                                        debug!("Received {} bytes from UDP", n);
                                        // Write response size first
                                        tcp_stream.write_all(&(n as u32).to_be_bytes()).await?;
                                        // Then write the actual response
                                        tcp_stream.write_all(&response[..n]).await?;
                                        tcp_stream.flush().await?;
                                        debug!("Sent response back to TCP client");
                                    }
                                    Ok(Err(e)) => {
                                        error!("UDP receive error: {}", e);
                                        return Err(ProxyError::Io(e));
                                    }
                                    Err(_) => {
                                        error!("UDP response timeout");
                                        // Send empty response on timeout
                                        tcp_stream.write_all(&0u32.to_be_bytes()).await?;
                                        tcp_stream.flush().await?;
                                    }
                                }
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
            _ = shutdown.notified() => {
                debug!("Received shutdown signal");
                break;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use tokio::sync::oneshot;

    use super::*;

    async fn setup_test_udp_server() -> (SocketAddr, std::sync::Arc<Notify>) {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap();
        let shutdown = std::sync::Arc::new(Notify::new());
        let shutdown_clone = shutdown.clone();

        tokio::spawn(async move {
            let mut buf = vec![0; MAX_UDP_PAYLOAD_SIZE];
            while let Ok((n, peer)) = socket.recv_from(&mut buf).await {
                socket.send_to(&buf[..n], peer).await.unwrap();
            }
        });

        (addr, shutdown_clone)
    }

    #[tokio::test]
    async fn test_udp_proxy() {
        let (server_addr, _shutdown) = setup_test_udp_server().await;
        let (tx, rx) = oneshot::channel();

        let config = ProxyConfig::builder()
            .target_host(server_addr.ip().to_string())
            .target_port(server_addr.port())
            .proxy_port(0)
            .proxy_type(crate::proxy::config::ProxyType::Udp)
            .build()
            .unwrap();

        let shutdown = std::sync::Arc::new(Notify::new());
        let shutdown_clone = shutdown.clone();

        let proxy_handle = tokio::spawn(async move {
            let addr = start_proxy(config, shutdown).await.unwrap();
            tx.send(addr).unwrap();
        });

        // Wait for proxy to start and get its address
        let proxy_addr = rx.await.unwrap();

        // Connect to the actual proxy address
        let mut client = TcpStream::connect(proxy_addr).await.unwrap();
        let test_data = b"Hello, UDP proxy!";

        let size = test_data.len() as u32;
        let size_bytes = size.to_be_bytes();
        client.write_all(&size_bytes).await.unwrap();
        client.write_all(test_data).await.unwrap();

        let mut response_size = [0u8; 4];
        client.read_exact(&mut response_size).await.unwrap();
        let response_len = u32::from_be_bytes(response_size);

        let mut response = vec![0; response_len as usize];
        client.read_exact(&mut response).await.unwrap();

        assert_eq!(&response, test_data);

        shutdown_clone.notify_one();
        proxy_handle.await.unwrap();
    }
}

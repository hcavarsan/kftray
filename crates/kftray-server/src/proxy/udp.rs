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

    let _accept_handle = tokio::spawn({
        let shutdown = shutdown.clone();
        let config = config.clone();
        async move {
            loop {
                tokio::select! {
                    accept_result = listener.accept() => {
                        match accept_result {
                            Ok((stream, addr)) => {
                                info!("Accepted connection from {}", addr);
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
        }
    });

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
                                        tcp_stream.write_all(&(n as u32).to_be_bytes()).await?;
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

    async fn setup_test_udp_server() -> (SocketAddr, oneshot::Sender<()>) {
        eprintln!("Starting UDP server setup");
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap();
        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

        eprintln!("UDP test server bound to {}", addr);

        tokio::spawn(async move {
            eprintln!("UDP server task started");
            let mut buf = vec![0; MAX_UDP_PAYLOAD_SIZE];
            loop {
                tokio::select! {
                    result = socket.recv_from(&mut buf) => {
                        match result {
                            Ok((n, peer)) => {
                                eprintln!("UDP server received {} bytes from {}", n, peer);
                                if let Err(e) = socket.send_to(&buf[..n], peer).await {
                                    eprintln!("UDP server failed to send response: {}", e);
                                } else {
                                    eprintln!("UDP server sent response back to {}", peer);
                                }
                            }
                            Err(e) => eprintln!("UDP server receive error: {}", e),
                        }
                    }
                    _ = &mut shutdown_rx => {
                        eprintln!("UDP server received shutdown signal");
                        break;
                    }
                }
            }
            eprintln!("UDP server task ending");
        });

        eprintln!("UDP server setup complete");
        (addr, shutdown_tx)
    }

    #[tokio::test]
    async fn test_udp_proxy() {
        eprintln!("\n=== Starting UDP proxy test ===");

        let (server_addr, server_shutdown) = setup_test_udp_server().await;
        eprintln!("Test UDP server ready at {}", server_addr);

        let (tx, rx) = oneshot::channel();

        let config = ProxyConfig::builder()
            .target_host(server_addr.ip().to_string())
            .target_port(server_addr.port())
            .proxy_port(0) // Use dynamic port
            .proxy_type(crate::proxy::config::ProxyType::Udp)
            .build()
            .unwrap();

        let shutdown = std::sync::Arc::new(Notify::new());
        let shutdown_clone = shutdown.clone();

        eprintln!("Starting proxy server");
        let proxy_handle = tokio::spawn(async move {
            match start_proxy(config, shutdown).await {
                Ok(addr) => {
                    eprintln!("Proxy started successfully on {}", addr);
                    tx.send(addr).unwrap();
                }
                Err(e) => {
                    eprintln!("Failed to start proxy: {}", e);
                    panic!("Failed to start proxy: {}", e);
                }
            }
        });

        eprintln!("Waiting for proxy to start...");
        let proxy_addr = rx.await.expect("Failed to get proxy address");
        eprintln!("Proxy is listening on {}", proxy_addr);

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        eprintln!("Connecting to proxy at {}", proxy_addr);
        let mut client = match TcpStream::connect(proxy_addr).await {
            Ok(stream) => {
                eprintln!("Successfully connected to proxy");
                stream
            }
            Err(e) => {
                eprintln!("Failed to connect to proxy: {}", e);
                panic!("Connection failed");
            }
        };

        let test_data = b"Hello, UDP proxy!";
        let size = test_data.len() as u32;
        eprintln!("Sending {} bytes of data", size);

        if let Err(e) = client.write_all(&size.to_be_bytes()).await {
            eprintln!("Failed to write size: {}", e);
            panic!("Write failed");
        }

        if let Err(e) = client.write_all(test_data).await {
            eprintln!("Failed to write data: {}", e);
            panic!("Write failed");
        }
        eprintln!("Test data sent successfully");

        eprintln!("Reading response size");
        let mut response_size = [0u8; 4];
        if let Err(e) = client.read_exact(&mut response_size).await {
            eprintln!("Failed to read response size: {}", e);
            panic!("Read failed");
        }

        let response_len = u32::from_be_bytes(response_size);
        eprintln!("Response size: {} bytes", response_len);

        let mut response = vec![0; response_len as usize];
        eprintln!("Reading response data");
        if let Err(e) = client.read_exact(&mut response).await {
            eprintln!("Failed to read response data: {}", e);
            panic!("Read failed");
        }
        eprintln!("Response received");

        assert_eq!(&response, test_data);
        eprintln!("Response matches test data");

        eprintln!("Starting cleanup");
        shutdown_clone.notify_one();
        let _ = server_shutdown.send(());
        proxy_handle.await.unwrap();
        eprintln!("Test completed successfully");
    }
}

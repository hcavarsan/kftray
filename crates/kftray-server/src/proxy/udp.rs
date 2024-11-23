use byteorder::{
    BigEndian,
    ReadBytesExt,
    WriteBytesExt,
};
use log::{
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
};

pub async fn start_proxy(
    config: ProxyConfig, shutdown: std::sync::Arc<Notify>,
) -> Result<(), ProxyError> {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", config.proxy_port)).await?;
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

    Ok(())
}

async fn handle_client(
    tcp_stream: TcpStream, config: ProxyConfig, shutdown: std::sync::Arc<Notify>,
) -> Result<(), ProxyError> {
    let udp_socket = UdpSocket::bind("0.0.0.0:0").await?;
    udp_socket
        .connect((config.target_host, config.target_port))
        .await?;

    let (tcp_reader, tcp_writer) = tcp_stream.into_split();
    let udp_socket = std::sync::Arc::new(udp_socket);

    let tcp_to_udp = handle_tcp_to_udp(tcp_reader, udp_socket.clone(), shutdown.clone());
    let udp_to_tcp = handle_udp_to_tcp(udp_socket, tcp_writer, shutdown);

    tokio::select! {
        result = tcp_to_udp => result?,
        result = udp_to_tcp => result?,
    }

    Ok(())
}

async fn handle_tcp_to_udp(
    mut tcp_reader: impl AsyncReadExt + Unpin, udp_socket: std::sync::Arc<UdpSocket>,
    shutdown: std::sync::Arc<Notify>,
) -> Result<(), ProxyError> {
    let mut size_buf = [0u8; 4];

    loop {
        tokio::select! {
            read_result = tcp_reader.read_exact(&mut size_buf) => {
                match read_result {
                    Ok(_) => {
                        let mut rdr = &size_buf[..];
                        let size = ReadBytesExt::read_u32::<BigEndian>(&mut rdr)
                            .map_err(ProxyError::Io)?;
                        let mut buffer = vec![0u8; size as usize];
                        tcp_reader.read_exact(&mut buffer).await?;
                        udp_socket.send(&buffer).await?;
                    }
                    Err(e) => return Err(ProxyError::Io(e)),
                }
            }
            _ = shutdown.notified() => {
                break;
            }
        }
    }

    Ok(())
}

async fn handle_udp_to_tcp(
    udp_socket: std::sync::Arc<UdpSocket>, mut tcp_writer: impl AsyncWriteExt + Unpin,
    shutdown: std::sync::Arc<Notify>,
) -> Result<(), ProxyError> {
    let mut buffer = vec![0u8; 65535];

    loop {
        tokio::select! {
            recv_result = udp_socket.recv(&mut buffer) => {
                match recv_result {
                    Ok(size) => {
                        let mut size_buf = Vec::new();
                        WriteBytesExt::write_u32::<BigEndian>(&mut size_buf, size as u32)
                            .map_err(ProxyError::Io)?;
                        tcp_writer.write_all(&size_buf).await?;
                        tcp_writer.write_all(&buffer[..size]).await?;
                        tcp_writer.flush().await?;
                    }
                    Err(e) => return Err(ProxyError::Io(e)),
                }
            }
            _ = shutdown.notified() => {
                break;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use tokio::net::UdpSocket;

    use super::*;

    async fn setup_test_udp_server() -> (SocketAddr, std::sync::Arc<Notify>) {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap();
        let shutdown = std::sync::Arc::new(Notify::new());
        let shutdown_clone = shutdown.clone();

        tokio::spawn(async move {
            let mut buf = vec![0; 65535];
            while let Ok((n, peer)) = socket.recv_from(&mut buf).await {
                socket.send_to(&buf[..n], peer).await.unwrap();
            }
        });

        (addr, shutdown_clone)
    }

    #[tokio::test]
    async fn test_udp_proxy() {
        let (server_addr, _shutdown) = setup_test_udp_server().await;

        let config = ProxyConfig::new(
            server_addr.ip().to_string(),
            server_addr.port(),
            0,
            crate::proxy::config::ProxyType::Udp,
        );

        let shutdown = std::sync::Arc::new(Notify::new());
        let shutdown_clone = shutdown.clone();

        tokio::spawn(async move {
            start_proxy(config, shutdown).await.unwrap();
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let mut client = TcpStream::connect("127.0.0.1:0").await.unwrap();
        let test_data = b"Hello, UDP proxy!";

        let mut size_buf = Vec::new();
        WriteBytesExt::write_u32::<BigEndian>(&mut size_buf, test_data.len() as u32).unwrap();
        client.write_all(&size_buf).await.unwrap();
        client.write_all(test_data).await.unwrap();

        let mut response_size = [0u8; 4];
        client.read_exact(&mut response_size).await.unwrap();
        let mut rdr = &response_size[..];
        let response_len = ReadBytesExt::read_u32::<BigEndian>(&mut rdr).unwrap();

        let mut response = vec![0; response_len as usize];
        client.read_exact(&mut response).await.unwrap();

        assert_eq!(&response, test_data);

        shutdown_clone.notify_one();
    }
}

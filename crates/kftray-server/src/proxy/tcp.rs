use std::sync::atomic::{
    AtomicUsize,
    Ordering,
};

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
    },
    sync::Notify,
};

use crate::proxy::{
    config::ProxyConfig,
    error::ProxyError,
};

const BUFFER_SIZE: usize = 65536;
const MAX_CONNECTIONS: usize = 1000;

pub async fn start_proxy(
    config: ProxyConfig, shutdown: std::sync::Arc<Notify>,
) -> Result<(), ProxyError> {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", config.proxy_port)).await?;
    info!("TCP Proxy started on port {}", config.proxy_port);
    let connection_count = std::sync::Arc::new(AtomicUsize::new(0));

    loop {
        tokio::select! {
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, addr)) => {
                        let current_connections = connection_count.load(Ordering::Relaxed);
                        if current_connections >= MAX_CONNECTIONS {
                            error!("Maximum connection limit reached, rejecting connection from {}", addr);
                            continue;
                        }
                        connection_count.fetch_add(1, Ordering::Relaxed);
                        info!("Accepted connection from {}", addr);
                        let config = config.clone();
                        let shutdown = shutdown.clone();
                        let connection_count = connection_count.clone();

                        tokio::spawn(async move {
                            if let Err(e) = handle_client(stream, config, shutdown).await {
                                error!("Error handling client: {}", e);
                            }
                            connection_count.fetch_sub(1, Ordering::Relaxed);
                        });
                    }
                    Err(e) => error!("Failed to accept connection: {}", e),
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

async fn handle_client(
    client_stream: TcpStream, config: ProxyConfig, shutdown: std::sync::Arc<Notify>,
) -> Result<(), ProxyError> {
    let server_stream =
        TcpStream::connect(format!("{}:{}", config.target_host, config.target_port))
            .await
            .map_err(|e| ProxyError::Connection(format!("Failed to connect to target: {}", e)))?;

    info!(
        "Connected to target {}:{}",
        config.target_host, config.target_port
    );

    client_stream.set_nodelay(true)?;
    server_stream.set_nodelay(true)?;

    let (client_reader, client_writer) = client_stream.into_split();
    let (server_reader, server_writer) = server_stream.into_split();

    let client_to_server = relay_stream(client_reader, server_writer, shutdown.clone());
    let server_to_client = relay_stream(server_reader, client_writer, shutdown);

    tokio::try_join!(client_to_server, server_to_client)?;

    Ok(())
}

async fn relay_stream(
    mut read_stream: impl AsyncReadExt + Unpin, mut write_stream: impl AsyncWriteExt + Unpin,
    shutdown: std::sync::Arc<Notify>,
) -> Result<(), ProxyError> {
    let mut buffer = vec![0u8; BUFFER_SIZE];

    loop {
        tokio::select! {
            read_result = read_stream.read(&mut buffer) => {
                match read_result {
                    Ok(0) => {
                        debug!("Stream closed by peer");
                        break;
                    }
                    Ok(n) => {
                        debug!("Relaying {} bytes", n);
                        write_stream.write_all(&buffer[..n]).await?;
                        write_stream.flush().await?;
                    }
                    Err(e) => {
                        error!("Read error: {}", e);
                        return Err(ProxyError::Io(e));
                    }
                }
            }
            _ = shutdown.notified() => {
                debug!("Relay shutdown requested");
                break;
            }
        }
    }

    write_stream.shutdown().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use tokio::net::TcpSocket;

    use super::*;

    async fn setup_test_server() -> (SocketAddr, std::sync::Arc<Notify>) {
        let socket = TcpSocket::new_v4().unwrap();
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        socket.bind(addr).unwrap();
        let listener = socket.listen(1024).unwrap();
        let addr = listener.local_addr().unwrap();
        let shutdown = std::sync::Arc::new(Notify::new());
        let shutdown_clone = shutdown.clone();

        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let (mut reader, mut writer) = socket.split();
                let mut buf = vec![0; 1024];
                while let Ok(n) = reader.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                    writer.write_all(&buf[..n]).await.unwrap();
                }
            }
        });

        (addr, shutdown_clone)
    }

    #[tokio::test]
    async fn test_tcp_proxy() {
        let (server_addr, _shutdown) = setup_test_server().await;

        // Use a specific port for testing
        let proxy_port = 50000;

        let config = ProxyConfig::builder()
            .target_host(server_addr.ip().to_string())
            .target_port(server_addr.port())
            .proxy_port(proxy_port)
            .proxy_type(crate::proxy::config::ProxyType::Tcp)
            .build()
            .unwrap();

        let shutdown = std::sync::Arc::new(Notify::new());
        let shutdown_clone = shutdown.clone();

        let proxy_handle = tokio::spawn(async move {
            start_proxy(config, shutdown).await.unwrap();
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let mut client = TcpStream::connect(format!("127.0.0.1:{}", proxy_port))
            .await
            .unwrap();

        let test_data = b"Hello, proxy!";
        client.write_all(test_data).await.unwrap();

        let mut response = vec![0; test_data.len()];
        client.read_exact(&mut response).await.unwrap();

        assert_eq!(&response, test_data);

        shutdown_clone.notify_one();
        proxy_handle.await.unwrap();
    }
}

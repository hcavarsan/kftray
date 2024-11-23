use std::time::Duration;

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
    },
    sync::Notify,
    time,
};

use crate::proxy::{
    config::ProxyConfig,
    error::ProxyError,
};

const BUFFER_SIZE: usize = 65536;
const MAX_RETRIES: u32 = 5;
const INITIAL_RETRY_DELAY: Duration = Duration::from_millis(100);

pub async fn start_proxy(
    config: ProxyConfig, shutdown: std::sync::Arc<Notify>,
) -> Result<(), ProxyError> {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", config.proxy_port)).await?;
    info!("HTTP Proxy started on port {}", config.proxy_port);

    loop {
        tokio::select! {
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((client_stream, addr)) => {
                        info!("Accepted connection from {}", addr);
                        let config = config.clone();
                        let shutdown = shutdown.clone();

                        tokio::spawn(async move {
                            if let Err(e) = handle_client(client_stream, config, shutdown).await {
                                error!("Error handling client: {}", e);
                            }
                        });
                    }
                    Err(e) => error!("Failed to accept connection: {}", e),
                }
            }
            _ = shutdown.notified() => {
                info!("Shutdown signal received, stopping HTTP proxy");
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

    let (client_reader, client_writer) = client_stream.into_split();
    let (server_reader, server_writer) = server_stream.into_split();

    let client_to_server = relay_stream(client_reader, server_writer, shutdown.clone());
    let server_to_client = relay_stream(server_reader, client_writer, shutdown);

    tokio::select! {
        result = client_to_server => result?,
        result = server_to_client => result?,
    }

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
                    Ok(0) => break,
                    Ok(n) => {
                        retryable_write(&mut write_stream, &buffer[..n]).await?;
                    }
                    Err(e) => return Err(ProxyError::Io(e)),
                }
            }
            _ = shutdown.notified() => {
                break;
            }
        }
    }

    write_stream.shutdown().await?;
    Ok(())
}

async fn retryable_write(
    writer: &mut (impl AsyncWriteExt + Unpin), buf: &[u8],
) -> Result<(), ProxyError> {
    let mut attempts = 0;
    let mut delay = INITIAL_RETRY_DELAY;

    loop {
        match writer.write_all(buf).await {
            Ok(()) => return Ok(()),
            Err(_) if attempts < MAX_RETRIES => {
                attempts += 1;
                time::sleep(delay).await;
                delay *= 2;
            }
            Err(e) => return Err(ProxyError::Io(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::watch;

    use super::*;

    async fn setup_test_server() -> (u16, watch::Sender<bool>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        tokio::spawn(async move {
            while !*shutdown_rx.borrow() {
                if let Ok((mut socket, _)) = listener.accept().await {
                    let (mut reader, mut writer) = socket.split();
                    let mut buf = vec![0; 1024];
                    if let Ok(n) = reader.read(&mut buf).await {
                        writer.write_all(&buf[..n]).await.unwrap();
                    }
                }
            }
        });

        (port, shutdown_tx)
    }

    #[tokio::test]
    async fn test_proxy_relay() {
        let (echo_port, _shutdown) = setup_test_server().await;

        let config = ProxyConfig::builder()
            .target_host("127.0.0.1".to_string())
            .target_port(echo_port)
            .proxy_port(0)
            .proxy_type(crate::proxy::config::ProxyType::Http)
            .build()
            .unwrap();

        let shutdown = std::sync::Arc::new(Notify::new());

        tokio::spawn(async move {
            start_proxy(config, shutdown).await.unwrap();
        });
    }
}

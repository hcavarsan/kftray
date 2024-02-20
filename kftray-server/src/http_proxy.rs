use log::{error, info};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

async fn handle_client(
    client_stream: TcpStream,
    server_stream: TcpStream,
    shutdown_notify: Arc<Notify>,
) -> std::io::Result<()> {
    let (mut client_reader, mut client_writer) = client_stream.into_split();
    let (mut server_reader, mut server_writer) = server_stream.into_split();

    let client_to_server = tokio::spawn(async move {
        let mut buf = [0; 1024];
        loop {
            let read_size = match client_reader.read(&mut buf).await {
                Ok(0) => return Ok(()),
                Ok(size) => size,
                Err(e) => {
                    error!("Error reading from client: {}", e);
                    return Err(e);
                }
            };

            if let Err(e) = server_writer.write_all(&buf[..read_size]).await {
                error!("Error writing to server: {}", e);
                return Err(e);
            }
        }
    });

    let server_to_client = tokio::spawn(async move {
        let mut buf = [0; 1024];
        loop {
            let read_size = match server_reader.read(&mut buf).await {
                Ok(0) => return Ok(()),
                Ok(size) => size,
                Err(e) => {
                    error!("Error reading from server: {}", e);
                    return Err(e);
                }
            };

            if let Err(e) = client_writer.write_all(&buf[..read_size]).await {
                error!("Error writing to client: {}", e);
                return Err(e);
            }
        }
    });

    let _ = tokio::try_join!(client_to_server, server_to_client)?;

    shutdown_notify.notified().await;

    Ok(())
}

pub async fn start_http_proxy(
    target_host: &str,
    target_port: u16,
    proxy_port: u16,
    is_running: Arc<AtomicBool>,
    shutdown_notify: Arc<Notify>,
) -> std::io::Result<()> {
    let tcp_listener = TcpListener::bind(format!("0.0.0.0:{}", proxy_port)).await?;
    info!("HTTP Proxy started on port {}", proxy_port);

    while is_running.load(Ordering::SeqCst) {
        let (client_stream, _) = tcp_listener.accept().await?;
        let server_stream = TcpStream::connect(format!("{}:{}", target_host, target_port)).await?;
        let shutdown_notify_clone = shutdown_notify.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_client(client_stream, server_stream, shutdown_notify_clone).await
            {
                error!("Error while handling client: {}", e);
            }
        });
    }

    Ok(())
}

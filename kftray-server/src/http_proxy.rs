use log::{error, info};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio::time::{self, Duration};

const MAX_RETRIES: u32 = 5;
const RETRY_DELAY: Duration = Duration::from_secs(1);

async fn retryable_write(writer: &mut (impl AsyncWriteExt + Unpin), buf: &[u8]) -> io::Result<()> {
    let mut attempts = 0;
    loop {
        match writer.write_all(buf).await {
            Ok(()) => return Ok(()),
            Err(_e) if attempts < MAX_RETRIES => {
                attempts += 1;
                time::sleep(RETRY_DELAY).await;
            }
            Err(e) => return Err(e),
        }
    }
}

async fn handle_client(
    client_stream: TcpStream,
    server_stream: TcpStream,
    shutdown_notify: Arc<Notify>,
) -> std::io::Result<()> {
    let (mut client_reader, mut client_writer) = client_stream.into_split();
    let (mut server_reader, mut server_writer) = server_stream.into_split();

    let client_to_server = tokio::spawn(async move {
        let mut buf = vec![0; 4096];
        loop {
            let n = client_reader.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            retryable_write(&mut server_writer, &buf[..n]).await?;
        }
        Ok::<(), io::Error>(())
    });

    let server_to_client = tokio::spawn(async move {
        let mut buf = vec![0; 4096];
        loop {
            let n = server_reader.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            retryable_write(&mut client_writer, &buf[..n]).await?;
        }
        Ok::<(), io::Error>(())
    });

    tokio::select! {
        result = client_to_server => Ok(result??),
        result = server_to_client => Ok(result??),
        _ = shutdown_notify.notified() => Ok(()),
    }
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

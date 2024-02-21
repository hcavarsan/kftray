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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::net::TcpListener;
    use tokio::sync::watch;
    use tokio::time::{self};

    async fn start_echo_server() -> io::Result<(u16, watch::Sender<bool>)> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let local_port = listener.local_addr()?.port();
        let (shutdown_sender, mut shutdown_receiver) = watch::channel(false);

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    accept_result = listener.accept() => {
                        if let Ok((mut socket, _)) = accept_result {
                            let (mut reader, mut writer) = socket.split();
                            let mut buffer = [0; 1024];
                            while let Ok(read_bytes) = reader.read(&mut buffer).await {
                                if read_bytes == 0 {
                                    break;
                                }
                                writer.write_all(&buffer[..read_bytes]).await.unwrap();
                            }
                        }
                    }
                    _ = shutdown_receiver.changed() => {
                        if *shutdown_receiver.borrow() {
                            break;
                        }
                    }
                }
            }
        });

        Ok((local_port, shutdown_sender))
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_retryable_write_success() {
        let (echo_port, shutdown_sender) = start_echo_server().await.unwrap();
        let mut stream = TcpStream::connect(("127.0.0.1", echo_port)).await.unwrap();
        let message = "test message";
        retryable_write(&mut stream, message.as_bytes())
            .await
            .unwrap();

        shutdown_sender.send(true).unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_start_http_proxy() {
        let (echo_port, shutdown_sender) = start_echo_server().await.unwrap();
        let is_running = Arc::new(AtomicBool::new(true));
        let shutdown_notify = Arc::new(Notify::new());
        let proxy_port = {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            listener.local_addr().unwrap().port()
        };

        let target_host = "127.0.0.1";

        let is_running_clone = is_running.clone();
        let shutdown_notify_clone = shutdown_notify.clone();

        tokio::spawn(async move {
            if let Err(e) = start_http_proxy(
                target_host,
                echo_port,
                proxy_port,
                is_running_clone,
                shutdown_notify_clone,
            )
            .await
            {
                eprintln!("HTTP Proxy failed: {:?}", e);
            }
        });

        time::sleep(Duration::from_secs(1)).await;

        let mut stream = TcpStream::connect(("127.0.0.1", proxy_port)).await.unwrap();
        let message = "test message through proxy";
        retryable_write(&mut stream, message.as_bytes())
            .await
            .unwrap();

        let mut buffer = vec![0; message.len()];
        stream.read_exact(&mut buffer).await.unwrap();
        assert_eq!(message.as_bytes(), &buffer[..]);

        is_running.store(false, Ordering::SeqCst);
        shutdown_notify.notify_waiters();

        shutdown_sender.send(true).unwrap();

        time::sleep(Duration::from_secs(1)).await;
    }
}

use std::sync::{
    atomic::{
        AtomicBool,
        Ordering,
    },
    Arc,
};

use log::{
    error,
    info,
    warn,
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
    time::{
        self,
        Duration,
    },
};

const MAX_RETRIES: u32 = 5;

const RETRY_DELAY: Duration = Duration::from_secs(1);

async fn handle_client(
    client_stream: TcpStream, server_stream: TcpStream, shutdown_notify: Arc<Notify>,
) -> std::io::Result<()> {
    let (client_reader, client_writer) = tokio::io::split(client_stream);

    let (server_reader, server_writer) = server_stream.into_split();

    let client_to_server = relay_stream(client_reader, server_writer, shutdown_notify.clone());

    let server_to_client = relay_stream(server_reader, client_writer, shutdown_notify);

    tokio::select! {
        result = client_to_server => result,
        result = server_to_client => result,
    }
}

async fn relay_stream(
    mut read_stream: impl AsyncReadExt + Unpin, mut write_stream: impl AsyncWriteExt + Unpin,
    shutdown_notify: Arc<Notify>,
) -> std::io::Result<()> {
    let mut buffer = vec![0u8; 65536];

    loop {
        tokio::select! {
            read_result = read_stream.read(&mut buffer) => {
                match read_result {
                    Ok(0) => {
                        break;
                    },
                    Ok(n) => {
                        retryable_write(&mut write_stream, &buffer[..n]).await?;
                    },
                    Err(e) => {
                        error!("Failed to read from stream: {}", e);
                        return Err(e);
                    },
                }
            },

            _ = shutdown_notify.notified() => {
                info!("Shutdown signal received.");
                break;
            },
        };
    }

    write_stream.shutdown().await?;

    Ok(())
}

async fn retryable_write(
    writer: &mut (impl AsyncWriteExt + Unpin), buf: &[u8],
) -> std::io::Result<()> {
    let mut attempts = 0;

    loop {
        match writer.write_all(buf).await {
            Ok(()) => return Ok(()),
            Err(e) if attempts < MAX_RETRIES => {
                warn!(
                    "Failed to write to stream, attempt {}: {}. Retrying in {} seconds...",
                    attempts + 1,
                    e,
                    RETRY_DELAY.as_secs()
                );

                attempts += 1;

                time::sleep(RETRY_DELAY).await;
            }
            Err(e) => {
                error!(
                    "Failed to write to stream after {} attempts: {}.",
                    attempts, e
                );

                return Err(e);
            }
        }
    }
}

pub async fn start_http_proxy(
    target_host: &str, target_port: u16, proxy_port: u16, is_running: Arc<AtomicBool>,
    shutdown_notify: Arc<Notify>,
) -> std::io::Result<()> {
    let tcp_listener = TcpListener::bind(format!("0.0.0.0:{}", proxy_port)).await?;

    info!("HTTP Proxy started on port {}", proxy_port);

    while is_running.load(Ordering::SeqCst) {
        let (client_stream, peer_addr) = match tcp_listener.accept().await {
            Ok((stream, addr)) => (stream, addr),
            Err(e) => {
                error!("Failed to accept client: {}", e);

                continue;
            }
        };

        info!("Accepted connection from {}", peer_addr);

        let server_stream =
            match TcpStream::connect(format!("{}:{}", target_host, target_port)).await {
                Ok(stream) => stream,
                Err(e) => {
                    error!(
                        "Failed to connect to server at {}:{}: {}",
                        target_host, target_port, e
                    );

                    continue;
                }
            };

        info!("Connected to server at {}:{}", target_host, target_port);

        let shutdown_notify_clone = shutdown_notify.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_client(client_stream, server_stream, shutdown_notify_clone).await
            {
                error!("Error while handling client: {}", e);
            }
        });
    }

    info!("HTTP Proxy stopped.");

    Ok(())
}

#[cfg(test)]

mod tests {

    use tokio::sync::watch;

    use super::*;

    async fn start_echo_server() -> std::io::Result<(u16, watch::Sender<bool>)> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;

        let local_port = listener.local_addr()?.port();

        let (shutdown_sender, mut shutdown_receiver) = watch::channel(false);

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    accept_result = listener.accept() => {
                        if let Ok((mut socket, _)) = accept_result {
                            let (mut reader, mut writer) = socket.split();
                            let mut buffer = [0; 65536];
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

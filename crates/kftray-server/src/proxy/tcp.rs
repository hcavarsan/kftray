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
    time::Duration,
};

use crate::proxy::{
    config::ProxyConfig,
    error::ProxyError,
};

const BUFFER_SIZE: usize = 65536;
const MAX_CONNECTIONS: usize = 1000;
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
const READ_TIMEOUT: Duration = Duration::from_secs(600);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);
const KEEPALIVE_RETRY_COUNT: u32 = 5;

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
    client_stream.set_nodelay(true)?;

    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = client_stream.as_raw_fd();

        unsafe {
            let optval: libc::c_int = 1;
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_KEEPALIVE,
                &optval as *const _ as *const libc::c_void,
                std::mem::size_of_val(&optval) as libc::socklen_t,
            );

            #[cfg(target_os = "macos")]
            {
                let keepalive_time = KEEPALIVE_INTERVAL.as_secs() as libc::c_int;
                libc::setsockopt(
                    fd,
                    libc::IPPROTO_TCP,
                    libc::TCP_KEEPALIVE,
                    &keepalive_time as *const _ as *const libc::c_void,
                    std::mem::size_of_val(&keepalive_time) as libc::socklen_t,
                );
            }

            #[cfg(target_os = "linux")]
            {
                let keepalive_time = KEEPALIVE_INTERVAL.as_secs() as libc::c_int;
                libc::setsockopt(
                    fd,
                    libc::IPPROTO_TCP,
                    libc::TCP_KEEPIDLE,
                    &keepalive_time as *const _ as *const libc::c_void,
                    std::mem::size_of_val(&keepalive_time) as libc::socklen_t,
                );
            }

            #[cfg(any(target_os = "linux", target_os = "macos"))]
            {
                let keepalive_retry = KEEPALIVE_RETRY_COUNT as libc::c_int;
                libc::setsockopt(
                    fd,
                    libc::IPPROTO_TCP,
                    libc::TCP_KEEPCNT,
                    &keepalive_retry as *const _ as *const libc::c_void,
                    std::mem::size_of_val(&keepalive_retry) as libc::socklen_t,
                );

                let keepalive_intvl = 5 as libc::c_int;
                libc::setsockopt(
                    fd,
                    libc::IPPROTO_TCP,
                    libc::TCP_KEEPINTVL,
                    &keepalive_intvl as *const _ as *const libc::c_void,
                    std::mem::size_of_val(&keepalive_intvl) as libc::socklen_t,
                );
            }
        }
    }

    let server_stream = match tokio::time::timeout(
        CONNECT_TIMEOUT,
        TcpStream::connect(format!("{}:{}", config.target_host, config.target_port)),
    )
    .await
    {
        Ok(Ok(stream)) => {
            info!(
                "Connected to target {}:{}",
                config.target_host, config.target_port
            );
            stream
        }
        Ok(Err(e)) => {
            error!("Failed to connect to target: {}", e);
            return Err(ProxyError::Connection(format!(
                "Failed to connect to target: {}",
                e
            )));
        }
        Err(_) => {
            error!(
                "Connection to target timed out after {} seconds",
                CONNECT_TIMEOUT.as_secs()
            );
            return Err(ProxyError::Connection(format!(
                "Connection timeout after {} seconds",
                CONNECT_TIMEOUT.as_secs()
            )));
        }
    };

    server_stream.set_nodelay(true)?;

    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = server_stream.as_raw_fd();

        unsafe {
            let optval: libc::c_int = 1;
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_KEEPALIVE,
                &optval as *const _ as *const libc::c_void,
                std::mem::size_of_val(&optval) as libc::socklen_t,
            );
        }
    }

    let (client_reader, client_writer) = client_stream.into_split();
    let (server_reader, server_writer) = server_stream.into_split();

    let client_to_server = relay_stream(
        "client->server",
        client_reader,
        server_writer,
        shutdown.clone(),
    );
    let server_to_client = relay_stream("server->client", server_reader, client_writer, shutdown);

    match tokio::try_join!(client_to_server, server_to_client) {
        Ok(_) => {
            debug!("Connection closed gracefully");
            Ok(())
        }
        Err(e) => match e {
            ProxyError::Io(ref io_err) if io_err.kind() == std::io::ErrorKind::BrokenPipe => {
                debug!("Connection closed by peer (broken pipe)");
                Ok(())
            }
            ProxyError::Io(ref io_err) if io_err.kind() == std::io::ErrorKind::ConnectionReset => {
                debug!("Connection reset by peer");
                Ok(())
            }
            _ => {
                error!("Connection error: {}", e);
                Err(e)
            }
        },
    }
}

async fn relay_stream(
    direction: &'static str, mut read_stream: impl AsyncReadExt + Unpin,
    mut write_stream: impl AsyncWriteExt + Unpin, shutdown: std::sync::Arc<Notify>,
) -> Result<(), ProxyError> {
    let mut buffer = vec![0u8; BUFFER_SIZE];
    let mut consecutive_timeouts = 0;
    const MAX_CONSECUTIVE_TIMEOUTS: u32 = 3;

    loop {
        tokio::select! {
            read_result = tokio::time::timeout(READ_TIMEOUT, read_stream.read(&mut buffer)) => {
                match read_result {
                    Ok(Ok(0)) => {
                        debug!("[{}] Stream closed by peer", direction);
                        break;
                    }
                    Ok(Ok(n)) => {
                        consecutive_timeouts = 0;
                        debug!("[{}] Relaying {} bytes", direction, n);

                        match tokio::time::timeout(
                            WRITE_TIMEOUT,
                            write_stream.write_all(&buffer[..n])
                        ).await {
                            Ok(Ok(())) => {
                                if let Err(e) = write_stream.flush().await {
                                    if e.kind() != std::io::ErrorKind::BrokenPipe {
                                        error!("[{}] Flush error: {}", direction, e);
                                        return Err(ProxyError::Io(e));
                                    }
                                    break;
                                }
                            }
                            Ok(Err(e)) => {
                                if e.kind() != std::io::ErrorKind::BrokenPipe {
                                    error!("[{}] Write error: {}", direction, e);
                                    return Err(ProxyError::Io(e));
                                }
                                break;
                            }
                            Err(_) => {
                                error!("[{}] Write timeout", direction);
                                return Err(ProxyError::Connection("Write timeout".into()));
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        if e.kind() != std::io::ErrorKind::ConnectionReset {
                            error!("[{}] Read error: {}", direction, e);
                            return Err(ProxyError::Io(e));
                        }
                        break;
                    }
                    Err(_) => {
                        consecutive_timeouts += 1;
                        if consecutive_timeouts >= MAX_CONSECUTIVE_TIMEOUTS {
                            error!("[{}] Maximum consecutive read timeouts reached", direction);
                            return Err(ProxyError::Connection("Maximum read timeouts exceeded".into()));
                        }
                        debug!("[{}] Read timeout ({}/{}), connection idle",
                            direction, consecutive_timeouts, MAX_CONSECUTIVE_TIMEOUTS);
                        continue;
                    }
                }
            }
            _ = shutdown.notified() => {
                debug!("[{}] Relay shutdown requested", direction);
                break;
            }
        }
    }

    if let Err(e) = write_stream.shutdown().await {
        debug!("[{}] Shutdown error (expected): {}", direction, e);
    }
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

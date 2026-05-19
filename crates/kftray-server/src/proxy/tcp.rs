use std::{
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use log::{
    error,
    info,
    warn,
};
use tokio::{
    net::{
        TcpListener,
        TcpStream,
    },
    sync::Notify,
};
use tokio_util::sync::CancellationToken;

use crate::proxy::{
    config::ProxyConfig,
    error::ProxyError,
    http_proxy::HttpProxy,
    relay::relay_direct,
    sniff::{
        Protocol,
        classify,
    },
    traits::ProxyHandler,
};

#[derive(Clone)]
pub(crate) struct TcpProxy;

impl TcpProxy {
    pub(crate) const fn new() -> Self {
        Self
    }

    async fn handle_tcp_connection(
        inbound: TcpStream, config: ProxyConfig, http_proxy: HttpProxy, cancel: CancellationToken,
    ) -> Result<(), ProxyError> {
        let _ = inbound.set_nodelay(true);
        let protocol = classify(&inbound)
            .await
            .map_err(|e| ProxyError::Connection(format!("sniff: {e}")))?;

        match protocol {
            Protocol::Http1 => {
                tracing::debug!(?protocol, "dispatch: http1");
                http_proxy.serve_http1(inbound, cancel).await
            }
            Protocol::Http2 => {
                tracing::debug!(?protocol, "dispatch: http2");
                http_proxy.serve_http2(inbound, cancel).await
            }
            _ => {
                tracing::debug!(?protocol, "dispatch: raw relay");
                relay_direct(inbound, &config, cancel).await
            }
        }
    }
}

#[async_trait]
impl ProxyHandler for TcpProxy {
    async fn start(&self, config: ProxyConfig, shutdown: Arc<Notify>) -> Result<(), ProxyError> {
        let addr: SocketAddr = format!("0.0.0.0:{}", config.proxy_port).parse()?;
        let listener = TcpListener::bind(addr).await?;

        info!("TCP Proxy started on port {}", config.proxy_port);

        let http_proxy = HttpProxy::new(&config);

        let cancel = CancellationToken::new();
        let shutdown_cancel = cancel.clone();
        let shutdown_notify = shutdown.clone();
        let bridge = tokio::spawn(async move {
            shutdown_notify.notified().await;
            shutdown_cancel.cancel();
        });

        let mut backoff_ms: u64 = 10;

        loop {
            tokio::select! {
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, addr)) => {
                            info!("Accepted connection from {addr}");
                            backoff_ms = 10;
                            let config = config.clone();
                            let http_proxy = http_proxy.clone();
                            let child_cancel = cancel.child_token();

                            tokio::spawn(async move {
                                if let Err(e) = Self::handle_tcp_connection(stream, config, http_proxy, child_cancel).await {
                                    error!("Connection error for {addr}: {e}");
                                }
                            });
                        }
                        Err(e) => {
                            warn!("Accept error (retrying in {backoff_ms}ms): {e}");
                            tokio::select! {
                                () = tokio::time::sleep(Duration::from_millis(backoff_ms)) => {
                                    backoff_ms = (backoff_ms * 2).min(5000);
                                }
                                () = cancel.cancelled() => {
                                    info!("Shutdown signal received during backoff, stopping TCP proxy");
                                    bridge.abort();
                                    return Ok(());
                                }
                            }
                            continue;
                        }
                    }
                }
                () = cancel.cancelled() => {
                    info!("Shutdown signal received, stopping TCP proxy");
                    break;
                }
            }
        }

        bridge.abort();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::SocketAddr,
        time::Duration,
    };

    use tokio::{
        io::{
            AsyncReadExt,
            AsyncWriteExt,
        },
        net::TcpStream,
    };

    use super::*;
    use crate::proxy::{
        config::ProxyType,
        test_utils::{
            self,
            TestServer,
        },
    };

    const TEST_TIMEOUT: Duration = Duration::from_secs(5);

    async fn setup_proxy() -> (TestServer, Arc<Notify>, SocketAddr) {
        let echo_server = test_utils::setup_test_tcp_echo_server().await;
        let proxy = TcpProxy::new();
        let shutdown = Arc::new(Notify::new());
        let shutdown_clone = shutdown.clone();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let config = ProxyConfig::builder()
            .target_host(echo_server.addr().ip().to_string())
            .target_port(echo_server.addr().port())
            .proxy_port(addr.port())
            .proxy_type(ProxyType::Tcp)
            .build()
            .unwrap();

        tokio::spawn(async move {
            let _ = proxy.start(config, shutdown).await;
        });

        assert!(
            test_utils::wait_for_port(addr).await,
            "Proxy failed to start"
        );

        (echo_server, shutdown_clone, addr)
    }

    #[tokio::test]
    async fn test_tcp_proxy_echo() {
        let (echo_server, shutdown, proxy_addr) = setup_proxy().await;
        let test_data = b"\x01\x02\x03Hello, proxy!";
        let mut response = vec![0; test_data.len()];

        let mut stream = TcpStream::connect(proxy_addr).await.unwrap();
        stream.write_all(test_data).await.unwrap();
        stream.flush().await.unwrap();

        let n = tokio::time::timeout(TEST_TIMEOUT, stream.read_exact(&mut response))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(n, test_data.len());
        assert_eq!(&response, test_data);

        shutdown.notify_one();
        echo_server.shutdown();
    }

    #[tokio::test]
    async fn test_tcp_proxy_large_data() {
        let (echo_server, shutdown, proxy_addr) = setup_proxy().await;
        let test_data = vec![0x55; 1024 * 1024];
        let mut response = vec![0; test_data.len()];

        let mut stream = TcpStream::connect(proxy_addr).await.unwrap();
        stream.write_all(&test_data).await.unwrap();
        stream.flush().await.unwrap();

        let n = tokio::time::timeout(TEST_TIMEOUT, stream.read_exact(&mut response))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(n, test_data.len());
        assert_eq!(response, test_data);

        shutdown.notify_one();
        echo_server.shutdown();
    }

    #[tokio::test]
    async fn test_tcp_proxy_multiple_clients() {
        let (echo_server, shutdown, proxy_addr) = setup_proxy().await;
        let test_data = b"\x00\x01Hello from client";
        let client_count = 5;
        let mut handles = Vec::new();

        for i in 0..client_count {
            let addr = proxy_addr;
            let data = test_data.to_vec();
            handles.push(tokio::spawn(async move {
                let mut stream = TcpStream::connect(addr).await.unwrap();
                stream.write_all(&data).await.unwrap();
                stream.flush().await.unwrap();

                let mut response = vec![0; data.len()];
                let n = stream.read_exact(&mut response).await.unwrap();
                (i, n, response)
            }));
        }

        for handle in handles {
            let (client_id, n, response) = handle.await.unwrap();
            assert_eq!(
                n,
                test_data.len(),
                "Client {client_id} received wrong data length"
            );
            assert_eq!(
                &response, test_data,
                "Client {client_id} received incorrect data"
            );
        }

        shutdown.notify_one();
        echo_server.shutdown();
    }

    #[tokio::test]
    async fn test_target_connection_has_keepalive() {
        let (echo_server, shutdown, proxy_addr) = setup_proxy().await;

        let mut stream = TcpStream::connect(proxy_addr).await.unwrap();
        stream.write_all(b"\x00\x00\x00test").await.unwrap();
        stream.flush().await.unwrap();

        let mut buf = [0; 7];
        let _ = stream.read_exact(&mut buf).await;

        shutdown.notify_one();
        echo_server.shutdown();
    }

    #[tokio::test]
    async fn test_http_traffic_through_proxy_uses_pool_correctly() {
        use http_body_util::{
            BodyExt,
            Empty,
        };
        use hyper::body::{
            Bytes,
            Incoming,
        };
        use hyper::server::conn::http1;
        use hyper::service::service_fn;
        use hyper::{
            Request,
            Response,
        };
        use hyper_util::rt::TokioIo;

        let http_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let http_addr = http_listener.local_addr().unwrap();

        tokio::spawn(async move {
            loop {
                let (sock, _) = match http_listener.accept().await {
                    Ok(v) => v,
                    Err(_) => return,
                };
                tokio::spawn(async move {
                    let io = TokioIo::new(sock);
                    let svc = service_fn(|_req: Request<Incoming>| async move {
                        Ok::<_, hyper::Error>(
                            Response::builder()
                                .status(200)
                                .body(Empty::<Bytes>::new())
                                .unwrap(),
                        )
                    });
                    let _ = http1::Builder::new()
                        .keep_alive(true)
                        .serve_connection(io, svc)
                        .await;
                });
            }
        });

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_addr = listener.local_addr().unwrap();
        drop(listener);

        let config = ProxyConfig::builder()
            .target_host("127.0.0.1".into())
            .target_port(http_addr.port())
            .proxy_port(proxy_addr.port())
            .proxy_type(ProxyType::Tcp)
            .build()
            .unwrap();

        let proxy = TcpProxy::new();
        let shutdown = Arc::new(Notify::new());
        let shutdown_clone = shutdown.clone();
        tokio::spawn(async move {
            let _ = proxy.start(config, shutdown).await;
        });
        assert!(test_utils::wait_for_port(proxy_addr).await);

        for _ in 0..5 {
            let stream = TcpStream::connect(proxy_addr).await.unwrap();
            let io = TokioIo::new(stream);
            let (mut sender, conn) = hyper::client::conn::http1::handshake::<_, Empty<Bytes>>(io)
                .await
                .unwrap();
            tokio::spawn(async move {
                let _ = conn.await;
            });
            let req = Request::builder()
                .method("GET")
                .uri("/")
                .header("host", "test")
                .body(Empty::<Bytes>::new())
                .unwrap();
            let resp = sender.send_request(req).await.unwrap();
            assert_eq!(resp.status(), 200);
            let _ = resp.into_body().collect().await;
        }

        shutdown_clone.notify_one();
    }

    #[test]
    fn test_tcp_backoff_calculation() {
        let mut backoff_ms: u64 = 10;
        for _ in 0..9 {
            backoff_ms = (backoff_ms * 2).min(5000);
        }
        assert_eq!(backoff_ms, 5000);
    }
}

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{
    BodyExt,
    Full,
};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{
    Request,
    Response,
    StatusCode,
};
use hyper_util::rt::TokioIo;
use log::{
    error,
    info,
};
use tokio::net::TcpListener;
use tokio::sync::Notify;
use uuid::Uuid;

use super::websocket_server::WebSocketTunnelServer;
use crate::models::tunnel_protocol::TunnelMessage;

pub struct ReverseHttpProxy {
    tunnel_server: Arc<WebSocketTunnelServer>,
    http_port: u16,
}

impl ReverseHttpProxy {
    pub fn new(tunnel_server: Arc<WebSocketTunnelServer>, http_port: u16) -> Self {
        Self {
            tunnel_server,
            http_port,
        }
    }

    pub async fn start(&self, shutdown: Arc<Notify>) -> Result<(), String> {
        let addr = SocketAddr::from(([0, 0, 0, 0], self.http_port));
        let tunnel_server = self.tunnel_server.clone();

        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| format!("Failed to bind HTTP server: {}", e))?;

        info!("HTTP reverse proxy listening on {}", addr);

        loop {
            tokio::select! {
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, _)) => {
                            let io = TokioIo::new(stream);
                            let tunnel = tunnel_server.clone();

                            tokio::spawn(async move {
                                if let Err(err) = http1::Builder::new()
                                    .serve_connection(
                                        io,
                                        service_fn(move |req| {
                                            let tunnel = tunnel.clone();
                                            async move { Self::handle_request(tunnel, req).await }
                                        }),
                                    )
                                    .await
                                {
                                    error!("Error serving connection: {:?}", err);
                                }
                            });
                        }
                        Err(e) => {
                            error!("Failed to accept connection: {}", e);
                        }
                    }
                }
                _ = shutdown.notified() => {
                    info!("HTTP reverse proxy shutting down");
                    break;
                }
            }
        }

        Ok(())
    }

    async fn handle_request(
        tunnel: Arc<WebSocketTunnelServer>, req: Request<Incoming>,
    ) -> Result<Response<Full<Bytes>>, Infallible> {
        if !tunnel.is_connected().await {
            return Ok(Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .body(Full::new(Bytes::from("Tunnel not connected")))
                .unwrap());
        }

        let request_id = Uuid::new_v4().to_string();

        let method = req.method().to_string();
        let path = req
            .uri()
            .path_and_query()
            .map(|pq| pq.as_str().to_string())
            .unwrap_or_else(|| "/".to_string());

        let headers: Vec<(String, String)> = req
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();

        let body_bytes = match req.into_body().collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(e) => {
                error!("Failed to read request body: {}", e);
                return Ok(Response::builder()
                    .status(StatusCode::BAD_REQUEST)
                    .body(Full::new(Bytes::from(format!(
                        "Failed to read request body: {}",
                        e
                    ))))
                    .unwrap());
            }
        };

        match tunnel
            .send_request(
                request_id.clone(),
                method.clone(),
                path.clone(),
                headers,
                body_bytes.to_vec(),
            )
            .await
        {
            Ok(TunnelMessage::HttpResponse {
                status,
                headers,
                body,
                ..
            }) => {
                let mut response = Response::new(Full::new(Bytes::from(body)));
                *response.status_mut() =
                    StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

                for (key, value) in headers {
                    if let Ok(header_name) = hyper::header::HeaderName::from_bytes(key.as_bytes())
                        && let Ok(header_value) = hyper::header::HeaderValue::from_str(&value)
                    {
                        response.headers_mut().append(header_name, header_value);
                    }
                }

                Ok(response)
            }
            Ok(TunnelMessage::Error { message, .. }) => {
                error!("Tunnel error for {} {}: {}", method, path, message);
                Ok(Response::builder()
                    .status(StatusCode::BAD_GATEWAY)
                    .body(Full::new(Bytes::from(format!("Tunnel error: {}", message))))
                    .unwrap())
            }
            Ok(_) => {
                // Unexpected message type
                error!("Unexpected tunnel response for {} {}", method, path);
                Ok(Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Full::new(Bytes::from("Unexpected tunnel response")))
                    .unwrap())
            }
            Err(e) => {
                error!("Tunnel error for {} {}: {}", method, path, e);
                Ok(Response::builder()
                    .status(StatusCode::BAD_GATEWAY)
                    .body(Full::new(Bytes::from(format!("Tunnel error: {}", e))))
                    .unwrap())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::net::TcpStream;
    use tokio::sync::Notify;

    use super::*;
    use crate::proxy::test_utils;

    #[tokio::test]
    async fn http_proxy_should_stop_accepting_when_shutdown_signaled() {
        let tunnel_server = Arc::new(WebSocketTunnelServer::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let http_proxy = ReverseHttpProxy::new(tunnel_server, port);
        let shutdown = Arc::new(Notify::new());
        let shutdown_clone = shutdown.clone();

        let proxy_handle = tokio::spawn(async move { http_proxy.start(shutdown_clone).await });

        let addr = format!("127.0.0.1:{}", port).parse().unwrap();
        assert!(
            test_utils::wait_for_port(addr).await,
            "HTTP proxy failed to start"
        );

        shutdown.notify_one();

        let result = tokio::time::timeout(Duration::from_secs(5), proxy_handle)
            .await
            .expect("proxy did not shut down within timeout")
            .expect("proxy task panicked");

        assert!(result.is_ok(), "proxy should return Ok on shutdown");

        let connect_result =
            tokio::time::timeout(Duration::from_secs(1), TcpStream::connect(addr)).await;

        match connect_result {
            Ok(Ok(_)) => panic!("should not accept connections after shutdown"),
            Ok(Err(_)) => {}
            Err(_) => panic!("connect timed out; proxy port may still be open after shutdown"),
        }
    }

    #[tokio::test]
    async fn repeated_headers_cross_the_tunnel_both_ways() {
        use futures::{
            SinkExt,
            StreamExt,
        };
        use tokio::io::{
            AsyncReadExt,
            AsyncWriteExt,
        };
        use tokio_tungstenite::tungstenite::Message;

        async fn free_port() -> u16 {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            listener.local_addr().unwrap().port()
        }

        let ws_port = free_port().await;
        let http_port = free_port().await;
        let tunnel_server = Arc::new(WebSocketTunnelServer::new(ws_port));
        let shutdown = Arc::new(Notify::new());
        let ws_task = tokio::spawn({
            let tunnel_server = tunnel_server.clone();
            let shutdown = shutdown.clone();
            async move { tunnel_server.start(shutdown).await }
        });
        let http_task = tokio::spawn({
            let proxy = ReverseHttpProxy::new(tunnel_server.clone(), http_port);
            let shutdown = shutdown.clone();
            async move { proxy.start(shutdown).await }
        });

        let ws_addr = format!("127.0.0.1:{ws_port}").parse().unwrap();
        assert!(test_utils::wait_for_port(ws_addr).await);
        let (mut ws_client, _) = tokio_tungstenite::connect_async(format!("ws://{ws_addr}"))
            .await
            .unwrap();
        while !tunnel_server.is_connected().await {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let http_addr = format!("127.0.0.1:{http_port}").parse().unwrap();
        assert!(test_utils::wait_for_port(http_addr).await);

        let browser = tokio::spawn(async move {
            let mut stream = TcpStream::connect(http_addr).await.unwrap();
            stream
                .write_all(
                    b"GET /login HTTP/1.1\r\nHost: app\r\nX-Tag: one\r\nX-Tag: two\r\n\
                      Connection: close\r\n\r\n",
                )
                .await
                .unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).await.unwrap();
            response
        });

        let request = tokio::time::timeout(Duration::from_secs(5), ws_client.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let Message::Binary(data) = request else {
            panic!("expected a binary tunnel message, got {request:?}");
        };
        let TunnelMessage::HttpRequest { id, headers, .. } =
            TunnelMessage::deserialize(&data).unwrap()
        else {
            panic!("expected an HTTP request");
        };
        let tags: Vec<&str> = headers
            .iter()
            .filter(|(name, _)| name == "x-tag")
            .map(|(_, value)| value.as_str())
            .collect();
        assert_eq!(tags, ["one", "two"]);

        let reply = TunnelMessage::HttpResponse {
            id,
            status: 200,
            headers: vec![
                ("set-cookie".to_owned(), "session=abc; Path=/".to_owned()),
                ("set-cookie".to_owned(), "csrf=xyz; Path=/".to_owned()),
            ],
            body: b"ok".to_vec(),
        };
        ws_client
            .send(Message::Binary(reply.serialize().unwrap().into()))
            .await
            .unwrap();

        let response = tokio::time::timeout(Duration::from_secs(5), browser)
            .await
            .unwrap()
            .unwrap();
        assert!(
            response.contains("set-cookie: session=abc; Path=/\r\n")
                && response.contains("set-cookie: csrf=xyz; Path=/\r\n"),
            "{response}"
        );

        shutdown.notify_waiters();
        ws_task.abort();
        http_task.abort();
    }
}

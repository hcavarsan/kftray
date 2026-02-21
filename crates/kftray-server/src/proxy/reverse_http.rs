use std::collections::HashMap;
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

        let headers: HashMap<String, String> = req
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
                        response.headers_mut().insert(header_name, header_value);
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
        // Arrange: create a tunnel server and HTTP proxy on a random port
        let tunnel_server = Arc::new(WebSocketTunnelServer::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let http_proxy = ReverseHttpProxy::new(tunnel_server, port);
        let shutdown = Arc::new(Notify::new());
        let shutdown_clone = shutdown.clone();

        let proxy_handle = tokio::spawn(async move {
            http_proxy.start(shutdown_clone).await
        });

        // Wait for proxy to be listening
        let addr = format!("127.0.0.1:{}", port).parse().unwrap();
        assert!(
            test_utils::wait_for_port(addr).await,
            "HTTP proxy failed to start"
        );

        // Act: signal shutdown
        shutdown.notify_one();

        // Wait for the proxy task to complete
        let result = tokio::time::timeout(Duration::from_secs(5), proxy_handle)
            .await
            .expect("proxy did not shut down within timeout")
            .expect("proxy task panicked");

        // Assert: proxy exited cleanly
        assert!(result.is_ok(), "proxy should return Ok on shutdown");

        // Assert: new connections are refused after shutdown
        let connect_result = tokio::time::timeout(
            Duration::from_secs(1),
            TcpStream::connect(addr),
        )
        .await;

        match connect_result {
            Ok(Ok(_)) => panic!("should not accept connections after shutdown"),
            Ok(Err(_)) => {} // connection refused — expected
            Err(_) => {}     // timeout — also acceptable
        }
    }
}

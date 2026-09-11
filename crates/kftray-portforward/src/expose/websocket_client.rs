use std::collections::HashMap;

use bytes::Bytes;
use futures::{
    SinkExt,
    StreamExt,
};
use http_body_util::{
    BodyExt,
    Full,
};
use hyper::{
    Method,
    Request,
    Uri,
};
use hyper_util::client::legacy::Client as LegacyClient;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use kftray_commons::models::tunnel_protocol::TunnelMessage;
use log::{
    debug,
    error,
    info,
    warn,
};
use tokio::sync::oneshot;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        Error as WsError,
        Message,
    },
};

pub struct WebSocketTunnelClient {
    websocket_port: u16,
    local_service_address: String,
    local_service_port: u16,
}

impl WebSocketTunnelClient {
    pub fn new(
        websocket_port: u16, local_service_address: String, local_service_port: u16,
    ) -> Self {
        Self {
            websocket_port,
            local_service_address,
            local_service_port,
        }
    }

    pub async fn start(&self, ready: oneshot::Sender<Result<(), String>>) -> Result<(), String> {
        let mut ready = Some(ready);
        let ws_url = format!("ws://127.0.0.1:{}", self.websocket_port);
        let max_retries = 100;
        let mut retry_count = 0;

        loop {
            info!(
                "Connecting WebSocket client to {} (attempt {}/{})",
                ws_url,
                retry_count + 1,
                max_retries
            );

            match self.connect_and_run(&ws_url, &mut ready).await {
                Ok(_) => {
                    info!("WebSocket tunnel disconnected gracefully");
                }
                Err(e) => {
                    error!("WebSocket tunnel error: {}", e);
                }
            }

            retry_count += 1;
            if retry_count >= max_retries {
                return Err(format!(
                    "Max reconnection attempts ({}) reached",
                    max_retries
                ));
            }

            let backoff_secs = std::cmp::min(2_u64.pow(retry_count.min(4)), 30);
            warn!(
                "WebSocket disconnected. Reconnecting in {} seconds...",
                backoff_secs
            );
            tokio::time::sleep(tokio::time::Duration::from_secs(backoff_secs)).await;
        }
    }

    async fn connect_and_run(
        &self, ws_url: &str, ready: &mut Option<oneshot::Sender<Result<(), String>>>,
    ) -> Result<(), String> {
        let (ws_stream, _) = match connect_async(ws_url).await {
            Ok(connection) => connection,
            Err(error) => {
                let message = format!("Failed to connect to WebSocket: {error}");
                let is_permanent = matches!(error, WsError::Http(_));
                if is_permanent
                    && let Some(ready) = ready.take()
                    && ready.send(Err(message.clone())).is_err()
                {
                    return Err("Expose startup was cancelled".to_owned());
                }
                return Err(message);
            }
        };
        if let Some(ready) = ready.take() {
            ready
                .send(Ok(()))
                .map_err(|_| "Expose startup was cancelled".to_owned())?;
        }

        info!("WebSocket tunnel connected");

        let (mut ws_write, mut ws_read) = ws_stream.split();

        let mut http_connector = HttpConnector::new();
        http_connector.set_nodelay(true);
        http_connector.set_keepalive(Some(std::time::Duration::from_secs(30)));
        let http_client = LegacyClient::builder(TokioExecutor::new()).build(http_connector);

        while let Some(msg) = ws_read.next().await {
            match msg {
                Ok(Message::Binary(data)) => match TunnelMessage::deserialize(&data) {
                    Ok(TunnelMessage::HttpRequest {
                        id,
                        method,
                        path,
                        headers,
                        body,
                    }) => {
                        debug!("Received HTTP request: {} {}", method, path);

                        let response_msg = self
                            .forward_to_local_service(
                                &http_client,
                                id.clone(),
                                method,
                                path,
                                headers,
                                body,
                            )
                            .await;

                        match response_msg.serialize() {
                            Ok(response_data) => {
                                if let Err(e) =
                                    ws_write.send(Message::Binary(response_data.into())).await
                                {
                                    error!("Failed to send response through WebSocket: {}", e);
                                    break;
                                }
                            }
                            Err(e) => {
                                error!("Failed to serialize response: {}", e);
                            }
                        }
                    }
                    Ok(TunnelMessage::Ping) => {
                        debug!("Received ping, sending pong");
                        let pong = TunnelMessage::Pong;
                        if let Ok(pong_data) = pong.serialize() {
                            let _ = ws_write.send(Message::Binary(pong_data.into())).await;
                        }
                    }
                    Ok(_) => {
                        warn!("Received unexpected message type from pod");
                    }
                    Err(e) => {
                        error!("Failed to deserialize tunnel message: {}", e);
                    }
                },
                Ok(Message::Ping(_)) => {
                    debug!("Received WebSocket ping");
                }
                Ok(Message::Close(_)) => {
                    info!("WebSocket closed by pod");
                    break;
                }
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    break;
                }
                _ => {}
            }
        }

        Ok(())
    }

    async fn forward_to_local_service(
        &self, http_client: &LegacyClient<HttpConnector, Full<Bytes>>, request_id: String,
        method: String, path: String, headers: HashMap<String, String>, body: Vec<u8>,
    ) -> TunnelMessage {
        let uri_str = format!(
            "http://{}:{}{}",
            self.local_service_address, self.local_service_port, path
        );
        debug!(
            "Forwarding {} {} to local service at {}",
            method, path, uri_str
        );
        let uri: Uri = match uri_str.parse() {
            Ok(u) => u,
            Err(e) => {
                error!("Invalid URI {}: {}", uri_str, e);
                return TunnelMessage::Error {
                    id: Some(request_id),
                    message: format!("Invalid URI: {}", e),
                };
            }
        };

        let http_method: Method = match method.as_str() {
            "GET" => Method::GET,
            "POST" => Method::POST,
            "PUT" => Method::PUT,
            "DELETE" => Method::DELETE,
            "PATCH" => Method::PATCH,
            "HEAD" => Method::HEAD,
            "OPTIONS" => Method::OPTIONS,
            _ => {
                error!("Unsupported HTTP method: {}", method);
                return TunnelMessage::Error {
                    id: Some(request_id),
                    message: format!("Unsupported method: {}", method),
                };
            }
        };

        let mut req_builder = Request::builder().method(http_method).uri(uri);

        for (key, value) in headers {
            if let Ok(header_name) = hyper::header::HeaderName::from_bytes(key.as_bytes())
                && let Ok(header_value) = hyper::header::HeaderValue::from_str(&value)
            {
                req_builder = req_builder.header(header_name, header_value);
            }
        }

        let request = match req_builder.body(Full::new(Bytes::from(body))) {
            Ok(req) => req,
            Err(e) => {
                error!("Failed to build HTTP request: {}", e);
                return TunnelMessage::Error {
                    id: Some(request_id),
                    message: format!("Failed to build request: {}", e),
                };
            }
        };

        match http_client.request(request).await {
            Ok(response) => {
                let status = response.status().as_u16();

                let response_headers: HashMap<String, String> = response
                    .headers()
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                    .collect();

                match response.into_body().collect().await {
                    Ok(collected) => {
                        let body_bytes = collected.to_bytes();
                        debug!("Forwarded request successfully, status: {}", status);
                        TunnelMessage::HttpResponse {
                            id: request_id,
                            status,
                            headers: response_headers,
                            body: body_bytes.to_vec(),
                        }
                    }
                    Err(e) => {
                        error!("Failed to read response body: {}", e);
                        TunnelMessage::Error {
                            id: Some(request_id),
                            message: format!("Failed to read response body: {}", e),
                        }
                    }
                }
            }
            Err(e) => {
                error!(
                    "Failed to forward request to local service at {}:{}: {:?}",
                    self.local_service_port, uri_str, e
                );
                TunnelMessage::Error {
                    id: Some(request_id),
                    message: format!(
                        "Failed to connect to local service at localhost:{}: {}",
                        self.local_service_port, e
                    ),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    use super::WebSocketTunnelClient;

    #[tokio::test]
    async fn startup_waits_for_the_websocket_handshake() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = WebSocketTunnelClient::new(
            listener.local_addr().unwrap().port(),
            "127.0.0.1".to_owned(),
            8080,
        );
        let (ready_tx, mut ready_rx) = oneshot::channel();
        let task = tokio::spawn(async move { client.start(ready_tx).await });
        let (socket, _) = listener.accept().await.unwrap();
        assert!(matches!(
            ready_rx.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        let _peer = tokio_tungstenite::accept_async(socket).await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), ready_rx)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn rejected_handshake_fails_startup() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = WebSocketTunnelClient::new(
            listener.local_addr().unwrap().port(),
            "127.0.0.1".to_owned(),
            8080,
        );
        let (ready_tx, ready_rx) = oneshot::channel();
        let task = tokio::spawn(async move { client.start(ready_tx).await });
        let (mut socket, _) = listener.accept().await.unwrap();
        socket
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap();
        let result = tokio::time::timeout(Duration::from_secs(1), ready_rx)
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_err());
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn transient_connection_failure_retries_before_reporting_readiness() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = WebSocketTunnelClient::new(
            listener.local_addr().unwrap().port(),
            "127.0.0.1".to_owned(),
            8080,
        );
        let (ready_tx, ready_rx) = oneshot::channel();
        let task = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            client.start(ready_tx).await
        }));
        let (first, _) = listener.accept().await.unwrap();
        drop(first);
        let peer = async {
            let (socket, _) = listener.accept().await.unwrap();
            tokio_tungstenite::accept_async(socket).await.unwrap()
        };
        let (ready, _peer) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(ready_rx, peer)
        })
        .await
        .unwrap();
        ready.unwrap().unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn forwards_the_request_without_a_disposable_tcp_probe() {
        use std::collections::HashMap;

        use http_body_util::Full;
        use hyper::body::Bytes;
        use hyper_util::client::legacy::Client;
        use hyper_util::rt::TokioExecutor;
        use tokio::io::AsyncReadExt;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = WebSocketTunnelClient::new(
            9999,
            "127.0.0.1".to_owned(),
            listener.local_addr().unwrap().port(),
        );
        let http_client = Client::builder(TokioExecutor::new()).build_http::<Full<Bytes>>();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(socket.read_u8().await.unwrap());
            }
            assert!(request.starts_with(b"GET /request HTTP/1.1\r\n"));
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nresponse",
                )
                .await
                .unwrap();
        });
        let response = tokio::time::timeout(
            Duration::from_secs(2),
            client.forward_to_local_service(
                &http_client,
                "request".to_owned(),
                "GET".to_owned(),
                "/request".to_owned(),
                HashMap::new(),
                Vec::new(),
            ),
        )
        .await
        .unwrap();
        match response {
            super::TunnelMessage::HttpResponse { status, body, .. } => {
                assert_eq!(status, 200);
                assert_eq!(body, b"response");
            }
            other => panic!("expected HTTP response, got {other:?}"),
        }
        server.await.unwrap();
    }
}

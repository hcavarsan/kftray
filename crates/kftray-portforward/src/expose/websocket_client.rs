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
use tokio_tungstenite::{
    connect_async,
    tungstenite::Message,
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

    pub async fn start(&self) -> Result<(), String> {
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

            match self.connect_and_run(&ws_url).await {
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

    async fn connect_and_run(&self, ws_url: &str) -> Result<(), String> {
        // Connect to the port-forwarded WebSocket endpoint
        let (ws_stream, _) = connect_async(ws_url)
            .await
            .map_err(|e| format!("Failed to connect to WebSocket: {}", e))?;

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

        let service_addr = format!("{}:{}", self.local_service_address, self.local_service_port);
        match tokio::net::TcpStream::connect(&service_addr).await {
            Ok(_stream) => {
                debug!("TCP connection to {} successful", service_addr);
            }
            Err(e) => {
                error!("Cannot establish TCP connection to {}: {}", service_addr, e);
                return TunnelMessage::Error {
                    id: Some(request_id),
                    message: format!("Cannot connect to local service at {}: {}", service_addr, e),
                };
            }
        }

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

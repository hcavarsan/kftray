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

    pub async fn start(&self) -> Result<(), String> {
        let addr = SocketAddr::from(([0, 0, 0, 0], self.http_port));
        let tunnel_server = self.tunnel_server.clone();

        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| format!("Failed to bind HTTP server: {}", e))?;

        info!("HTTP reverse proxy listening on {}", addr);

        loop {
            let (stream, _) = match listener.accept().await {
                Ok(v) => v,
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                    continue;
                }
            };

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

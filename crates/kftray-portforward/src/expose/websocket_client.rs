use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use futures::{
    SinkExt,
    StreamExt,
};
use http_body_util::{
    BodyExt,
    Full,
};
use hyper::upgrade::Upgraded;
use hyper::{
    Method,
    Request,
    StatusCode,
    Uri,
};
use hyper_util::client::legacy::Client as LegacyClient;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::{
    TokioExecutor,
    TokioIo,
};
use kftray_commons::models::tunnel_protocol::TunnelMessage;
use log::{
    debug,
    error,
    info,
    warn,
};
use tokio::io::{
    AsyncReadExt,
    AsyncWriteExt,
};
use tokio::sync::{
    mpsc,
    oneshot,
};
use tokio::task::JoinSet;
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
        self.reconnect_loop(ready, 100).await
    }

    /// The retry loop behind [`Self::start`], with the attempt budget as a
    /// parameter so a test can exhaust it without waiting on the real one.
    async fn reconnect_loop(
        &self, ready: oneshot::Sender<Result<(), String>>, max_retries: u32,
    ) -> Result<(), String> {
        let mut ready = Some(ready);
        let ws_url = format!("ws://127.0.0.1:{}", self.websocket_port);
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
                let message = format!("Max reconnection attempts ({}) reached", max_retries);
                // Every attempt so far was retryable, so `ready` is still
                // pending: without this, the caller only learns startup
                // ended through the channel closing, losing why.
                if let Some(sender) = ready.take() {
                    let _ = sender.send(Err(message.clone()));
                }
                return Err(message);
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
                // A rejection the relay may recover from keeps the readiness
                // sender pending, so the reconnect loop can retry within the
                // startup timeout instead of tearing the exposure down.
                let is_permanent = match &error {
                    WsError::Http(response) => !matches!(
                        response.status().as_u16(),
                        408 | 425 | 429 | 500 | 502 | 503 | 504
                    ),
                    _ => false,
                };
                if is_permanent
                    && let Some(ready) = ready.take()
                    && ready.send(Err(message.clone())).is_err()
                {
                    return Err(format!("Expose startup was cancelled: {message}"));
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

        let target = Arc::new(Self::new(
            self.websocket_port,
            self.local_service_address.clone(),
            self.local_service_port,
        ));
        let mut requests = JoinSet::new();
        let mut streams: HashMap<String, mpsc::UnboundedSender<Vec<u8>>> = HashMap::new();
        let (outgoing_tx, mut outgoing_rx) = mpsc::unbounded_channel::<TunnelMessage>();

        loop {
            let msg = tokio::select! {
                Some(joined) = requests.join_next(), if !requests.is_empty() => {
                    let (response_msg, upgraded) = match joined {
                        Ok(reply) => reply,
                        Err(e) => {
                            error!("Request task failed: {}", e);
                            continue;
                        }
                    };
                    let stream = match (&response_msg, upgraded) {
                        (TunnelMessage::HttpResponse { id, .. }, Some(upgraded)) => {
                            Some((id.clone(), upgraded))
                        }
                        _ => None,
                    };
                    let _ = outgoing_tx.send(response_msg);
                    if let Some((id, upgraded)) = stream {
                        let (to_local_tx, to_local_rx) = mpsc::unbounded_channel();
                        streams.insert(id.clone(), to_local_tx);
                        tokio::spawn(pipe_upgraded(id, upgraded, to_local_rx, outgoing_tx.clone()));
                    }
                    continue;
                }
                Some(outgoing) = outgoing_rx.recv() => {
                    if let TunnelMessage::StreamClose { id } = &outgoing {
                        streams.remove(id);
                    }
                    match outgoing.serialize() {
                        Ok(data) => {
                            if let Err(e) = ws_write.send(Message::Binary(data.into())).await {
                                error!("Failed to send response through WebSocket: {}", e);
                                break;
                            }
                        }
                        Err(e) => {
                            error!("Failed to serialize response: {}", e);
                        }
                    }
                    continue;
                }
                msg = ws_read.next() => match msg {
                    Some(msg) => msg,
                    None => break,
                },
            };

            match msg {
                Ok(Message::Binary(data)) => match TunnelMessage::deserialize(&data) {
                    Ok(TunnelMessage::HttpRequest {
                        id,
                        method,
                        path,
                        headers,
                        body,
                        upgrade,
                    }) => {
                        debug!("Received HTTP request: {} {}", method, path);

                        let target = Arc::clone(&target);
                        let http_client = http_client.clone();
                        requests.spawn(async move {
                            target
                                .forward_to_local_service(
                                    &http_client,
                                    id,
                                    method,
                                    path,
                                    headers,
                                    body,
                                    upgrade,
                                )
                                .await
                        });
                    }
                    Ok(TunnelMessage::StreamData { id, data }) => {
                        if let Some(to_local) = streams.get(&id)
                            && to_local.send(data).is_err()
                        {
                            streams.remove(&id);
                        }
                    }
                    Ok(TunnelMessage::StreamClose { id }) => {
                        streams.remove(&id);
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

    #[allow(clippy::too_many_arguments)]
    async fn forward_to_local_service(
        &self, http_client: &LegacyClient<HttpConnector, Full<Bytes>>, request_id: String,
        method: String, path: String, headers: Vec<(String, String)>, body: Vec<u8>, upgrade: bool,
    ) -> (TunnelMessage, Option<Upgraded>) {
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
                let message = format!("Invalid URI: {}", e);
                return (
                    TunnelMessage::Error {
                        id: Some(request_id),
                        message,
                    },
                    None,
                );
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
                let message = format!("Unsupported method: {}", method);
                return (
                    TunnelMessage::Error {
                        id: Some(request_id),
                        message,
                    },
                    None,
                );
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
                let message = format!("Failed to build request: {}", e);
                return (
                    TunnelMessage::Error {
                        id: Some(request_id),
                        message,
                    },
                    None,
                );
            }
        };

        match http_client.request(request).await {
            Ok(response) => {
                let status = response.status().as_u16();

                let response_headers: Vec<(String, String)> = response
                    .headers()
                    .iter()
                    .map(|(k, v)| {
                        (
                            k.to_string(),
                            String::from_utf8_lossy(v.as_bytes()).into_owned(),
                        )
                    })
                    .collect();

                if upgrade && response.status() == StatusCode::SWITCHING_PROTOCOLS {
                    return match hyper::upgrade::on(response).await {
                        Ok(upgraded) => (
                            TunnelMessage::HttpResponse {
                                id: request_id,
                                status,
                                headers: response_headers,
                                body: Vec::new(),
                                upgraded: true,
                            },
                            Some(upgraded),
                        ),
                        Err(e) => {
                            error!("Failed to upgrade the local connection: {}", e);
                            let message = format!("Failed to upgrade the local connection: {}", e);
                            (
                                TunnelMessage::Error {
                                    id: Some(request_id),
                                    message,
                                },
                                None,
                            )
                        }
                    };
                }

                let reply = match response.into_body().collect().await {
                    Ok(collected) => {
                        let body_bytes = collected.to_bytes();
                        debug!("Forwarded request successfully, status: {}", status);
                        TunnelMessage::HttpResponse {
                            id: request_id,
                            status,
                            headers: response_headers,
                            body: body_bytes.to_vec(),
                            upgraded: false,
                        }
                    }
                    Err(e) => {
                        error!("Failed to read response body: {}", e);
                        TunnelMessage::Error {
                            id: Some(request_id),
                            message: format!("Failed to read response body: {}", e),
                        }
                    }
                };
                (reply, None)
            }
            Err(e) => {
                error!(
                    "Failed to forward request to local service at {}:{}: {:?}",
                    self.local_service_port, uri_str, e
                );
                let message = format!(
                    "Failed to connect to local service at localhost:{}: {}",
                    self.local_service_port, e
                );
                (
                    TunnelMessage::Error {
                        id: Some(request_id),
                        message,
                    },
                    None,
                )
            }
        }
    }
}

async fn pipe_upgraded(
    id: String, upgraded: Upgraded, mut from_relay: mpsc::UnboundedReceiver<Vec<u8>>,
    outgoing: mpsc::UnboundedSender<TunnelMessage>,
) {
    let (mut reader, mut writer) = tokio::io::split(TokioIo::new(upgraded));
    let mut buf = vec![0u8; 16 * 1024];

    let local_closed = loop {
        tokio::select! {
            read = reader.read(&mut buf) => match read {
                Ok(0) | Err(_) => break true,
                Ok(n) => {
                    let data = TunnelMessage::StreamData {
                        id: id.clone(),
                        data: buf[..n].to_vec(),
                    };
                    if outgoing.send(data).is_err() {
                        break false;
                    }
                }
            },
            data = from_relay.recv() => match data {
                Some(data) => {
                    if writer.write_all(&data).await.is_err() {
                        break true;
                    }
                }
                None => break false,
            },
        }
    };

    let _ = writer.shutdown().await;
    if local_closed {
        let _ = outgoing.send(TunnelMessage::StreamClose { id });
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
    async fn exhausting_every_retry_reports_the_failure_through_ready() {
        // A port nothing listens on: every attempt is refused immediately,
        // which classifies as retryable, so the loop runs out its attempt
        // budget instead of ever reaching a permanent failure that sends
        // early.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let client = WebSocketTunnelClient::new(port, "127.0.0.1".to_owned(), 8080);
        let (ready_tx, ready_rx) = oneshot::channel();
        let result =
            tokio::time::timeout(Duration::from_secs(10), client.reconnect_loop(ready_tx, 2))
                .await
                .expect("a 2-attempt budget must not need more than a few seconds");
        let error = result.expect_err("every attempt was refused");
        assert!(error.contains("Max reconnection attempts"), "{error}");
        let ready_result = ready_rx
            .await
            .expect("ready must be resolved with the failure, not dropped silently");
        assert_eq!(ready_result, Err(error));
    }

    #[tokio::test]
    async fn forwards_the_request_without_a_disposable_tcp_probe() {
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
                Vec::new(),
                Vec::new(),
                false,
            ),
        )
        .await
        .unwrap()
        .0;
        match response {
            super::TunnelMessage::HttpResponse { status, body, .. } => {
                assert_eq!(status, 200);
                assert_eq!(body, b"response");
            }
            other => panic!("expected HTTP response, got {other:?}"),
        }
        server.await.unwrap();
    }

    #[tokio::test]
    async fn repeated_headers_keep_every_value() {
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
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nSet-Cookie: session=abc; Path=/\r\nSet-Cookie: csrf=xyz; \
                      Path=/\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                )
                .await
                .unwrap();
            String::from_utf8(request).unwrap()
        });
        let response = tokio::time::timeout(
            Duration::from_secs(2),
            client.forward_to_local_service(
                &http_client,
                "request".to_owned(),
                "GET".to_owned(),
                "/login".to_owned(),
                vec![
                    ("x-tag".to_owned(), "one".to_owned()),
                    ("x-tag".to_owned(), "two".to_owned()),
                ],
                Vec::new(),
                false,
            ),
        )
        .await
        .unwrap()
        .0;
        let request = server.await.unwrap();
        assert!(
            request.contains("x-tag: one\r\n") && request.contains("x-tag: two\r\n"),
            "{request}"
        );
        let wire = response.serialize().unwrap();
        match super::TunnelMessage::deserialize(&wire).unwrap() {
            super::TunnelMessage::HttpResponse { headers, .. } => {
                let cookies: Vec<&str> = headers
                    .iter()
                    .filter(|(name, _)| name == "set-cookie")
                    .map(|(_, value)| value.as_str())
                    .collect();
                assert_eq!(cookies, ["session=abc; Path=/", "csrf=xyz; Path=/"]);
            }
            other => panic!("expected HTTP response, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn non_ascii_response_header_crosses_the_tunnel() {
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
            socket
                .write_all(
                    "HTTP/1.1 200 OK\r\nContent-Disposition: attachment; filename=\"caf\u{e9}.txt\"\
                     \r\nContent-Length: 2\r\nConnection: close\r\n\r\nok"
                        .as_bytes(),
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
                "/file".to_owned(),
                Vec::new(),
                Vec::new(),
                false,
            ),
        )
        .await
        .unwrap()
        .0;
        server.await.unwrap();
        let wire = response.serialize().unwrap();
        match super::TunnelMessage::deserialize(&wire).unwrap() {
            super::TunnelMessage::HttpResponse { headers, .. } => {
                let disposition = headers
                    .iter()
                    .find(|(name, _)| name == "content-disposition")
                    .map(|(_, value)| value.as_str());
                assert_eq!(
                    disposition,
                    Some("attachment; filename=\"caf\u{e9}.txt\""),
                    "{headers:?}"
                );
            }
            other => panic!("expected HTTP response, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn open_response_does_not_hold_up_other_requests() {
        use futures::{
            SinkExt,
            StreamExt,
        };
        use tokio::io::AsyncReadExt;
        use tokio_tungstenite::tungstenite::Message;

        let app = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let app_port = app.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = app.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = socket.read(&mut buf).await.unwrap();
                    if buf[..n].starts_with(b"GET /events") {
                        socket
                            .write_all(
                                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\ndata: 1\n\n",
                            )
                            .await
                            .unwrap();
                        std::future::pending::<()>().await;
                    } else {
                        socket
                            .write_all(
                                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                            )
                            .await
                            .unwrap();
                    }
                });
            }
        });

        let request = |id: &str, path: &str| {
            super::TunnelMessage::HttpRequest {
                id: id.to_owned(),
                method: "GET".to_owned(),
                path: path.to_owned(),
                headers: Vec::new(),
                body: Vec::new(),
                upgrade: false,
            }
            .serialize()
            .unwrap()
        };

        let ws_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = WebSocketTunnelClient::new(
            ws_listener.local_addr().unwrap().port(),
            "127.0.0.1".to_owned(),
            app_port,
        );
        let (ready_tx, ready_rx) = oneshot::channel();
        let _task = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            client.start(ready_tx).await
        }));
        let (socket, _) = ws_listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
        ready_rx.await.unwrap().unwrap();

        ws.send(Message::Binary(request("sse", "/events").into()))
            .await
            .unwrap();
        ws.send(Message::Binary(request("fast", "/health").into()))
            .await
            .unwrap();
        ws.send(Message::Binary(
            super::TunnelMessage::Ping.serialize().unwrap().into(),
        ))
        .await
        .unwrap();

        let mut replies = Vec::new();
        while replies.len() < 2 {
            let reply = tokio::time::timeout(Duration::from_secs(3), ws.next())
                .await
                .expect("no reply to /health or ping within 3s while /events is open")
                .unwrap()
                .unwrap();
            replies.push(super::TunnelMessage::deserialize(&reply.into_data()).unwrap());
        }
        assert!(replies.iter().any(|reply| matches!(
            reply,
            super::TunnelMessage::HttpResponse { id, status: 200, body, .. }
                if id == "fast" && body == b"ok"
        )));
        assert!(
            replies
                .iter()
                .any(|reply| matches!(reply, super::TunnelMessage::Pong))
        );
    }

    #[tokio::test]
    async fn upgrade_request_streams_bytes_to_the_local_app_and_back() {
        use futures::{
            SinkExt,
            StreamExt,
        };
        use tokio::io::AsyncReadExt;
        use tokio_tungstenite::tungstenite::Message;

        use super::TunnelMessage;

        let app = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let app_port = app.local_addr().unwrap().port();
        let app_task = tokio::spawn(async move {
            let (mut socket, _) = app.accept().await.unwrap();
            let mut head = Vec::new();
            while !head.ends_with(b"\r\n\r\n") {
                head.push(socket.read_u8().await.unwrap());
            }
            socket
                .write_all(
                    b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\
                      Connection: Upgrade\r\n\r\n",
                )
                .await
                .unwrap();
            let mut ping = [0u8; 4];
            socket.read_exact(&mut ping).await.unwrap();
            socket.write_all(b"pong").await.unwrap();
            String::from_utf8(head).unwrap().to_ascii_lowercase()
        });

        let ws_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = WebSocketTunnelClient::new(
            ws_listener.local_addr().unwrap().port(),
            "127.0.0.1".to_owned(),
            app_port,
        );
        let (ready_tx, ready_rx) = oneshot::channel();
        let _task = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            client.start(ready_tx).await
        }));
        let (socket, _) = ws_listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
        ready_rx.await.unwrap().unwrap();

        let request = TunnelMessage::HttpRequest {
            id: "ws".to_owned(),
            method: "GET".to_owned(),
            path: "/socket".to_owned(),
            headers: vec![
                ("connection".to_owned(), "upgrade".to_owned()),
                ("upgrade".to_owned(), "websocket".to_owned()),
            ],
            body: Vec::new(),
            upgrade: true,
        };
        ws.send(Message::Binary(request.serialize().unwrap().into()))
            .await
            .unwrap();

        let mut replies = Vec::new();
        loop {
            let reply = tokio::time::timeout(Duration::from_secs(3), ws.next())
                .await
                .expect("no tunnel message within 3s")
                .unwrap()
                .unwrap();
            let reply = TunnelMessage::deserialize(&reply.into_data()).unwrap();
            if let TunnelMessage::HttpResponse {
                status: 101,
                upgraded: true,
                ..
            } = reply
            {
                let data = TunnelMessage::StreamData {
                    id: "ws".to_owned(),
                    data: b"ping".to_vec(),
                };
                ws.send(Message::Binary(data.serialize().unwrap().into()))
                    .await
                    .unwrap();
            }
            let closed = matches!(&reply, TunnelMessage::StreamClose { id } if id == "ws");
            replies.push(reply);
            if closed {
                break;
            }
        }

        let head = app_task.await.unwrap();
        assert!(head.contains("upgrade: websocket"), "{head}");
        let received: Vec<u8> = replies
            .iter()
            .filter_map(|reply| match reply {
                TunnelMessage::StreamData { id, data } if id == "ws" => Some(data.clone()),
                _ => None,
            })
            .flatten()
            .collect();
        assert_eq!(received, b"pong", "{replies:?}");
        assert!(
            matches!(
                replies.first(),
                Some(TunnelMessage::HttpResponse {
                    status: 101,
                    upgraded: true,
                    ..
                })
            ),
            "{replies:?}"
        );
    }
}

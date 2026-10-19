use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{
    BodyExt,
    Full,
};
use hyper::body::Incoming;
use hyper::header::{
    CONNECTION,
    UPGRADE,
};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::upgrade::OnUpgrade;
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
use tokio::io::{
    AsyncRead,
    AsyncReadExt,
    AsyncWrite,
    AsyncWriteExt,
};
use tokio::net::{
    TcpListener,
    TcpStream,
};
use tokio::sync::Notify;
use uuid::Uuid;

use super::websocket_server::{
    TunnelStream,
    WebSocketTunnelServer,
    take_credit,
};
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
                            let tunnel = tunnel_server.clone();
                            tokio::spawn(async move {
                                if tunnel.carries_tcp().await {
                                    Self::pipe_tcp(tunnel, stream).await;
                                } else {
                                    Self::serve_http(tunnel, stream).await;
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

    async fn serve_http(tunnel: Arc<WebSocketTunnelServer>, stream: TcpStream) {
        if let Err(err) = http1::Builder::new()
            .serve_connection(
                TokioIo::new(stream),
                service_fn(move |req| {
                    let tunnel = tunnel.clone();
                    async move { Self::handle_request(tunnel, req).await }
                }),
            )
            .with_upgrades()
            .await
        {
            error!("Error serving connection: {:?}", err);
        }
    }

    async fn pipe_tcp(tunnel: Arc<WebSocketTunnelServer>, stream: TcpStream) {
        match tunnel.open_stream(Uuid::new_v4().to_string()).await {
            Ok(tunnel_stream) => pipe_stream(stream, tunnel_stream).await,
            Err(e) => error!("Failed to open a TCP stream through the tunnel: {}", e),
        }
    }

    async fn handle_request(
        tunnel: Arc<WebSocketTunnelServer>, mut req: Request<Incoming>,
    ) -> Result<Response<Full<Bytes>>, Infallible> {
        if !tunnel.is_connected().await {
            return Ok(Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .body(Full::new(Bytes::from("Tunnel not connected")))
                .unwrap());
        }

        let request_id = Uuid::new_v4().to_string();
        let on_upgrade = is_upgrade_request(&req).then(|| hyper::upgrade::on(&mut req));

        let method = req.method().to_string();
        let path = req
            .uri()
            .path_and_query()
            .map(|pq| pq.as_str().to_string())
            .unwrap_or_else(|| "/".to_string());

        let headers: Vec<(String, String)> = req
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.to_string(),
                    String::from_utf8_lossy(v.as_bytes()).into_owned(),
                )
            })
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
                on_upgrade.is_some(),
            )
            .await
        {
            Ok((
                TunnelMessage::HttpResponse {
                    status,
                    headers,
                    body,
                    ..
                },
                stream,
            )) => {
                if status == StatusCode::SWITCHING_PROTOCOLS.as_u16() && stream.is_none() {
                    error!(
                        "Local app switched protocols for {} {}, but the kftray app cannot carry it",
                        method, path
                    );
                    return Ok(Response::builder()
                        .status(StatusCode::BAD_GATEWAY)
                        .body(Full::new(Bytes::from(
                            "Tunnel error: this kftray version cannot carry upgraded connections",
                        )))
                        .unwrap());
                }

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

                if let (Some(stream), Some(on_upgrade)) = (stream, on_upgrade) {
                    tokio::spawn(pipe_upgraded(on_upgrade, stream));
                }

                Ok(response)
            }
            Ok((TunnelMessage::Error { message, .. }, _)) => {
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

fn is_upgrade_request(req: &Request<Incoming>) -> bool {
    req.headers().contains_key(UPGRADE)
        && req.headers().get_all(CONNECTION).iter().any(|value| {
            value.to_str().is_ok_and(|value| {
                value
                    .split(',')
                    .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
            })
        })
}

async fn pipe_upgraded(on_upgrade: OnUpgrade, stream: TunnelStream) {
    match on_upgrade.await {
        Ok(upgraded) => pipe_stream(TokioIo::new(upgraded), stream).await,
        Err(e) => error!("Failed to upgrade the browser connection: {}", e),
    }
}

async fn pipe_stream<T: AsyncRead + AsyncWrite>(io: T, mut stream: TunnelStream) {
    let (mut reader, mut writer) = tokio::io::split(io);
    let mut buf = vec![0u8; 16 * 1024];
    let credit = stream.credit();
    let mut granted = 0;

    loop {
        tokio::select! {
            taken = take_credit(&credit, buf.len()), if granted == 0 => granted = taken,
            read = reader.read(&mut buf[..granted]), if granted > 0 => match read {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    granted -= n;
                    if !stream.send(buf[..n].to_vec()) {
                        break;
                    }
                }
            },
            data = stream.recv() => match data {
                Some(data) => {
                    if writer.write_all(&data).await.is_err()
                        || !stream.return_credit(data.len())
                    {
                        break;
                    }
                }
                None => break,
            },
        }
    }

    let _ = writer.shutdown().await;
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::net::TcpStream;
    use tokio::sync::Notify;

    use super::*;
    use crate::models::tunnel_protocol::STREAM_WINDOW;
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
            upgraded: false,
            flow_control: false,
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

    #[tokio::test]
    async fn non_ascii_header_crosses_the_tunnel() {
        use futures::StreamExt;
        use tokio::io::AsyncWriteExt;
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

        let mut stream = TcpStream::connect(http_addr).await.unwrap();
        stream
            .write_all(
                "GET /x HTTP/1.1\r\nHost: app\r\nX-Name: caf\u{e9}\r\nConnection: close\r\n\r\n"
                    .as_bytes(),
            )
            .await
            .unwrap();

        let request = tokio::time::timeout(Duration::from_secs(5), ws_client.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let Message::Binary(data) = request else {
            panic!("expected a binary tunnel message, got {request:?}");
        };
        let TunnelMessage::HttpRequest { headers, .. } = TunnelMessage::deserialize(&data).unwrap()
        else {
            panic!("expected an HTTP request");
        };
        let name = headers
            .iter()
            .find(|(k, _)| k == "x-name")
            .map(|(_, v)| v.as_str());
        assert_eq!(name, Some("caf\u{e9}"), "{headers:?}");

        shutdown.notify_waiters();
        ws_task.abort();
        http_task.abort();
    }

    type FakeClient = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;

    struct Relay {
        client: FakeClient,
        http_addr: std::net::SocketAddr,
        mode_header: Option<String>,
        shutdown: Arc<Notify>,
        tasks: Vec<tokio::task::JoinHandle<Result<(), String>>>,
    }

    impl Relay {
        async fn start() -> Self {
            Self::start_on("").await
        }

        async fn start_on(path: &str) -> Self {
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
            let (client, response) =
                tokio_tungstenite::connect_async(format!("ws://{ws_addr}{path}"))
                    .await
                    .unwrap();
            let mode_header = response
                .headers()
                .get(crate::models::tunnel_protocol::TUNNEL_MODE_HEADER)
                .map(|value| value.to_str().unwrap().to_owned());
            while !tunnel_server.is_connected().await {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let http_addr = format!("127.0.0.1:{http_port}").parse().unwrap();
            if mode_header.is_none() {
                assert!(test_utils::wait_for_port(http_addr).await);
            }

            Self {
                client,
                http_addr,
                mode_header,
                shutdown,
                tasks: vec![ws_task, http_task],
            }
        }

        /// In TCP mode every accepted connection opens a stream, so the
        /// client connection itself is the readiness probe.
        async fn connect_client(&self) -> TcpStream {
            for _ in 0..50 {
                if let Ok(stream) = TcpStream::connect(self.http_addr).await {
                    return stream;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            panic!("the relay never listened on {}", self.http_addr);
        }

        async fn recv(&mut self) -> TunnelMessage {
            use futures::StreamExt;
            use tokio_tungstenite::tungstenite::Message;

            loop {
                let message = tokio::time::timeout(Duration::from_secs(5), self.client.next())
                    .await
                    .expect("no tunnel message within 5s")
                    .unwrap()
                    .unwrap();
                if let Message::Binary(data) = message {
                    return TunnelMessage::deserialize(&data).unwrap();
                }
            }
        }

        async fn send(&mut self, message: TunnelMessage) {
            use futures::SinkExt;
            use tokio_tungstenite::tungstenite::Message;

            self.client
                .send(Message::Binary(message.serialize().unwrap().into()))
                .await
                .unwrap();
        }

        async fn stream_bytes(&mut self, id: &str, total: usize) -> usize {
            let mut received = 0;
            while received < total {
                match self.recv().await {
                    TunnelMessage::StreamData { id: got, data } if got == id => {
                        received += data.len();
                    }
                    other => panic!("expected stream data for {id}, got {other:?}"),
                }
            }
            received
        }

        async fn assert_quiet(&mut self) {
            use futures::StreamExt;

            if let Ok(message) =
                tokio::time::timeout(Duration::from_millis(300), self.client.next()).await
            {
                panic!("expected no tunnel message, got {message:?}");
            }
        }

        async fn open_flood(&mut self) -> (tokio::net::tcp::OwnedReadHalf, String) {
            use tokio::io::AsyncWriteExt;

            let (mut browser, id) = self.open_upgrade().await;
            self.send(switching_protocols(id.clone(), true, true)).await;
            read_head(&mut browser).await;
            let (read, mut write) = browser.into_split();
            self.tasks.push(tokio::spawn(async move {
                let _ = write.write_all(&vec![7u8; 4 * STREAM_WINDOW]).await;
                std::future::pending().await
            }));
            (read, id)
        }

        async fn open_upgrade(&mut self) -> (TcpStream, String) {
            use tokio::io::AsyncWriteExt;

            let mut browser = TcpStream::connect(self.http_addr).await.unwrap();
            browser
                .write_all(
                    b"GET /ws HTTP/1.1\r\nHost: app\r\nConnection: Upgrade\r\n\
                      Upgrade: websocket\r\n\r\n",
                )
                .await
                .unwrap();
            let TunnelMessage::HttpRequest {
                id,
                upgrade: true,
                flow_control: true,
                ..
            } = self.recv().await
            else {
                panic!("expected an upgrade request");
            };
            (browser, id)
        }

        fn stop(self) {
            self.shutdown.notify_waiters();
            for task in self.tasks {
                task.abort();
            }
        }
    }

    async fn read_head(stream: &mut TcpStream) -> String {
        use tokio::io::AsyncReadExt;

        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            head.push(stream.read_u8().await.unwrap());
        }
        String::from_utf8(head).unwrap()
    }

    async fn read_until_closed(stream: &mut tokio::net::tcp::OwnedReadHalf) -> Vec<u8> {
        use tokio::io::AsyncReadExt;

        let mut rest = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut rest))
            .await
            .expect("the browser connection stayed open")
            .unwrap();
        rest
    }

    fn switching_protocols(id: String, upgraded: bool, flow_control: bool) -> TunnelMessage {
        TunnelMessage::HttpResponse {
            id,
            status: 101,
            headers: vec![
                ("upgrade".to_owned(), "websocket".to_owned()),
                ("connection".to_owned(), "upgrade".to_owned()),
            ],
            body: Vec::new(),
            upgraded,
            flow_control,
        }
    }

    #[tokio::test]
    async fn tcp_tunnel_pipes_raw_bytes_between_a_client_and_the_app() {
        use tokio::io::{
            AsyncReadExt,
            AsyncWriteExt,
        };

        let mut relay = Relay::start_on(crate::models::tunnel_protocol::TCP_MODE_PATH).await;
        assert_eq!(relay.mode_header.as_deref(), Some("tcp"));

        let ssl_request = [0, 0, 0, 8, 4, 210, 22, 47];
        let mut client = relay.connect_client().await;
        client.write_all(&ssl_request).await.unwrap();
        let TunnelMessage::StreamOpen { id } = relay.recv().await else {
            panic!("expected a stream open");
        };
        match relay.recv().await {
            TunnelMessage::StreamData { id: got, data } => {
                assert_eq!(got, id);
                assert_eq!(data, ssl_request);
            }
            other => panic!("expected the client bytes, got {other:?}"),
        }

        relay
            .send(TunnelMessage::StreamData {
                id: id.clone(),
                data: b"N".to_vec(),
            })
            .await;
        let mut reply = [0u8; 1];
        tokio::time::timeout(Duration::from_secs(5), client.read_exact(&mut reply))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&reply, b"N");
        assert!(matches!(
            relay.recv().await,
            TunnelMessage::StreamCredit { id: got, bytes: 1 } if got == id
        ));

        relay.send(TunnelMessage::StreamClose { id }).await;
        let (mut read, _write) = client.into_split();
        assert!(read_until_closed(&mut read).await.is_empty());
        relay.stop();
    }

    #[tokio::test]
    async fn tcp_client_closing_closes_the_stream_in_the_app() {
        let mut relay = Relay::start_on(crate::models::tunnel_protocol::TCP_MODE_PATH).await;

        let client = relay.connect_client().await;
        let TunnelMessage::StreamOpen { id } = relay.recv().await else {
            panic!("expected a stream open");
        };
        drop(client);
        assert!(matches!(
            relay.recv().await,
            TunnelMessage::StreamClose { id: got } if got == id
        ));
        relay.stop();
    }

    #[tokio::test]
    async fn tunnel_without_tcp_mode_keeps_serving_http() {
        use tokio::io::AsyncWriteExt;

        let mut relay = Relay::start().await;
        assert_eq!(relay.mode_header, None);

        let mut client = TcpStream::connect(relay.http_addr).await.unwrap();
        client
            .write_all(b"GET / HTTP/1.1\r\nHost: app\r\n\r\n")
            .await
            .unwrap();
        assert!(matches!(
            relay.recv().await,
            TunnelMessage::HttpRequest { method, .. } if method == "GET"
        ));
        relay.stop();
    }

    #[tokio::test]
    async fn upgraded_connection_carries_bytes_both_ways_until_a_side_closes() {
        use tokio::io::{
            AsyncReadExt,
            AsyncWriteExt,
        };

        let mut relay = Relay::start().await;

        let (mut browser, id) = relay.open_upgrade().await;
        relay
            .send(switching_protocols(id.clone(), true, true))
            .await;
        let head = read_head(&mut browser).await;
        assert!(head.starts_with("HTTP/1.1 101"), "{head}");

        browser.write_all(b"hello").await.unwrap();
        match relay.recv().await {
            TunnelMessage::StreamData { id: got, data } => {
                assert_eq!(got, id);
                assert_eq!(data, b"hello");
            }
            other => panic!("expected stream data, got {other:?}"),
        }

        relay
            .send(TunnelMessage::StreamData {
                id: id.clone(),
                data: b"world".to_vec(),
            })
            .await;
        let mut reply = [0u8; 5];
        tokio::time::timeout(Duration::from_secs(5), browser.read_exact(&mut reply))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&reply, b"world");
        match relay.recv().await {
            TunnelMessage::StreamCredit { id: got, bytes } => {
                assert_eq!(got, id);
                assert_eq!(bytes, 5);
            }
            other => panic!("expected credit for the bytes the browser got, got {other:?}"),
        }

        relay.send(TunnelMessage::StreamClose { id }).await;
        let mut rest = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), browser.read_to_end(&mut rest))
            .await
            .expect("the browser connection stayed open after the client closed")
            .unwrap();
        assert!(rest.is_empty());

        let (mut browser, id) = relay.open_upgrade().await;
        relay
            .send(switching_protocols(id.clone(), true, true))
            .await;
        read_head(&mut browser).await;
        drop(browser);
        match relay.recv().await {
            TunnelMessage::StreamClose { id: got } => assert_eq!(got, id),
            other => panic!("expected stream close, got {other:?}"),
        }

        relay.stop();
    }

    #[tokio::test]
    async fn switching_protocols_from_a_client_without_credit_support_returns_bad_gateway() {
        let mut relay = Relay::start().await;

        for upgraded in [false, true] {
            let (mut browser, id) = relay.open_upgrade().await;
            relay.send(switching_protocols(id, upgraded, false)).await;
            let head = tokio::time::timeout(Duration::from_secs(5), read_head(&mut browser))
                .await
                .unwrap();
            assert!(head.starts_with("HTTP/1.1 502"), "{head}");
        }

        relay.stop();
    }

    #[tokio::test]
    async fn stalled_stream_stops_at_the_window_and_other_traffic_keeps_moving() {
        use tokio::io::{
            AsyncReadExt,
            AsyncWriteExt,
        };

        let mut relay = Relay::start().await;
        let (mut stalled, id) = relay.open_flood().await;
        assert_eq!(relay.stream_bytes(&id, STREAM_WINDOW).await, STREAM_WINDOW);
        relay.assert_quiet().await;

        let (mut other, other_id) = relay.open_upgrade().await;
        relay
            .send(switching_protocols(other_id.clone(), true, true))
            .await;
        read_head(&mut other).await;
        other.write_all(b"hi").await.unwrap();
        assert_eq!(relay.stream_bytes(&other_id, 2).await, 2);

        let mut plain = TcpStream::connect(relay.http_addr).await.unwrap();
        plain
            .write_all(b"GET /health HTTP/1.1\r\nHost: app\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let TunnelMessage::HttpRequest { id: plain_id, .. } = relay.recv().await else {
            panic!("expected the plain request");
        };
        relay
            .send(TunnelMessage::HttpResponse {
                id: plain_id,
                status: 200,
                headers: Vec::new(),
                body: b"ok".to_vec(),
                upgraded: false,
                flow_control: false,
            })
            .await;
        let mut response = String::new();
        tokio::time::timeout(Duration::from_secs(5), plain.read_to_string(&mut response))
            .await
            .unwrap()
            .unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");

        relay
            .send(TunnelMessage::StreamCredit {
                id: id.clone(),
                bytes: 1000,
            })
            .await;
        assert_eq!(relay.stream_bytes(&id, 1000).await, 1000);
        relay.assert_quiet().await;

        relay
            .send(TunnelMessage::StreamData {
                id: id.clone(),
                data: b"bye".to_vec(),
            })
            .await;
        relay.send(TunnelMessage::StreamClose { id }).await;
        assert_eq!(read_until_closed(&mut stalled).await, b"bye");

        relay.stop();
    }

    #[tokio::test]
    async fn tunnel_drop_closes_a_stream_waiting_for_credit() {
        let mut relay = Relay::start().await;
        let (mut stalled, id) = relay.open_flood().await;
        assert_eq!(relay.stream_bytes(&id, STREAM_WINDOW).await, STREAM_WINDOW);

        relay.client.close(None).await.unwrap();
        assert!(read_until_closed(&mut stalled).await.is_empty());

        relay.stop();
    }
}

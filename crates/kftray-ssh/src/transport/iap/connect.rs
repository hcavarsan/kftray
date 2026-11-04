//! Opens the WebSocket to IAP: plain TCP for `ws://` (tests), TLS with the
//! system roots for `wss://`, optionally through an HTTP CONNECT proxy.

use std::io;
use std::sync::{
    Arc,
    OnceLock,
};

use log::debug;
use rustls::pki_types::ServerName;
use rustls::{
    ClientConfig,
    RootCertStore,
};
use secrecy::ExposeSecret;
use tokio::io::{
    AsyncRead,
    AsyncReadExt,
    AsyncWrite,
    AsyncWriteExt,
};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_tungstenite::tungstenite::handshake::client::generate_key;
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::http::Uri;
use tokio_tungstenite::tungstenite::protocol::Role;
use tokio_tungstenite::{
    MaybeTlsStream,
    WebSocketStream,
};

use super::consts::{
    CONNECT,
    ORIGIN,
    PATH_ROOT,
    PROXY_HEADER_LIMIT,
    RECONNECT,
    SUBPROTOCOL,
    USER_AGENT,
};
use super::error::IapError;
use super::proxy;
use super::{
    IapTarget,
    TokenSource,
};
use crate::error::SshError;

pub(crate) type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Everything needed to open (and re-open) the WebSocket of one tunnel.
pub(crate) struct Dialer {
    target: IapTarget,
    endpoint: String,
    tokens: Arc<dyn TokenSource>,
    /// Built on first `wss://` use; native roots are read once per transport.
    tls: OnceLock<Arc<ClientConfig>>,
}

impl Dialer {
    pub(crate) fn new(target: IapTarget, endpoint: String, tokens: Arc<dyn TokenSource>) -> Self {
        Self {
            target,
            endpoint,
            tokens,
            tls: OnceLock::new(),
        }
    }

    /// Name of the tunnel's destination, for logs.
    pub(crate) fn name(&self) -> &str {
        self.target.name()
    }

    pub(crate) async fn connect(&self) -> Result<Ws, SshError> {
        self.dial(self.url(CONNECT, &self.target.connect_query()))
            .await
    }

    pub(crate) async fn reconnect(&self, sid: &str, ack: u64) -> Result<Ws, SshError> {
        self.dial(self.url(RECONNECT, &self.target.reconnect_query(sid, ack)))
            .await
    }

    fn url(&self, action: &str, query: &str) -> String {
        format!(
            "{}{PATH_ROOT}/{action}?{query}",
            self.endpoint.trim_end_matches('/')
        )
    }

    /// No timeout of its own: callers bound it together with the first frame.
    async fn dial(&self, url: String) -> Result<Ws, SshError> {
        // A fresh token for every (re)connect; the credentials crate caches and refreshes.
        let token = self.tokens.access_token().await?;
        let token = token.expose_secret();
        if !token.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(
                IapError::Protocol("the access token is not a valid header value".into()).into(),
            );
        }
        let uri: Uri = url
            .parse()
            .map_err(|_| IapError::Io("the IAP endpoint is not a valid URL".into()))?;
        let secure = match uri.scheme_str() {
            Some("wss") => true,
            Some("ws") => false,
            _ => {
                return Err(
                    IapError::Io("the IAP endpoint must be a ws:// or wss:// URL".into()).into(),
                );
            }
        };
        let host = uri
            .host()
            .ok_or_else(|| IapError::Io("the IAP endpoint has no host".into()))?;
        let port = uri.port_u16().unwrap_or(if secure { 443 } else { 80 });
        let authority = uri.authority().map_or(host, |authority| authority.as_str());
        let path = uri.path_and_query().map_or("/", |path| path.as_str());
        let key = generate_key();
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: Upgrade\r\nUpgrade: \
             websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: \
             {key}\r\nSec-WebSocket-Protocol: {SUBPROTOCOL}\r\nOrigin: {ORIGIN}\r\nUser-Agent: \
             {USER_AGENT}\r\nAuthorization: Bearer {token}\r\n\r\n"
        );

        let mut stream = if secure {
            let tcp = match proxy::from_env(host)? {
                Some(proxy) => {
                    debug!(
                        "[iap {}] reaching {host}:{port} through an HTTP proxy",
                        self.name()
                    );
                    proxy::connect(&proxy, host, port).await?
                }
                None => tcp_connect(host, port).await?,
            };
            let server_name = ServerName::try_from(host)
                .map_err(|_| IapError::Io(format!("{host} is not a valid server name")))?
                .to_owned();
            let tls = TlsConnector::from(self.tls_config().await?)
                .connect(server_name, tcp)
                .await
                .map_err(|e| IapError::Io(e.to_string()))?;
            MaybeTlsStream::Rustls(tls)
        } else {
            MaybeTlsStream::Plain(tcp_connect(host, port).await?)
        };
        upgrade(&mut stream, &request, &key).await?;
        Ok(WebSocketStream::from_raw_socket(stream, Role::Client, None).await)
    }

    async fn tls_config(&self) -> Result<Arc<ClientConfig>, IapError> {
        if let Some(config) = self.tls.get() {
            return Ok(config.clone());
        }
        let config = tokio::task::spawn_blocking(build_tls_config)
            .await
            .map_err(|e| IapError::Io(e.to_string()))??;
        // A concurrent first use may have won; either config is equivalent.
        Ok(self.tls.get_or_init(|| config).clone())
    }
}

async fn tcp_connect(host: &str, port: u16) -> Result<TcpStream, IapError> {
    let tcp = TcpStream::connect((host, port))
        .await
        .map_err(|e| IapError::Io(format!("cannot reach {host}:{port}: {e}")))?;
    // Frames are small and latency-sensitive (interactive SSH).
    tcp.set_nodelay(true)
        .map_err(|e| IapError::Io(e.to_string()))?;
    Ok(tcp)
}

async fn upgrade<S>(stream: &mut S, request: &str, key: &str) -> Result<(), IapError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let io = |e: io::Error| IapError::Io(e.to_string());
    stream.write_all(request.as_bytes()).await.map_err(io)?;
    stream.flush().await.map_err(io)?;
    let head = read_head(stream).await.map_err(io)?;
    let status = status_of(&head)
        .ok_or_else(|| IapError::Protocol("malformed answer to the WebSocket upgrade".into()))?;
    if status != 101 {
        return Err(IapError::from_status(status));
    }
    let head = String::from_utf8_lossy(&head);
    let header = |wanted: &str| {
        head.lines()
            .skip(1)
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.trim().eq_ignore_ascii_case(wanted))
            .map(|(_, value)| value.trim())
    };
    let accept = derive_accept_key(key.as_bytes());
    let upgraded = header("upgrade").is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
        && header("connection").is_some_and(|value| {
            value
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
        })
        && header("sec-websocket-accept") == Some(accept.as_str())
        && header("sec-websocket-protocol") == Some(SUBPROTOCOL);
    if !upgraded {
        return Err(IapError::Protocol(
            "IAP did not accept the WebSocket upgrade".into(),
        ));
    }
    Ok(())
}

pub(crate) async fn read_head<S>(stream: &mut S) -> io::Result<Vec<u8>>
where
    S: AsyncRead + Unpin,
{
    // One byte at a time: the peer sends nothing after the headers until we
    // do, but this way no TLS or WebSocket byte can ever be swallowed.
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() >= PROXY_HEADER_LIMIT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "oversized headers",
            ));
        }
        head.push(stream.read_u8().await?);
    }
    Ok(head)
}

pub(crate) fn status_of(head: &[u8]) -> Option<u16> {
    head.split(|b| *b == b'\r')
        .next()
        .and_then(|line| std::str::from_utf8(line).ok())
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|status| status.parse().ok())
}

fn build_tls_config() -> Result<Arc<ClientConfig>, IapError> {
    let loaded = rustls_native_certs::load_native_certs();
    if loaded.certs.is_empty() {
        return Err(IapError::Io(format!(
            "no native root certificates found ({} loading errors)",
            loaded.errors.len()
        )));
    }
    let mut roots = RootCertStore::empty();
    roots.add_parsable_certificates(loaded.certs);
    let config =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|e| IapError::Io(e.to_string()))?
            .with_root_certificates(roots)
            .with_no_client_auth();
    Ok(Arc::new(config))
}

/// RFC 3986 unreserved characters stay; everything else is escaped.
const QUERY_VALUE: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

pub(crate) fn percent_encode(value: &str) -> String {
    percent_encoding::utf8_percent_encode(value, QUERY_VALUE).to_string()
}

pub(crate) fn percent_decode(value: &str) -> String {
    percent_encoding::percent_decode_str(value)
        .decode_utf8_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "dGhlIHNhbXBsZSBub25jZQ==";
    const ACCEPT: &str = "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=";

    async fn upgrade_answered_with(answer: String) -> Result<(), IapError> {
        let (mut client, mut server) = tokio::io::duplex(4096);
        let mock = tokio::spawn(async move {
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(server.read_u8().await.unwrap());
            }
            server.write_all(answer.as_bytes()).await.unwrap();
        });
        let result = upgrade(&mut client, "GET / HTTP/1.1\r\n\r\n", KEY).await;
        mock.await.unwrap();
        result
    }

    fn switching(accept: &str, protocol: &str) -> String {
        format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: \
             Upgrade\r\nsec-websocket-accept: {accept}\r\nSec-WebSocket-Protocol: \
             {protocol}\r\n\r\n"
        )
    }

    #[tokio::test]
    async fn upgrade_should_succeed_when_accept_key_matches() {
        upgrade_answered_with(switching(ACCEPT, SUBPROTOCOL))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn upgrade_should_fail_when_accept_key_is_wrong() {
        assert!(matches!(
            upgrade_answered_with(switching("AAAA", SUBPROTOCOL)).await,
            Err(IapError::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn upgrade_should_fail_when_the_subprotocol_is_not_the_relay() {
        assert!(matches!(
            upgrade_answered_with(switching(ACCEPT, "chat")).await,
            Err(IapError::Protocol(_))
        ));
    }

    #[tokio::test]
    async fn upgrade_should_map_the_status_when_iap_refuses() {
        let refuse = |status: &str| format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\n\r\n");
        assert!(matches!(
            upgrade_answered_with(refuse("403 Forbidden")).await,
            Err(IapError::PermissionDenied)
        ));
        assert!(matches!(
            upgrade_answered_with(refuse("503 Service Unavailable")).await,
            Err(IapError::Http { status: 503 })
        ));
    }
}

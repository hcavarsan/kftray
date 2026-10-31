//! Opens the WebSocket to IAP: plain TCP for `ws://` (tests), TLS with the
//! system roots for `wss://`, optionally through an HTTP CONNECT proxy.

use std::sync::{
    Arc,
    OnceLock,
};

use log::debug;
use rustls::{
    ClientConfig,
    RootCertStore,
};
use secrecy::ExposeSecret;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::http::header::{
    AUTHORIZATION,
    ORIGIN,
    SEC_WEBSOCKET_PROTOCOL,
    USER_AGENT,
};
use tokio_tungstenite::{
    Connector,
    MaybeTlsStream,
    WebSocketStream,
    client_async_tls_with_config,
};

use super::consts::{
    self,
    CONNECT,
    PATH_ROOT,
    RECONNECT,
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
        let mut request = url.into_client_request().map_err(IapError::from_ws)?;
        let mut authorization = HeaderValue::from_str(&format!("Bearer {}", token.expose_secret()))
            .map_err(|_| {
                IapError::Protocol("the access token is not a valid header value".into())
            })?;
        authorization.set_sensitive(true);
        let headers = request.headers_mut();
        headers.insert(AUTHORIZATION, authorization);
        headers.insert(ORIGIN, HeaderValue::from_static(consts::ORIGIN));
        headers.insert(
            SEC_WEBSOCKET_PROTOCOL,
            HeaderValue::from_static(consts::SUBPROTOCOL),
        );
        headers.insert(USER_AGENT, HeaderValue::from_static(consts::USER_AGENT));

        let uri = request.uri().clone();
        let host = uri
            .host()
            .ok_or_else(|| IapError::Io("the IAP endpoint has no host".into()))?
            .to_owned();
        let secure = uri.scheme_str() == Some("wss");
        let port = uri.port_u16().unwrap_or(if secure { 443 } else { 80 });

        let (tcp, connector) = if secure {
            let tcp = match proxy::from_env(&host)? {
                Some(proxy) => {
                    debug!(
                        "[iap {}] reaching {host}:{port} through an HTTP proxy",
                        self.name()
                    );
                    proxy::connect(&proxy, &host, port).await?
                }
                None => tcp_connect(&host, port).await?,
            };
            (tcp, Connector::Rustls(self.tls_config().await?))
        } else {
            (tcp_connect(&host, port).await?, Connector::Plain)
        };
        let (ws, _) = client_async_tls_with_config(request, tcp, None, Some(connector))
            .await
            .map_err(IapError::from_ws)?;
        Ok(ws)
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

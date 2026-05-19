//! HTTP/1.1 and HTTP/2 reverse proxy with per-request pooled upstream client.
//!
//! Inbound connections are served by hyper; outbound requests check out a
//! pooled connection from `hyper_util::client::legacy::Client`, decoupling
//! inbound TCP lifetime from outbound TCP lifetime. This is the fix for the
//! multiplexed-channel-vs-HTTP-keep-alive regression: each forwarded request
//! gets a fresh checkout from the pool, so upstream-idle closes are invisible.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use http_body_util::{
    BodyExt,
    Full,
    combinators::BoxBody,
};
use hyper::body::{
    Bytes,
    Incoming,
};
use hyper::header::{
    HOST,
    HeaderValue,
    LOCATION,
};
use hyper::http::uri::Authority;
use hyper::server::conn::{
    http1 as server_http1,
    http2 as server_http2,
};
use hyper::service::service_fn;
use hyper::{
    HeaderMap,
    Request,
    Response,
    StatusCode,
    Uri,
};
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::{
    TokioExecutor,
    TokioIo,
    TokioTimer,
};
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

use crate::proxy::config::ProxyConfig;
use crate::proxy::error::ProxyError;

static HOP_HEADERS: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailers",
    "transfer-encoding",
    "upgrade",
];

type ResponseBody = BoxBody<Bytes, hyper::Error>;

/// Cheap-to-clone HTTP reverse proxy with a shared, pooled upstream client.
#[derive(Clone)]
pub struct HttpProxy {
    inner: Arc<Inner>,
}

struct Inner {
    client: Client<HttpConnector, Incoming>,
    target_authority: String,
}

impl HttpProxy {
    /// Build a new proxy for the given target. Initialises the pooled client.
    pub fn new(config: &ProxyConfig) -> Self {
        let mut connector = HttpConnector::new();
        connector.set_nodelay(true);
        connector.set_connect_timeout(Some(Duration::from_secs(5)));
        connector.set_keepalive(Some(Duration::from_secs(60)));
        connector.enforce_http(true);

        let client = Client::builder(TokioExecutor::new())
            .pool_idle_timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(16)
            .pool_timer(TokioTimer::new())
            .build(connector);

        let host = config
            .resolved_ip
            .clone()
            .unwrap_or_else(|| config.target_host.clone());
        let target_authority = format!("{}:{}", host, config.target_port);

        Self {
            inner: Arc::new(Inner {
                client,
                target_authority,
            }),
        }
    }

    /// Serve a single inbound HTTP/1.x connection until close or cancellation.
    pub async fn serve_http1(
        self, inbound: TcpStream, cancel: CancellationToken,
    ) -> Result<(), ProxyError> {
        let peer_addr = inbound
            .peer_addr()
            .map(|a| a.to_string())
            .unwrap_or_else(|_| "unknown".into());
        let span = tracing::info_span!("http_serve", version = "http1", peer = %peer_addr);
        let _guard = span.enter();

        let _ = inbound.set_nodelay(true);
        let io = TokioIo::new(inbound);
        let proxy = self.clone();
        let service = service_fn(move |req: Request<Incoming>| {
            let proxy = proxy.clone();
            async move { Ok::<_, Infallible>(proxy.forward(req).await) }
        });

        let conn = server_http1::Builder::new()
            .keep_alive(true)
            .timer(TokioTimer::new())
            .serve_connection(io, service)
            .with_upgrades();

        tokio::select! {
            res = conn => res.map_err(|e| ProxyError::Connection(format!("http1 serve: {e}"))),
            _ = cancel.cancelled() => Ok(()),
        }
    }

    /// Serve a single inbound HTTP/2 (cleartext h2c) connection.
    pub async fn serve_http2(
        self, inbound: TcpStream, cancel: CancellationToken,
    ) -> Result<(), ProxyError> {
        let peer_addr = inbound
            .peer_addr()
            .map(|a| a.to_string())
            .unwrap_or_else(|_| "unknown".into());
        let span = tracing::info_span!("http_serve", version = "http2", peer = %peer_addr);
        let _guard = span.enter();

        let _ = inbound.set_nodelay(true);
        let io = TokioIo::new(inbound);
        let proxy = self.clone();
        let service = service_fn(move |req: Request<Incoming>| {
            let proxy = proxy.clone();
            async move { Ok::<_, Infallible>(proxy.forward(req).await) }
        });

        let conn = server_http2::Builder::new(TokioExecutor::new())
            .timer(TokioTimer::new())
            .serve_connection(io, service);

        tokio::select! {
            res = conn => res.map_err(|e| ProxyError::Connection(format!("http2 serve: {e}"))),
            _ = cancel.cancelled() => Ok(()),
        }
    }

    async fn forward(&self, req: Request<Incoming>) -> Response<ResponseBody> {
        match self.try_forward(req).await {
            Ok(resp) => resp,
            Err(e) => {
                log::warn!("http proxy forward error: {e}");
                error_response(502, "Bad Gateway")
            }
        }
    }

    async fn try_forward(
        &self, mut req: Request<Incoming>,
    ) -> Result<Response<ResponseBody>, ProxyError> {
        tracing::debug!(method = %req.method(), uri = %req.uri(), "forward: start");

        let is_upgrade = req
            .headers()
            .get("connection")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_ascii_lowercase().contains("upgrade"))
            .unwrap_or(false);

        if is_upgrade {
            tracing::warn!(uri = %req.uri(), "forward: HTTP Upgrade not implemented; returning 501");
            return Ok(error_response(501, "Upgrade Not Implemented"));
        }

        // Capture the client-facing authority (what the client used to reach
        // us) before `rewrite_request` overwrites the inbound `Host` header
        // with the upstream's authority. Needed by the response-side rewriter
        // to repair absolute redirect URLs that leak the upstream address.
        let client_facing_authority = req
            .headers()
            .get(HOST)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);

        rewrite_request(&mut req, &self.inner.target_authority)?;

        tracing::debug!(target = %self.inner.target_authority, "forward: dispatching to upstream");

        let upstream = self.inner.client.request(req).await.map_err(|e| {
            tracing::warn!(error = %e, "forward: upstream error");
            ProxyError::Connection(format!("upstream: {e}"))
        })?;

        tracing::debug!(status = %upstream.status(), "forward: upstream responded");

        let (mut parts, body) = upstream.into_parts();
        for h in HOP_HEADERS {
            parts.headers.remove(*h);
        }
        rewrite_response_location(
            &mut parts.headers,
            parts.status,
            &self.inner.target_authority,
            client_facing_authority.as_deref(),
        );
        Ok(Response::from_parts(parts, body.boxed()))
    }
}

fn rewrite_request(req: &mut Request<Incoming>, authority: &str) -> Result<(), ProxyError> {
    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/")
        .to_string();
    let new_uri: Uri = format!("http://{authority}{path_and_query}")
        .parse()
        .map_err(|e| ProxyError::Connection(format!("uri build: {e}")))?;

    for h in HOP_HEADERS {
        req.headers_mut().remove(*h);
    }
    req.headers_mut().remove(HOST);
    req.headers_mut().insert(
        HOST,
        HeaderValue::from_str(authority)
            .map_err(|e| ProxyError::Connection(format!("host header: {e}")))?,
    );
    *req.uri_mut() = new_uri;
    Ok(())
}

/// Repair upstream-generated absolute redirect URLs that leak the upstream's
/// internal address.
///
/// Frameworks and admin UIs sometimes build `Location` headers from their own
/// configured hostname instead of honouring forwarding hints. The address
/// they emit is the one the proxy used to reach them, which a client outside
/// the cluster cannot resolve. When such a `Location` is detected on a 3xx
/// response, the upstream authority is swapped for the client-facing
/// authority the client used to reach the proxy. Scheme and path are
/// preserved.
///
/// Soft-fails on every error: malformed header, unparseable URI,
/// non-redirect status, authority mismatch, or rewritten value that does not
/// round-trip through `HeaderValue` all leave the header untouched. A proxy
/// that mangles edge cases is worse than one that occasionally forwards an
/// unmodified response.
fn rewrite_response_location(
    headers: &mut HeaderMap, status: StatusCode, upstream_authority: &str,
    client_facing_authority: Option<&str>,
) {
    if !status.is_redirection() {
        return;
    }

    let Some(location_hv) = headers.get(LOCATION) else {
        return;
    };
    let Ok(location_str) = location_hv.to_str() else {
        return;
    };
    let Ok(location_uri) = location_str.parse::<Uri>() else {
        return;
    };
    let Some(location_authority) = location_uri.authority() else {
        // Relative URI. Already resolved against the client's view of the
        // proxy, so passing it through is correct.
        return;
    };

    let Ok(upstream_auth) = upstream_authority.parse::<Authority>() else {
        return;
    };

    let host_matches = location_authority
        .host()
        .eq_ignore_ascii_case(upstream_auth.host());
    let port_matches = location_authority.port_u16() == upstream_auth.port_u16();
    if !(host_matches && port_matches) {
        // Cross-origin redirect (external SSO, CDN, etc.). Leave it alone.
        return;
    }

    let path_and_query = location_uri
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let scheme = location_uri.scheme_str().unwrap_or("http");

    let new_value = match client_facing_authority {
        Some(client_auth) => format!("{scheme}://{client_auth}{path_and_query}"),
        None => path_and_query.to_string(),
    };

    let Ok(new_hv) = HeaderValue::from_str(&new_value) else {
        return;
    };

    tracing::debug!(
        original = %location_str,
        rewritten = %new_value,
        "forward: rewrote Location header"
    );
    headers.insert(LOCATION, new_hv);
}

fn error_response(status: u16, msg: &'static str) -> Response<ResponseBody> {
    let body = Full::new(Bytes::from_static(msg.as_bytes()))
        .map_err(|never: Infallible| match never {})
        .boxed();
    Response::builder()
        .status(status)
        .header("content-type", "text/plain")
        .body(body)
        .expect("static response is well-formed")
}

#[cfg(test)]
mod tests {
    use hyper::HeaderMap;

    use super::*;

    fn strip(headers: &mut HeaderMap) {
        for h in HOP_HEADERS {
            headers.remove(*h);
        }
    }

    #[test]
    fn forward_strips_hop_by_hop_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("connection", HeaderValue::from_static("keep-alive"));
        headers.insert("transfer-encoding", HeaderValue::from_static("chunked"));
        headers.insert("upgrade", HeaderValue::from_static("h2c"));
        headers.insert("te", HeaderValue::from_static("trailers"));
        headers.insert("x-keep", HeaderValue::from_static("yes"));

        strip(&mut headers);

        assert!(!headers.contains_key("connection"));
        assert!(!headers.contains_key("transfer-encoding"));
        assert!(!headers.contains_key("upgrade"));
        assert!(!headers.contains_key("te"));
        assert_eq!(headers.get("x-keep").unwrap(), "yes");
    }

    #[test]
    fn forward_sets_host_to_target_authority() {
        let authority = "target.local:9000";
        let value = HeaderValue::from_str(authority).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(HOST, HeaderValue::from_static("old.example"));
        headers.remove(HOST);
        headers.insert(HOST, value);
        assert_eq!(headers.get(HOST).unwrap(), authority);
    }

    #[test]
    fn error_response_has_status() {
        let r = error_response(502, "Bad Gateway");
        assert_eq!(r.status(), 502);
    }

    fn headers_with_location(value: &'static str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(LOCATION, HeaderValue::from_static(value));
        h
    }

    #[test]
    fn rewrite_response_location_swaps_upstream_for_client_authority_on_301() {
        let mut headers = headers_with_location("http://10.0.0.5:8080/dashboard");
        rewrite_response_location(
            &mut headers,
            StatusCode::MOVED_PERMANENTLY,
            "10.0.0.5:8080",
            Some("proxy.local:9000"),
        );
        assert_eq!(
            headers.get(LOCATION).unwrap(),
            "http://proxy.local:9000/dashboard"
        );
    }

    #[test]
    fn rewrite_response_location_handles_302() {
        let mut headers = headers_with_location("http://upstream:7000/next");
        rewrite_response_location(
            &mut headers,
            StatusCode::FOUND,
            "upstream:7000",
            Some("client.local:80"),
        );
        assert_eq!(
            headers.get(LOCATION).unwrap(),
            "http://client.local:80/next"
        );
    }

    #[test]
    fn rewrite_response_location_handles_307_and_308() {
        for status in [
            StatusCode::TEMPORARY_REDIRECT,
            StatusCode::PERMANENT_REDIRECT,
        ] {
            let mut headers = headers_with_location("http://upstream:7000/x");
            rewrite_response_location(
                &mut headers,
                status,
                "upstream:7000",
                Some("client.local:80"),
            );
            assert_eq!(headers.get(LOCATION).unwrap(), "http://client.local:80/x");
        }
    }

    #[test]
    fn rewrite_response_location_preserves_query_and_fragment() {
        let mut headers = headers_with_location("http://upstream:7000/x?a=1&b=2");
        rewrite_response_location(
            &mut headers,
            StatusCode::FOUND,
            "upstream:7000",
            Some("client.local:80"),
        );
        assert_eq!(
            headers.get(LOCATION).unwrap(),
            "http://client.local:80/x?a=1&b=2"
        );
    }

    #[test]
    fn rewrite_response_location_ignores_cross_origin_redirect() {
        let mut headers = headers_with_location("https://sso.example.com/login");
        rewrite_response_location(
            &mut headers,
            StatusCode::FOUND,
            "upstream:7000",
            Some("client.local:80"),
        );
        assert_eq!(
            headers.get(LOCATION).unwrap(),
            "https://sso.example.com/login"
        );
    }

    #[test]
    fn rewrite_response_location_ignores_relative_uri() {
        let mut headers = headers_with_location("/already/relative?q=1");
        rewrite_response_location(
            &mut headers,
            StatusCode::FOUND,
            "upstream:7000",
            Some("client.local:80"),
        );
        assert_eq!(headers.get(LOCATION).unwrap(), "/already/relative?q=1");
    }

    #[test]
    fn rewrite_response_location_ignores_non_redirect_status() {
        let mut headers = headers_with_location("http://upstream:7000/new-resource");
        rewrite_response_location(
            &mut headers,
            StatusCode::CREATED,
            "upstream:7000",
            Some("client.local:80"),
        );
        assert_eq!(
            headers.get(LOCATION).unwrap(),
            "http://upstream:7000/new-resource"
        );
    }

    #[test]
    fn rewrite_response_location_ignores_200_with_location_header() {
        let mut headers = headers_with_location("http://upstream:7000/x");
        rewrite_response_location(
            &mut headers,
            StatusCode::OK,
            "upstream:7000",
            Some("client.local:80"),
        );
        assert_eq!(headers.get(LOCATION).unwrap(), "http://upstream:7000/x");
    }

    #[test]
    fn rewrite_response_location_no_header_is_noop() {
        let mut headers = HeaderMap::new();
        rewrite_response_location(
            &mut headers,
            StatusCode::FOUND,
            "upstream:7000",
            Some("client.local:80"),
        );
        assert!(!headers.contains_key(LOCATION));
    }

    #[test]
    fn rewrite_response_location_passes_through_malformed_uri() {
        // ASCII control byte would be rejected by HeaderValue, so use a
        // syntactically invalid URI instead (multiple colons, no scheme).
        let mut headers = HeaderMap::new();
        headers.insert(LOCATION, HeaderValue::from_static("http://:::/"));
        rewrite_response_location(
            &mut headers,
            StatusCode::FOUND,
            "upstream:7000",
            Some("client.local:80"),
        );
        assert_eq!(headers.get(LOCATION).unwrap(), "http://:::/");
    }

    #[test]
    fn rewrite_response_location_falls_back_to_path_only_without_client_authority() {
        let mut headers = headers_with_location("http://upstream:7000/dashboard?x=1");
        rewrite_response_location(
            &mut headers,
            StatusCode::FOUND,
            "upstream:7000",
            None,
        );
        assert_eq!(headers.get(LOCATION).unwrap(), "/dashboard?x=1");
    }

    #[test]
    fn rewrite_response_location_matches_host_case_insensitively() {
        let mut headers = headers_with_location("http://Upstream.Local:7000/x");
        rewrite_response_location(
            &mut headers,
            StatusCode::FOUND,
            "upstream.local:7000",
            Some("client.local:80"),
        );
        assert_eq!(headers.get(LOCATION).unwrap(), "http://client.local:80/x");
    }

    #[test]
    fn rewrite_response_location_leaves_mismatched_port_alone() {
        let mut headers = headers_with_location("http://upstream:9999/x");
        rewrite_response_location(
            &mut headers,
            StatusCode::FOUND,
            "upstream:7000",
            Some("client.local:80"),
        );
        assert_eq!(headers.get(LOCATION).unwrap(), "http://upstream:9999/x");
    }
}

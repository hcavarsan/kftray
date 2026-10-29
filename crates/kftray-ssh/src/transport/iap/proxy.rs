//! HTTP CONNECT proxy support (HTTPS_PROXY / NO_PROXY) for the wss:// endpoint.

use std::env;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use tokio::io::{
    AsyncRead,
    AsyncReadExt,
    AsyncWrite,
    AsyncWriteExt,
};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::http::Uri;

use super::connect::percent_decode;
use super::consts::PROXY_HEADER_LIMIT;
use super::error::IapError;

pub(crate) struct Proxy {
    host: String,
    port: u16,
    /// Full `Proxy-Authorization` value; a secret, so `Proxy` has no `Debug`.
    authorization: Option<String>,
}

impl Proxy {
    fn name(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// The proxy to use for `target_host`, from `HTTPS_PROXY`/`NO_PROXY` (either case).
pub(crate) fn from_env(target_host: &str) -> Result<Option<Proxy>, IapError> {
    let var = |names: [&str; 2]| {
        names
            .iter()
            .find_map(|name| env::var(name).ok().filter(|value| !value.is_empty()))
    };
    resolve(
        target_host,
        var(["HTTPS_PROXY", "https_proxy"]).as_deref(),
        var(["NO_PROXY", "no_proxy"]).as_deref(),
    )
}

fn resolve(
    target_host: &str, proxy: Option<&str>, no_proxy: Option<&str>,
) -> Result<Option<Proxy>, IapError> {
    let Some(raw) = proxy else {
        return Ok(None);
    };
    if no_proxy.is_some_and(|list| bypassed(target_host, list)) {
        return Ok(None);
    }
    parse(raw).map(Some)
}

/// `example.com` and `.example.com` match the host itself and its subdomains;
/// ports in entries are ignored and `*` matches everything.
fn bypassed(host: &str, no_proxy: &str) -> bool {
    let host = host.to_ascii_lowercase();
    no_proxy
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .any(|entry| {
            if entry == "*" {
                return true;
            }
            let entry = entry.to_ascii_lowercase();
            let name = entry
                .rsplit_once(':')
                .map_or(entry.as_str(), |(name, _)| name);
            let name = name.trim_start_matches(['*', '.']);
            host == name
                || host
                    .strip_suffix(name)
                    .is_some_and(|prefix| prefix.ends_with('.'))
        })
}

fn parse(raw: &str) -> Result<Proxy, IapError> {
    // The URL may carry a password, so it never appears in an error.
    let invalid = || IapError::Io("invalid proxy URL in HTTPS_PROXY".into());
    let uri: Uri = raw.parse().map_err(|_| invalid())?;
    match uri.scheme_str() {
        None | Some("http") => {}
        Some(scheme) => {
            return Err(IapError::Io(format!(
                "unsupported proxy scheme {scheme}; only http:// proxies are supported"
            )));
        }
    }
    let authority = uri.authority().ok_or_else(invalid)?;
    let authorization = authority
        .as_str()
        .rsplit_once('@')
        .map(|(userinfo, _)| format!("Basic {}", STANDARD.encode(percent_decode(userinfo))));
    Ok(Proxy {
        host: authority
            .host()
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_owned(),
        port: authority.port_u16().unwrap_or(80),
        authorization,
    })
}

/// TCP stream to `host:port` tunnelled through `proxy`.
pub(crate) async fn connect(proxy: &Proxy, host: &str, port: u16) -> Result<TcpStream, IapError> {
    let mut stream = TcpStream::connect((proxy.host.as_str(), proxy.port))
        .await
        .map_err(|e| IapError::Io(format!("cannot reach the proxy {}: {e}", proxy.name())))?;
    handshake(&mut stream, proxy, host, port).await?;
    Ok(stream)
}

async fn handshake<S>(stream: &mut S, proxy: &Proxy, host: &str, port: u16) -> Result<(), IapError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let io = |e: std::io::Error| IapError::Io(format!("proxy {}: {e}", proxy.name()));
    let mut request = format!("CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n");
    if let Some(authorization) = &proxy.authorization {
        request.push_str("Proxy-Authorization: ");
        request.push_str(authorization);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).await.map_err(io)?;

    // One byte at a time: the proxy sends nothing after the headers until we
    // do, but this way no TLS byte can ever be swallowed.
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() >= PROXY_HEADER_LIMIT {
            return Err(IapError::Io(format!(
                "proxy {} sent oversized headers",
                proxy.name()
            )));
        }
        head.push(stream.read_u8().await.map_err(io)?);
    }
    let status = head
        .split(|b| *b == b'\r')
        .next()
        .and_then(|line| std::str::from_utf8(line).ok())
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or_else(|| IapError::Io(format!("malformed answer from proxy {}", proxy.name())))?;
    if status != 200 {
        return Err(IapError::ProxyConnect {
            proxy: proxy.name(),
            status,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bypassed_should_match_hosts_when_no_proxy_lists_them() {
        assert!(bypassed("tunnel.cloudproxy.app", "*"));
        assert!(bypassed("tunnel.cloudproxy.app", "cloudproxy.app"));
        assert!(bypassed("tunnel.cloudproxy.app", ".cloudproxy.app"));
        assert!(bypassed("cloudproxy.app", ".cloudproxy.app"));
        assert!(bypassed(
            "TUNNEL.cloudproxy.app",
            "foo.com, CLOUDPROXY.app:443"
        ));
        assert!(!bypassed("tunnel.cloudproxy.app", "proxy.app"));
        assert!(!bypassed("tunnel.cloudproxy.app", "other.com,,"));
    }

    #[test]
    fn resolve_should_skip_proxy_when_unset_or_bypassed() {
        assert!(resolve("a.com", None, None).unwrap().is_none());
        assert!(
            resolve("a.com", Some("http://p:3128"), Some("a.com"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn resolve_should_parse_credentials_and_default_port_when_url_has_userinfo() {
        let proxy = resolve("a.com", Some("http://u:p%40ss@proxy.corp"), None)
            .unwrap()
            .unwrap();
        assert_eq!(proxy.host, "proxy.corp");
        assert_eq!(proxy.port, 80);
        assert_eq!(proxy.authorization.as_deref(), Some("Basic dTpwQHNz"));
        let bare = resolve("a.com", Some("proxy.corp:3128"), None)
            .unwrap()
            .unwrap();
        assert_eq!((bare.host.as_str(), bare.port), ("proxy.corp", 3128));
        assert!(bare.authorization.is_none());
    }

    #[test]
    fn resolve_should_reject_scheme_when_proxy_is_not_http() {
        let error = resolve("a.com", Some("socks5://p:1080"), None)
            .err()
            .unwrap();
        assert_eq!(
            error.to_string(),
            "IAP tunnel error: unsupported proxy scheme socks5; only http:// proxies are supported"
        );
    }

    async fn run_handshake(answer: &'static str) -> (Result<(), IapError>, String) {
        let proxy = parse("http://u:p@proxy.corp:3128").unwrap();
        let (mut client, mut server) = tokio::io::duplex(4096);
        let mock = tokio::spawn(async move {
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(server.read_u8().await.unwrap());
            }
            server.write_all(answer.as_bytes()).await.unwrap();
            String::from_utf8(request).unwrap()
        });
        let result = handshake(&mut client, &proxy, "tunnel.cloudproxy.app", 443).await;
        (result, mock.await.unwrap())
    }

    #[tokio::test]
    async fn handshake_should_send_connect_with_credentials_when_proxy_accepts() {
        let (result, request) = run_handshake("HTTP/1.1 200 Connection established\r\n\r\n").await;
        result.unwrap();
        assert_eq!(
            request,
            "CONNECT tunnel.cloudproxy.app:443 HTTP/1.1\r\nHost: tunnel.cloudproxy.app:443\r\nProxy-Authorization: Basic dTpw\r\n\r\n"
        );
    }

    #[tokio::test]
    async fn handshake_should_fail_with_status_when_proxy_refuses() {
        let (result, _) = run_handshake("HTTP/1.1 407 Proxy Authentication Required\r\n\r\n").await;
        assert!(matches!(
            result,
            Err(IapError::ProxyConnect { status: 407, .. })
        ));
    }
}

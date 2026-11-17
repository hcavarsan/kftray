use hyper::Uri;
use hyper_openssl::client::legacy::HttpsConnector;
use hyper_util::client::legacy::connect::HttpConnector;
use kube::Client;
use kube::client::ConfigExt;
use kube::config::Config;

use super::connection::{
    build_kube_client,
    create_http_connector,
    create_hyper_client,
};
use super::error::{
    KubeClientError,
    KubeResult,
};

pub async fn create_rustls_with_proxy(config: Config, proxy_url: &Uri) -> KubeResult<Client> {
    match proxy_url.scheme_str() {
        Some("http") | Some("https") => create_rustls_http_proxy(config, proxy_url).await,
        Some("socks5") | Some("socks5h") => create_rustls_socks5_proxy(config, proxy_url).await,
        _ => Err(KubeClientError::proxy_error(
            format!("Unsupported proxy scheme: {:?}", proxy_url.scheme_str()),
            proxy_url.to_string(),
        )),
    }
}

pub async fn create_openssl_with_proxy(
    config: Config, ssl_builder: openssl::ssl::SslConnectorBuilder, http_connector: HttpConnector,
    proxy_url: &Uri,
) -> KubeResult<Client> {
    match proxy_url.scheme_str() {
        Some("http") | Some("https") => {
            create_openssl_http_proxy(config, ssl_builder, http_connector, proxy_url).await
        }
        Some("socks5") | Some("socks5h") => {
            create_openssl_socks5_proxy(config, ssl_builder, http_connector, proxy_url).await
        }
        _ => Err(KubeClientError::proxy_error(
            format!("Unsupported proxy scheme: {:?}", proxy_url.scheme_str()),
            proxy_url.to_string(),
        )),
    }
}

pub async fn create_insecure_with_proxy(
    config: Config, http_connector: HttpConnector, proxy_url: &Uri,
) -> KubeResult<Client> {
    match proxy_url.scheme_str() {
        Some("http") | Some("https") => {
            create_insecure_http_proxy(config, http_connector, proxy_url).await
        }
        Some("socks5") | Some("socks5h") => {
            create_insecure_socks5_proxy(config, http_connector, proxy_url).await
        }
        _ => Err(KubeClientError::proxy_error(
            format!("Unsupported proxy scheme: {:?}", proxy_url.scheme_str()),
            proxy_url.to_string(),
        )),
    }
}

async fn create_rustls_http_proxy(config: Config, proxy_url: &Uri) -> KubeResult<Client> {
    let tunnel = create_http_tunnel(proxy_url);
    let connector = config
        .rustls_https_connector_with_connector(tunnel)
        .map_err(|e| {
            KubeClientError::connection_error_with_source(
                "Failed to create Rustls connector with HTTP proxy",
                e,
            )
        })?;

    let hyper_client = create_hyper_client(connector);
    build_kube_client(config, hyper_client)
}

async fn create_rustls_socks5_proxy(config: Config, proxy_url: &Uri) -> KubeResult<Client> {
    let socks = create_socks5_proxy(proxy_url);
    let connector = config
        .rustls_https_connector_with_connector(socks)
        .map_err(|e| {
            KubeClientError::connection_error_with_source(
                "Failed to create Rustls connector with SOCKS5 proxy",
                e,
            )
        })?;

    let hyper_client = create_hyper_client(connector);
    build_kube_client(config, hyper_client)
}

async fn create_openssl_http_proxy(
    config: Config, ssl_builder: openssl::ssl::SslConnectorBuilder, http_connector: HttpConnector,
    proxy_url: &Uri,
) -> KubeResult<Client> {
    let tunnel = create_http_tunnel_with_connector(proxy_url, http_connector);
    let connector = HttpsConnector::with_connector(tunnel, ssl_builder).map_err(|e| {
        KubeClientError::connection_error_with_source(
            "Failed to create OpenSSL connector with HTTP proxy",
            e,
        )
    })?;

    let hyper_client = create_hyper_client(connector);
    build_kube_client(config, hyper_client)
}

async fn create_openssl_socks5_proxy(
    config: Config, ssl_builder: openssl::ssl::SslConnectorBuilder, http_connector: HttpConnector,
    proxy_url: &Uri,
) -> KubeResult<Client> {
    let socks = create_socks5_proxy_with_connector(proxy_url, http_connector);
    let connector = HttpsConnector::with_connector(socks, ssl_builder).map_err(|e| {
        KubeClientError::connection_error_with_source(
            "Failed to create OpenSSL connector with SOCKS5 proxy",
            e,
        )
    })?;

    let hyper_client = create_hyper_client(connector);
    build_kube_client(config, hyper_client)
}

async fn create_insecure_http_proxy(
    config: Config, http_connector: HttpConnector, proxy_url: &Uri,
) -> KubeResult<Client> {
    let tunnel = create_http_tunnel_with_connector(proxy_url, http_connector);
    let hyper_client = create_hyper_client(tunnel);
    build_kube_client(config, hyper_client)
}

async fn create_insecure_socks5_proxy(
    config: Config, http_connector: HttpConnector, proxy_url: &Uri,
) -> KubeResult<Client> {
    let socks = create_socks5_proxy_with_connector(proxy_url, http_connector);
    let hyper_client = create_hyper_client(socks);
    build_kube_client(config, hyper_client)
}

fn create_http_tunnel(
    proxy_url: &Uri,
) -> hyper_util::client::legacy::connect::proxy::Tunnel<HttpConnector> {
    create_http_tunnel_with_connector(proxy_url, create_http_connector())
}

fn create_http_tunnel_with_connector(
    proxy_url: &Uri, connector: HttpConnector,
) -> hyper_util::client::legacy::connect::proxy::Tunnel<HttpConnector> {
    use base64::Engine;
    use hyper_util::client::legacy::connect::proxy::Tunnel;
    let tunnel = Tunnel::new(proxy_url.clone(), connector);
    let Some((user, pass)) = proxy_credentials(proxy_url) else {
        return tunnel;
    };
    let token = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"));
    match http::HeaderValue::from_str(&format!("Basic {token}")) {
        Ok(auth) => tunnel.with_auth(auth),
        Err(_) => tunnel,
    }
}

fn create_socks5_proxy(
    proxy_url: &Uri,
) -> hyper_util::client::legacy::connect::proxy::SocksV5<HttpConnector> {
    create_socks5_proxy_with_connector(proxy_url, create_http_connector())
}

fn create_socks5_proxy_with_connector(
    proxy_url: &Uri, connector: HttpConnector,
) -> hyper_util::client::legacy::connect::proxy::SocksV5<HttpConnector> {
    use hyper_util::client::legacy::connect::proxy::SocksV5;
    let socks = SocksV5::new(proxy_url.clone(), connector);
    match proxy_credentials(proxy_url) {
        Some((user, pass)) => socks.with_auth(user, pass),
        None => socks,
    }
}

fn proxy_credentials(proxy_url: &Uri) -> Option<(String, String)> {
    let (userinfo, _) = proxy_url.authority()?.as_str().rsplit_once('@')?;
    let (user, pass) = userinfo.split_once(':').unwrap_or((userinfo, ""));
    Some((percent_decode(user), percent_decode(pass)))
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .filter(|pair| pair.iter().all(u8::is_ascii_hexdigit))
            .and_then(|pair| std::str::from_utf8(pair).ok());
        match (
            bytes[i],
            hex.and_then(|pair| u8::from_str_radix(pair, 16).ok()),
        ) {
            (b'%', Some(byte)) => {
                decoded.push(byte);
                i += 3;
            }
            (byte, _) => {
                decoded.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug)]
    enum Entry {
        Rustls,
        Openssl,
        Insecure,
    }

    const ENTRIES: [Entry; 3] = [Entry::Rustls, Entry::Openssl, Entry::Insecure];

    async fn connect_through(entry: Entry, proxy: &Uri) -> KubeResult<Client> {
        crate::ssl::ensure_crypto_provider_installed();
        let mut config = Config::new("https://kube.invalid:6443".parse().unwrap());
        config.proxy_url = Some(proxy.clone());
        match entry {
            Entry::Rustls => create_rustls_with_proxy(config, proxy).await,
            Entry::Openssl => {
                let ssl_builder =
                    openssl::ssl::SslConnector::builder(openssl::ssl::SslMethod::tls()).unwrap();
                create_openssl_with_proxy(config, ssl_builder, create_http_connector(), proxy).await
            }
            Entry::Insecure => {
                create_insecure_with_proxy(config, create_http_connector(), proxy).await
            }
        }
    }

    async fn first_bytes_seen_by_proxy(entry: Entry, scheme: &str) -> Vec<u8> {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let accept = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 1024];
            let n = stream.read(&mut buf).await.unwrap();
            buf.truncate(n);
            buf
        });
        let proxy: Uri = format!("{scheme}://{addr}").parse().unwrap();
        let client = connect_through(entry, &proxy).await.unwrap();
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            client.apiserver_version(),
        )
        .await;
        tokio::time::timeout(std::time::Duration::from_secs(3), accept)
            .await
            .unwrap()
            .unwrap()
    }

    #[tokio::test]
    async fn http_proxy_url_sends_a_connect_from_every_client() {
        for entry in ENTRIES {
            let bytes = first_bytes_seen_by_proxy(entry, "http").await;
            let request = String::from_utf8_lossy(&bytes);
            assert!(
                request.starts_with("CONNECT kube.invalid:6443 "),
                "{entry:?}: {request}"
            );
        }
    }

    #[tokio::test]
    async fn socks5_proxy_url_sends_a_socks5_greeting_from_every_client() {
        for entry in ENTRIES {
            let bytes = first_bytes_seen_by_proxy(entry, "socks5").await;
            assert_eq!(bytes, [5, 1, 0], "{entry:?}");
        }
    }

    #[tokio::test]
    async fn unsupported_proxy_scheme_is_rejected_by_every_client() {
        let proxy: Uri = "ftp://127.0.0.1:2121".parse().unwrap();
        for entry in ENTRIES {
            let Err(error) = connect_through(entry, &proxy).await else {
                panic!("{entry:?} accepted an ftp proxy");
            };
            assert!(
                error.to_string().contains("Unsupported proxy scheme"),
                "{entry:?}: {error}"
            );
        }
    }

    #[tokio::test]
    async fn http_proxy_url_credentials_reach_connect() {
        use tokio::io::{
            AsyncReadExt,
            AsyncWriteExt,
        };
        use tower::Service;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = stream.read(&mut buf).await.unwrap();
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await;
            String::from_utf8_lossy(&buf[..n]).to_string()
        });
        let proxy: Uri = format!("http://user:p%40ss@127.0.0.1:{port}")
            .parse()
            .unwrap();
        let mut tunnel = create_http_tunnel(&proxy);
        let _ = tunnel
            .call("https://kube.invalid:6443".parse().unwrap())
            .await;
        let request = server.await.unwrap();
        assert!(
            request.contains("Proxy-Authorization: Basic dXNlcjpwQHNz\r\n"),
            "{request}"
        );
    }

    #[tokio::test]
    async fn socks5_proxy_url_credentials_offer_userpass() {
        use tokio::io::{
            AsyncReadExt,
            AsyncWriteExt,
        };
        use tower::Service;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut greeting = [0u8; 3];
            stream.read_exact(&mut greeting).await.unwrap();
            stream.write_all(&[5, 2]).await.unwrap();
            let mut auth = vec![0u8; 64];
            let n = stream.read(&mut auth).await.unwrap();
            (greeting, auth[..n].to_vec())
        });
        let proxy: Uri = format!("socks5://user:pass@127.0.0.1:{port}")
            .parse()
            .unwrap();
        let mut socks = create_socks5_proxy(&proxy);
        let call = tokio::spawn(async move {
            let _ = socks
                .call("https://kube.invalid:6443".parse().unwrap())
                .await;
        });
        let (greeting, auth) = server.await.unwrap();
        call.abort();
        assert_eq!(greeting, [5, 1, 2]);
        assert_eq!(auth, b"\x01\x04user\x04pass");
    }
}

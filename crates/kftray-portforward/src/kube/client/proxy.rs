use hyper::Uri;
use hyper_openssl::client::legacy::HttpsConnector;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use kube::client::ConfigExt;
use kube::config::Config;
use kube::Client;

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

    let hyper_client =
        hyper_util::client::legacy::Client::builder(TokioExecutor::new()).build(connector);
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

    let hyper_client =
        hyper_util::client::legacy::Client::builder(TokioExecutor::new()).build(connector);
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
    use hyper_util::client::legacy::connect::proxy::Tunnel;
    Tunnel::new(proxy_url.clone(), create_http_connector())
}

fn create_http_tunnel_with_connector(
    proxy_url: &Uri, connector: HttpConnector,
) -> hyper_util::client::legacy::connect::proxy::Tunnel<HttpConnector> {
    use hyper_util::client::legacy::connect::proxy::Tunnel;
    Tunnel::new(proxy_url.clone(), connector)
}

fn create_socks5_proxy(
    proxy_url: &Uri,
) -> hyper_util::client::legacy::connect::proxy::SocksV5<HttpConnector> {
    use hyper_util::client::legacy::connect::proxy::SocksV5;
    SocksV5::new(proxy_url.clone(), create_http_connector())
}

fn create_socks5_proxy_with_connector(
    proxy_url: &Uri, connector: HttpConnector,
) -> hyper_util::client::legacy::connect::proxy::SocksV5<HttpConnector> {
    use hyper_util::client::legacy::connect::proxy::SocksV5;
    SocksV5::new(proxy_url.clone(), connector)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proxy_scheme_validation() {
        let valid_schemes = ["http", "https", "socks5", "socks5h"];

        for scheme in valid_schemes {
            let url = format!("{scheme}://proxy.example.com:8080");
            let uri: Uri = url.parse().unwrap();
            assert!(uri.scheme_str().is_some());
        }
    }
}

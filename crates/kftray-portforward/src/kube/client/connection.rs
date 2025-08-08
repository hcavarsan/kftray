use std::future::Future;
use std::pin::Pin;
use std::sync::LazyLock;
use std::time::Duration;

use hyper_openssl::client::legacy::HttpsConnector;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use kube::client::ConfigExt;
use kube::config::Config;
use kube::Client;
use log::{
    error,
    info,
    warn,
};
use openssl::ssl::{
    SslConnector,
    SslMethod,
    SslVerifyMode,
};
use tower::ServiceBuilder;

use super::config::ConfigExtClone;
use super::error::{
    KubeClientError,
    KubeResult,
};
use super::proxy::{
    create_insecure_with_proxy,
    create_openssl_with_proxy,
    create_rustls_with_proxy,
};

type StrategyFuture<'a> = Pin<Box<dyn Future<Output = KubeResult<Client>> + Send + 'a>>;
type Strategy<'a> = (&'static str, StrategyFuture<'a>);

const CONNECTION_KEEPALIVE: Duration = Duration::from_secs(90);
const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
const POOL_MAX_IDLE_PER_HOST: usize = 20;

static HTTP_CONNECTOR: LazyLock<HttpConnector> = LazyLock::new(|| {
    let mut connector = HttpConnector::new();
    connector.set_keepalive(Some(CONNECTION_KEEPALIVE));
    connector.set_nodelay(true);
    connector.enforce_http(false);
    connector
});

pub async fn create_client_with_config(config: &Config) -> Option<Client> {
    let strategies = if config.accept_invalid_certs {
        info!("Creating insecure connection strategies for skip-tls-verify=true");
        create_insecure_connection_strategies(config)
    } else {
        create_connection_strategies(config)
    };

    execute_strategies(strategies).await
}

async fn execute_strategies(strategies: Vec<Strategy<'_>>) -> Option<Client> {
    let mut failed_attempts = Vec::new();
    let mut last_error = None;

    for (description, strategy) in strategies {
        info!("Attempting strategy: {description}");

        let result = match strategy.await {
            Ok(client) => test_client_connection(&client).await.map(|_| client),
            Err(e) => Err(e),
        };

        match result {
            Ok(client) => {
                info!("Successfully connected using: {description}");
                return Some(client);
            }
            Err(e) => {
                warn!("Strategy '{description}' failed: {e}");
                failed_attempts.push(description.to_string());
                last_error = Some(e);
            }
        }
    }

    log_connection_failure(&failed_attempts, last_error);
    None
}

fn create_connection_strategies(config: &Config) -> Vec<Strategy<'_>> {
    let original = config.clone();
    let without_invalid_certs = config.clone_with_invalid_certs(false);

    vec![
        // Try with original settings first
        create_rustls_strategy("Rustls (original settings)", original.clone()),
        create_openssl_strategy(
            "OpenSSL (original settings)",
            original.clone(),
            original.accept_invalid_certs,
        ),
        // Try with explicit certificate verification
        create_openssl_strategy(
            "OpenSSL (with cert verification)",
            without_invalid_certs,
            false,
        ),
    ]
}

fn create_insecure_connection_strategies(config: &Config) -> Vec<Strategy<'_>> {
    let original = config.clone();
    let with_invalid_certs = config.clone_with_invalid_certs(true);

    vec![
        create_openssl_strategy(
            "OpenSSL (skip cert verification)",
            with_invalid_certs.clone(),
            true,
        ),
        // Try with original settings
        create_openssl_strategy(
            "OpenSSL (original settings)",
            original.clone(),
            original.accept_invalid_certs,
        ),
        create_rustls_strategy("Rustls (original settings)", original),
        create_insecure_strategy("Insecure HTTP", with_invalid_certs),
    ]
}

fn create_rustls_strategy(description: &'static str, config: Config) -> Strategy<'static> {
    (
        description,
        Box::pin(async move { create_rustls_client(config).await }),
    )
}

fn create_openssl_strategy(
    description: &'static str, config: Config, skip_verify: bool,
) -> Strategy<'static> {
    (
        description,
        Box::pin(async move { create_openssl_client(config, skip_verify).await }),
    )
}

fn create_insecure_strategy(description: &'static str, config: Config) -> Strategy<'static> {
    (
        description,
        Box::pin(async move { create_insecure_client(config).await }),
    )
}

async fn create_rustls_client(config: Config) -> KubeResult<Client> {
    if let Some(proxy_url) = config.proxy_url.clone() {
        return create_rustls_with_proxy(config, &proxy_url).await;
    }

    let connector = config.rustls_https_connector().map_err(|e| {
        KubeClientError::connection_error_with_source("Failed to create Rustls connector", e)
    })?;

    let hyper_client =
        hyper_util::client::legacy::Client::builder(TokioExecutor::new()).build(connector);
    build_kube_client(config, hyper_client)
}

async fn create_openssl_client(config: Config, skip_verify: bool) -> KubeResult<Client> {
    if let Some(proxy_url) = config.proxy_url.clone() {
        let ssl_builder = config.openssl_ssl_connector_builder().map_err(|e| {
            KubeClientError::connection_error_with_source(
                "Failed to create SSL connector with client certificates",
                e,
            )
        })?;
        let http_connector = create_http_connector();
        return create_openssl_with_proxy(config, ssl_builder, http_connector, &proxy_url).await;
    }

    let ssl_connector = create_ssl_connector(skip_verify)?;
    let http_connector = create_http_connector();

    let https_connector =
        HttpsConnector::with_connector(http_connector, ssl_connector).map_err(|e| {
            KubeClientError::connection_error_with_source("Failed to create HTTPS connector", e)
        })?;

    let hyper_client = create_hyper_client(https_connector);
    build_kube_client(config, hyper_client)
}

async fn create_insecure_client(config: Config) -> KubeResult<Client> {
    let http_connector = create_http_connector();

    if let Some(proxy_url) = config.proxy_url.clone() {
        return create_insecure_with_proxy(config, http_connector, &proxy_url).await;
    }

    let hyper_client = create_hyper_client(http_connector);
    build_kube_client(config, hyper_client)
}

fn create_ssl_connector(skip_verify: bool) -> KubeResult<openssl::ssl::SslConnectorBuilder> {
    let mut builder = SslConnector::builder(SslMethod::tls()).map_err(|e| {
        KubeClientError::connection_error_with_source(
            "Failed to create SSL connector (ensure OpenSSL is properly configured)",
            e,
        )
    })?;

    builder.set_verify(if skip_verify {
        SslVerifyMode::NONE
    } else {
        SslVerifyMode::PEER
    });

    Ok(builder)
}

pub fn create_http_connector() -> HttpConnector {
    HTTP_CONNECTOR.clone()
}

pub fn create_hyper_client<C>(
    connector: C,
) -> hyper_util::client::legacy::Client<C, kube::client::Body>
where
    C: hyper_util::client::legacy::connect::Connect + Clone + Send + Sync + 'static,
{
    use hyper_util::rt::TokioTimer;

    hyper_util::client::legacy::Client::builder(TokioExecutor::new())
        .pool_idle_timeout(POOL_IDLE_TIMEOUT)
        .pool_max_idle_per_host(POOL_MAX_IDLE_PER_HOST)
        .retry_canceled_requests(true)
        .timer(TokioTimer::new())
        .build(connector)
}

async fn test_client_connection(client: &Client) -> KubeResult<()> {
    client.apiserver_version().await.map_err(|e| {
        KubeClientError::connection_error_with_source(
            "Failed to connect to Kubernetes API server",
            e,
        )
    })?;
    Ok(())
}

pub fn build_kube_client<C>(
    config: Config, hyper_client: hyper_util::client::legacy::Client<C, kube::client::Body>,
) -> KubeResult<Client>
where
    C: hyper_util::client::legacy::connect::Connect + Clone + Send + Sync + 'static,
{
    let auth_layer = config
        .auth_layer()
        .map_err(|e| KubeClientError::auth_error_with_source("Failed to create auth layer", e))?;

    let service = ServiceBuilder::new()
        .layer(config.base_uri_layer())
        .option_layer(auth_layer)
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })
        .service(hyper_client);

    Ok(Client::new(service, config.default_namespace))
}

fn log_connection_failure(failed_attempts: &[String], last_error: Option<KubeClientError>) {
    if failed_attempts.is_empty() {
        error!("No connection strategies available");
        return;
    }

    let strategies_list = failed_attempts.join(", ");
    match last_error {
        Some(err) => error!(
            "All connection strategies failed. Last error: {}. Attempted {} strategies: {}",
            err,
            failed_attempts.len(),
            strategies_list
        ),
        None => error!(
            "All {} connection strategies failed: {}",
            failed_attempts.len(),
            strategies_list
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_strategies_count() {
        let config = Config::new("https://example.com".parse().unwrap());
        let strategies = create_connection_strategies(&config);
        assert_eq!(strategies.len(), 3);

        let mut insecure_config = Config::new("https://example.com".parse().unwrap());
        insecure_config.accept_invalid_certs = true;
        let insecure_strategies = create_insecure_connection_strategies(&insecure_config);
        assert_eq!(insecure_strategies.len(), 4);
    }

    #[test]
    fn test_http_connector_configuration() {
        let connector = create_http_connector();
        drop(connector);
    }

    #[test]
    fn test_ssl_connector_verify_modes() {
        let connector_verify = create_ssl_connector(false);
        assert!(connector_verify.is_ok());

        let connector_no_verify = create_ssl_connector(true);
        assert!(connector_no_verify.is_ok());
    }

    #[test]
    fn test_proxy_url_parsing() {
        let mut config = Config::new("https://example.com".parse().unwrap());
        config.proxy_url = Some("http://proxy.example.com:8080".parse().unwrap());

        assert!(config.proxy_url.is_some());
        let proxy_url = config.proxy_url.as_ref().unwrap();
        assert_eq!(proxy_url.scheme_str(), Some("http"));
        assert_eq!(proxy_url.host(), Some("proxy.example.com"));
        assert_eq!(proxy_url.port_u16(), Some(8080));
    }

    #[test]
    fn test_socks5_proxy_url() {
        let mut config = Config::new("https://example.com".parse().unwrap());
        config.proxy_url = Some("socks5://localhost:1080".parse().unwrap());

        assert!(config.proxy_url.is_some());
        let proxy_url = config.proxy_url.as_ref().unwrap();
        assert_eq!(proxy_url.scheme_str(), Some("socks5"));
        assert_eq!(proxy_url.host(), Some("localhost"));
        assert_eq!(proxy_url.port_u16(), Some(1080));
    }

    #[test]
    fn test_error_creation() {
        let error = KubeClientError::connection_error("Test connection error");
        assert!(matches!(error, KubeClientError::ConnectionError { .. }));
        assert_eq!(error.to_string(), "Connection error: Test connection error");
    }
}

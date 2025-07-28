use std::future::Future;
use std::pin::Pin;

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

type StrategyFuture<'a> = Pin<Box<dyn Future<Output = KubeResult<Client>> + Send + 'a>>;
type Strategy<'a> = (&'static str, StrategyFuture<'a>);

pub async fn create_client_with_config(config: &Config) -> Option<Client> {
    let config_with_invalid_certs_true = config.clone_with_invalid_certs(true);
    let config_with_invalid_certs_false = config.clone_with_invalid_certs(false);

    let strategies = create_strategies(
        config,
        config_with_invalid_certs_true,
        config_with_invalid_certs_false,
    );

    let mut failed_attempts = Vec::new();
    let mut last_error: Option<KubeClientError> = None;

    for (description, strategy) in strategies {
        info!("Attempting strategy: {description}");
        match try_create_client(description, strategy).await {
            Ok(client) => {
                info!("Successfully connected using: {description}");
                return Some(client);
            }
            Err(err) => {
                warn!("Strategy '{description}' failed: {err}");
                failed_attempts.push(description.to_string());
                last_error = Some(err);
            }
        }
    }

    if !failed_attempts.is_empty() {
        let strategy_error =
            KubeClientError::strategy_failed("All connection strategies", failed_attempts.clone());

        if let Some(last_err) = last_error {
            error!(
                "All connection strategies failed. Last error: {}. Attempted {} strategies: {}",
                last_err,
                failed_attempts.len(),
                failed_attempts.join(", ")
            );
        } else {
            error!("All connection strategies failed: {strategy_error}");
        }
    } else {
        error!("No connection strategies available");
    }

    None
}

fn create_strategies<'a>(
    original_config: &Config, config_with_invalid_certs_true: Config,
    config_with_invalid_certs_false: Config,
) -> Vec<Strategy<'a>> {
    vec![
        create_rustls_strategy(
            "Rustls HTTPS connector (respecting kubeconfig settings)",
            original_config.clone(),
        ),
        create_openssl_strategy(
            "OpenSSL HTTPS connector (respecting kubeconfig settings)",
            original_config.clone(),
            original_config.accept_invalid_certs,
        ),
        create_openssl_strategy(
            "OpenSSL HTTPS connector (with verification)",
            config_with_invalid_certs_false.clone(),
            false,
        ),
        create_openssl_strategy(
            "OpenSSL HTTPS connector (without verification)",
            config_with_invalid_certs_true.clone(),
            true,
        ),
        create_openssl_strategy(
            "OpenSSL HTTPS connector (accept invalid certs and with verification)",
            config_with_invalid_certs_true.clone(),
            false,
        ),
        create_rustls_strategy(
            "Rustls HTTPS connector (accept invalid certs)",
            config_with_invalid_certs_true.clone(),
        ),
        create_rustls_strategy(
            "Rustls HTTPS connector (do not accept invalid certs)",
            config_with_invalid_certs_false.clone(),
        ),
        create_insecure_strategy(
            "Insecure HTTP connector (do not accept invalid certs)",
            config_with_invalid_certs_false,
        ),
        create_insecure_strategy(
            "Insecure HTTP connector (accept invalid certs)",
            config_with_invalid_certs_true,
        ),
    ]
}

fn create_rustls_strategy<'a>(description: &'static str, config: Config) -> Strategy<'a> {
    (
        description,
        Box::pin(async move { create_rustls_https_connector(&config).await }),
    )
}

fn create_openssl_strategy<'a>(
    description: &'static str, config: Config, accept_invalid_certs: bool,
) -> Strategy<'a> {
    (
        description,
        Box::pin(async move {
            let verify_mode = if accept_invalid_certs {
                SslVerifyMode::NONE
            } else {
                SslVerifyMode::PEER
            };
            create_openssl_https_connector(&config, verify_mode).await
        }),
    )
}

fn create_insecure_strategy<'a>(description: &'static str, config: Config) -> Strategy<'a> {
    (
        description,
        Box::pin(async move { create_insecure_http_client(&config).await }),
    )
}

async fn try_create_client(
    description: &str, strategy: StrategyFuture<'_>,
) -> Result<Client, KubeClientError> {
    info!("Attempting to create client with {description}");
    match strategy.await {
        Ok(client) => match test_client(&client).await {
            Ok(()) => {
                info!("Successfully created client with {description}");
                Ok(client)
            }
            Err(e) => {
                warn!("{description} failed connection test: {e}");
                Err(e)
            }
        },
        Err(e) => {
            warn!("Failed to create {description}: {e}");
            Err(e)
        }
    }
}

async fn create_openssl_https_connector(
    config: &Config, verify_mode: SslVerifyMode,
) -> KubeResult<Client> {
    let ssl_method = SslMethod::tls();
    let mut builder = SslConnector::builder(ssl_method).map_err(|e| {
        KubeClientError::connection_error_with_source(
            "Failed to create SSL connector (ensure OpenSSL is properly configured)",
            e,
        )
    })?;
    builder.set_verify(verify_mode);
    let https_connector =
        HttpsConnector::with_connector(HttpConnector::new(), builder).map_err(|e| {
            KubeClientError::connection_error_with_source("Failed to create HTTPS connector", e)
        })?;

    let auth_layer = match config.auth_layer() {
        Ok(Some(layer)) => Some(layer),
        Ok(None) => {
            warn!("No auth layer found for OpenSSL connector");
            None
        }
        Err(e) => {
            warn!("Failed to get auth layer (possible certificate validation issue): {e}");
            return Err(KubeClientError::auth_error_with_source(
                "Failed to get auth layer for OpenSSL connector",
                e,
            ));
        }
    };

    let service = ServiceBuilder::new()
        .layer(config.base_uri_layer())
        .option_layer(auth_layer)
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })
        .service(
            hyper_util::client::legacy::Client::builder(TokioExecutor::new())
                .build(https_connector),
        );

    let client = Client::new(service, config.default_namespace.clone());
    Ok(client)
}

async fn create_rustls_https_connector(config: &Config) -> KubeResult<Client> {
    let https_connector = config.rustls_https_connector().map_err(|e| {
        KubeClientError::connection_error_with_source("Failed to create Rustls HTTPS connector", e)
    })?;

    let auth_layer = match config.auth_layer() {
        Ok(Some(layer)) => Some(layer),
        Ok(None) => {
            warn!("No auth layer found for Rustls connector");
            None
        }
        Err(e) => {
            warn!("Failed to get auth layer (possible certificate validation issue): {e}");
            return Err(KubeClientError::auth_error_with_source(
                "Failed to get auth layer for Rustls connector",
                e,
            ));
        }
    };

    let service = ServiceBuilder::new()
        .layer(config.base_uri_layer())
        .option_layer(auth_layer)
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })
        .service(
            hyper_util::client::legacy::Client::builder(TokioExecutor::new())
                .build(https_connector),
        );

    let client = Client::new(service, config.default_namespace.clone());
    Ok(client)
}

async fn create_insecure_http_client(config: &Config) -> KubeResult<Client> {
    let http_connector = HttpConnector::new();

    let service = ServiceBuilder::new()
        .layer(config.base_uri_layer())
        .option_layer({
            match config.auth_layer() {
                Ok(layer) => layer,
                Err(e) => {
                    warn!("Failed to get auth layer for insecure client: {e}");
                    None
                }
            }
        })
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })
        .service(
            hyper_util::client::legacy::Client::builder(TokioExecutor::new()).build(http_connector),
        );

    let client = Client::new(service, config.default_namespace.clone());
    Ok(client)
}

async fn test_client(client: &Client) -> KubeResult<()> {
    match client.apiserver_version().await {
        Ok(version) => {
            info!("Kubernetes API server version: {version:?}");
            Ok(())
        }
        Err(e) => {
            warn!("Failed to connect to Kubernetes API server: {e}");
            Err(KubeClientError::connection_error_with_source(
                "Failed to connect to Kubernetes API server. Possible causes: invalid certificates, unreachable server, authentication failure, or network connectivity issues",
                e
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_strategies_ordering() {
        let original_config = {
            let mut config = Config::new("https://example.com".parse().unwrap());
            config.accept_invalid_certs = true;
            config
        };

        let config_true = original_config.clone_with_invalid_certs(true);
        let config_false = original_config.clone_with_invalid_certs(false);

        let strategies = create_strategies(&original_config, config_true, config_false);

        assert!(strategies.len() >= 2);
        assert_eq!(
            strategies[0].0,
            "Rustls HTTPS connector (respecting kubeconfig settings)"
        );
        assert_eq!(
            strategies[1].0,
            "OpenSSL HTTPS connector (respecting kubeconfig settings)"
        );
    }
}

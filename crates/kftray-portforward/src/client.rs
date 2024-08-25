use std::path::PathBuf;

use anyhow::{Context, Result};
use hyper_util::rt::TokioExecutor;
use kftray_commons::config_dir::get_kubeconfig_paths;
use kube::{
    client::ConfigExt,
    config::{Config, KubeConfigOptions, Kubeconfig},
    Client,
};
use log::{info, warn, error};
use tower::ServiceBuilder;

pub async fn create_client_with_specific_context(
    kubeconfig: Option<String>, context_name: Option<&str>,
) -> Result<(Option<Client>, Option<Kubeconfig>, Vec<String>)> {
    let kubeconfig_paths = get_kubeconfig_paths_from_option(kubeconfig)?;

    let mut errors = Vec::new();
    let mut all_contexts = Vec::new();

    for path in &kubeconfig_paths {
        info!("Attempting to read kubeconfig from path: {:?}", path);

        match Kubeconfig::read_from(path)
            .context(format!("Failed to read kubeconfig from {:?}", path))
        {
            Ok(kubeconfig) => {
                info!("Successfully read kubeconfig from {:?}", path);
                let contexts = list_contexts(&kubeconfig);
                all_contexts.extend(contexts.clone());
                info!("Available contexts in {:?}: {:?}", path, contexts);

                if let Some(context_name) = context_name {
                    match create_config_with_context(&kubeconfig, context_name).await {
                        Ok(config) => {
                            info!("Successfully created configuration for context: {}", context_name);
                            if let Some(client) = create_client_with_config(&config).await {
                                info!("Successfully created client for context: {}", context_name);
                                return Ok((Some(client), Some(kubeconfig), all_contexts));
                            } else {
                                let error_msg = format!(
                                    "Failed to create HTTPS connector for context: {} in path: {:?}",
                                    context_name, path
                                );
                                warn!("{}", error_msg);
                                errors.push(error_msg);
                            }
                        }
                        Err(e) => {
                            let error_msg = format!(
                                "Failed to create configuration from kubeconfig for context: {} in path: {:?}: {}",
                                context_name, path, e
                            );
                            error!("{}", error_msg);
                            errors.push(error_msg);
                        }
                    }
                }
            }
            Err(e) => {
                let error_msg = format!("Failed to read kubeconfig from {:?}: {}", path, e);
                error!("{}", error_msg);
                errors.push(error_msg);
            }
        }
    }

    if context_name.is_none() {
        info!("No specific context provided, returning all available contexts.");
        return Ok((None, None, all_contexts));
    }

    Err(anyhow::anyhow!(
        "Unable to create client with any of the provided kubeconfig paths: {}",
        errors.join("; ")
    ))
}

fn get_kubeconfig_paths_from_option(kubeconfig: Option<String>) -> Result<Vec<PathBuf>> {
    match kubeconfig {
        Some(path) if path == "default" => {
            info!("Using default kubeconfig paths.");
            get_kubeconfig_paths()
        },
        Some(path) => {
            info!("Using provided kubeconfig paths: {}", path);
            Ok(path.split(':').map(PathBuf::from).collect())
        },
        None => {
            info!("No kubeconfig path provided, using default paths.");
            get_kubeconfig_paths()
        },
    }
}

async fn create_config_with_context(
    kubeconfig: &Kubeconfig,
    context_name: &str,
) -> Result<Config> {
    info!("Creating configuration for context: {}", context_name);
    Config::from_custom_kubeconfig(
        kubeconfig.clone(),
        &KubeConfigOptions {
            context: Some(context_name.to_owned()),
            ..Default::default()
        },
    )
    .await
    .context("Failed to create configuration from kubeconfig")
}

async fn create_client_with_config(config: &Config) -> Option<Client> {
    info!("Attempting to create client with OpenSSL HTTPS connector.");
    match config.openssl_https_connector() {
        Ok(https_connector) => {
            let service = ServiceBuilder::new()
                .layer(config.base_uri_layer())
                .option_layer(config.auth_layer().ok()?)
                .service(
                    hyper_util::client::legacy::Client::builder(TokioExecutor::new())
                        .build(https_connector),
                );

            let client = Client::new(service, config.default_namespace.clone());
            info!("Successfully configured client with OpenSSL.");
            Some(client)
        }
        Err(openssl_err) => {
            warn!("Failed to create OpenSSL HTTPS connector: {}", openssl_err);
            info!("Attempting to create client with Rustls HTTPS connector.");
            match config.rustls_https_connector() {
                Ok(https_connector) => {
                    let service = ServiceBuilder::new()
                        .layer(config.base_uri_layer())
                        .option_layer(config.auth_layer().ok()?)
                        .service(
                            hyper_util::client::legacy::Client::builder(TokioExecutor::new())
                                .build(https_connector),
                        );

                    let client = Client::new(service, config.default_namespace.clone());
                    info!("Successfully configured client with Rustls.");
                    Some(client)
                }
                Err(rustls_err) => {
                    error!("Failed to create Rustls HTTPS connector: {}", rustls_err);
                    None
                }
            }
        }
    }
}

fn list_contexts(kubeconfig: &Kubeconfig) -> Vec<String> {
    kubeconfig
        .contexts
        .iter()
        .map(|context| context.name.clone())
        .collect()
}

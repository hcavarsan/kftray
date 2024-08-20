use std::path::PathBuf;

use anyhow::{
    Context,
    Result,
};
use hyper_util::rt::TokioExecutor;
use kftray_commons::config_dir::get_kubeconfig_paths;
use kube::{
    client::ConfigExt,
    config::{
        Config,
        KubeConfigOptions,
        Kubeconfig,
    },
    Client,
};
use log::info;
use tower::ServiceBuilder;

pub async fn create_client_with_specific_context(
    kubeconfig: Option<String>, context_name: Option<&str>,
) -> Result<(Option<Client>, Option<Kubeconfig>, Vec<String>)> {
    let kubeconfig_paths = match kubeconfig {
        Some(path) if path == "default" => get_kubeconfig_paths()?,
        Some(path) => path.split(':').map(PathBuf::from).collect(),
        None => get_kubeconfig_paths()?,
    };

    let mut errors = Vec::new();
    let mut all_contexts = Vec::new();

    for path in &kubeconfig_paths {
        info!("Trying kubeconfig path: {:?}", path);

        match Kubeconfig::read_from(path)
            .context(format!("Failed to read kubeconfig from {:?}", path))
        {
            Ok(kubeconfig) => {
                info!("Successfully read kubeconfig from {:?}", path);
                let contexts = list_contexts(&kubeconfig);
                all_contexts.extend(contexts.clone());
                info!("Available contexts: {:?}", contexts);

                if let Some(context_name) = context_name {
                    match Config::from_custom_kubeconfig(
                        kubeconfig.clone(),
                        &KubeConfigOptions {
                            context: Some(context_name.to_owned()),
                            ..Default::default()
                        },
                    )
                    .await
                    .context("Failed to create configuration from kubeconfig")
                    {
                        Ok(config) => {
                            info!(
                                "Successfully created configuration for context: {}",
                                context_name
                            );
                            match config
                                .rustls_https_connector()
                                .context("Failed to create Rustls HTTPS connector")
                            {
                                Ok(https_connector) => {
                                    let service = ServiceBuilder::new()
                                        .layer(config.base_uri_layer())
                                        .option_layer(config.auth_layer()?)
                                        .service(
                                            hyper_util::client::legacy::Client::builder(
                                                TokioExecutor::new(),
                                            )
                                            .build(https_connector),
                                        );

                                    let client = Client::new(service, config.default_namespace);
                                    return Ok((Some(client), Some(kubeconfig), all_contexts));
                                }
                                Err(e) => {
                                    let error_msg = format!(
                                        "Failed to create Rustls HTTPS connector for {:?}: {}",
                                        path, e
                                    );
                                    info!("{}", error_msg);
                                    errors.push(error_msg);
                                }
                            }
                        }
                        Err(e) => {
                            let error_msg = format!(
                                "Failed to create configuration from kubeconfig for {:?}: {}",
                                path, e
                            );
                            info!("{}", error_msg);
                            errors.push(error_msg);
                        }
                    }
                }
            }
            Err(e) => {
                let error_msg = format!("Failed to read kubeconfig from {:?}: {}", path, e);
                info!("{}", error_msg);
                errors.push(error_msg);
            }
        }
    }

    if context_name.is_none() {
        return Ok((None, None, all_contexts));
    }

    Err(anyhow::anyhow!(
        "Unable to create client with any of the provided kubeconfig paths: {}",
        errors.join("; ")
    ))
}

fn list_contexts(kubeconfig: &Kubeconfig) -> Vec<String> {
    kubeconfig
        .contexts
        .iter()
        .map(|context| context.name.clone())
        .collect()
}

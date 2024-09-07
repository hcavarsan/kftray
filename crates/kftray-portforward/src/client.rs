use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;

use anyhow::{
    Context,
    Result,
};
use hyper_util::rt::TokioExecutor;
use k8s_openapi::api::core::v1::Namespace;
use k8s_openapi::api::core::v1::Service;
use k8s_openapi::api::core::v1::ServiceSpec;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kftray_commons::config_dir::get_kubeconfig_paths;
use kube::api::ListParams;
use kube::Api;
use kube::{
    client::ConfigExt,
    config::{
        Config,
        KubeConfigOptions,
        Kubeconfig,
    },
    Client,
};
use log::{
    error,
    info,
    warn,
};
use tower::ServiceBuilder;

use crate::models::kube::KubeContextInfo;

pub async fn create_client_with_specific_context(
    kubeconfig: Option<String>, context_name: Option<&str>,
) -> Result<(Option<Client>, Option<Kubeconfig>, Vec<String>)> {
    let kubeconfig_paths = get_kubeconfig_paths_from_option(kubeconfig)?;

    let mut errors = Vec::new();
    let mut all_contexts = Vec::new();
    let mut merged_kubeconfig = Kubeconfig::default();

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

                merged_kubeconfig = merged_kubeconfig.merge(kubeconfig)?;
            }
            Err(e) => {
                let error_msg = format!("Failed to read kubeconfig from {:?}: {}", path, e);
                error!("{}", error_msg);
                errors.push(error_msg);
            }
        }
    }

    if let Some(context_name) = context_name {
        match create_config_with_context(&merged_kubeconfig, context_name).await {
            Ok(config) => {
                info!(
                    "Successfully created configuration for context: {}",
                    context_name
                );
                if let Some(client) = create_client_with_config(&config).await {
                    info!("Successfully created client for context: {}", context_name);
                    return Ok((Some(client), Some(merged_kubeconfig), all_contexts));
                } else {
                    let error_msg = format!(
                        "Failed to create HTTPS connector for context: {}",
                        context_name
                    );
                    warn!("{}", error_msg);
                    errors.push(error_msg);
                }
            }
            Err(e) => {
                let error_msg = format!(
                    "Failed to create configuration for context: {}: {}",
                    context_name, e
                );
                error!("{}", error_msg);
                errors.push(error_msg);
            }
        }
    } else {
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
        }
        Some(path) => {
            info!("Using provided kubeconfig paths: {}", path);
            Ok(path.split(':').map(PathBuf::from).collect())
        }
        None => {
            info!("No kubeconfig path provided, using default paths.");
            get_kubeconfig_paths()
        }
    }
}

async fn create_config_with_context(kubeconfig: &Kubeconfig, context_name: &str) -> Result<Config> {
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
                .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
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
                        .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
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

pub async fn list_kube_contexts(
    kubeconfig: Option<String>,
) -> Result<Vec<KubeContextInfo>, String> {
    info!("list_kube_contexts {}", kubeconfig.as_deref().unwrap_or(""));

    let (_, kubeconfig, contexts) = create_client_with_specific_context(kubeconfig, None)
        .await
        .map_err(|err| format!("Failed to create client: {}", err))?;

    if let Some(kubeconfig) = kubeconfig {
        let contexts: Vec<KubeContextInfo> = kubeconfig
            .contexts
            .into_iter()
            .map(|c| KubeContextInfo { name: c.name })
            .collect();

        Ok(contexts)
    } else if !contexts.is_empty() {
        let context_infos: Vec<KubeContextInfo> = contexts
            .into_iter()
            .map(|name| KubeContextInfo { name })
            .collect();

        Ok(context_infos)
    } else {
        Err("Failed to retrieve kubeconfig".to_string())
    }
}

pub async fn list_all_namespaces(client: Client) -> Result<Vec<String>, anyhow::Error> {
    let namespaces: Api<Namespace> = Api::all(client);
    let namespace_list = namespaces.list(&ListParams::default()).await?;

    let mut namespace_names = Vec::new();
    for namespace in namespace_list {
        if let Some(name) = namespace.metadata.name {
            namespace_names.push(name);
        }
    }

    Ok(namespace_names)
}
pub async fn get_services_with_annotation(
    client: Client, namespace: &str, _: &str,
) -> Result<Vec<(String, HashMap<String, String>, HashMap<String, i32>)>, Box<dyn std::error::Error>>
{
    let services: Api<Service> = Api::namespaced(client, namespace);
    let lp = ListParams::default();

    let service_list = services.list(&lp).await?;

    let mut results = Vec::new();

    for service in service_list {
        if let Some(service_name) = service.metadata.name.clone() {
            if let Some(annotations) = &service.metadata.annotations {
                if annotations
                    .get("kftray.app/enabled")
                    .map_or(false, |v| v == "true")
                {
                    let ports = extract_ports_from_service(&service);
                    let annotations_hashmap: HashMap<String, String> =
                        annotations.clone().into_iter().collect();
                    results.push((service_name, annotations_hashmap, ports));
                }
            }
        }
    }

    Ok(results)
}

fn extract_ports_from_service(service: &Service) -> HashMap<String, i32> {
    let mut ports = HashMap::new();
    if let Some(spec) = &service.spec {
        for port in spec.ports.as_ref().unwrap_or(&vec![]) {
            let port_number = match port.target_port {
                Some(IntOrString::Int(port)) => port,
                Some(IntOrString::String(ref name)) => {
                    resolve_named_port(spec, name).unwrap_or_default()
                }
                None => continue,
            };
            ports.insert(
                port.name.clone().unwrap_or_else(|| port_number.to_string()),
                port_number,
            );
        }
    }
    ports
}

fn resolve_named_port(spec: &ServiceSpec, name: &str) -> Option<i32> {
    spec.ports.as_ref()?.iter().find_map(|port| {
        if port.name.as_deref() == Some(name) {
            Some(port.port)
        } else {
            None
        }
    })
}

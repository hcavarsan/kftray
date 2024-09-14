use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use anyhow::{
    Context,
    Result,
};
use futures::future::join_all;
use hyper_openssl::client::legacy::HttpsConnector;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use k8s_openapi::api::core::v1::{
    Namespace,
    Service,
    ServiceSpec,
};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kftray_commons::config_dir::get_kubeconfig_paths;
use kube::api::ListParams;
use kube::client::ConfigExt;
use kube::{
    config::{
        Config,
        KubeConfigOptions,
        Kubeconfig,
    },
    Api,
    Client,
};
use log::{
    error,
    info,
    warn,
};
use openssl::base64::{
    decode_block,
    encode_block,
};
use openssl::pkey::PKey;
use openssl::ssl::{
    SslConnector,
    SslMethod,
    SslVerifyMode,
};
use secrecy::ExposeSecret;
use tower::ServiceBuilder;

use crate::models::kube::KubeContextInfo;

trait ConfigExtClone {
    fn clone_with_invalid_certs(&self, accept_invalid_certs: bool) -> Self;
}

impl ConfigExtClone for Config {
    fn clone_with_invalid_certs(&self, accept_invalid_certs: bool) -> Self {
        let mut config = self.clone();
        config.accept_invalid_certs = accept_invalid_certs;
        config
    }
}

type StrategyFuture<'a> = Pin<
    Box<dyn Future<Output = Result<Client, Box<dyn std::error::Error + Send + Sync>>> + Send + 'a>,
>;

type Strategy<'a> = (&'static str, StrategyFuture<'a>);

type ServiceInfo = (String, HashMap<String, String>, HashMap<String, i32>);

pub async fn create_client_with_specific_context(
    kubeconfig: Option<String>, context_name: Option<&str>,
) -> Result<(Option<Client>, Option<Kubeconfig>, Vec<String>)> {
    let kubeconfig_paths = get_kubeconfig_paths_from_option(kubeconfig)?;
    let (merged_kubeconfig, all_contexts, mut errors) = merge_kubeconfigs(&kubeconfig_paths)?;

    if let Some(context_name) = context_name {
        match create_config_with_context(&merged_kubeconfig, context_name).await {
            Ok(config) => {
                if let Some(client) = create_client_with_config(&config).await {
                    return Ok((Some(client), Some(merged_kubeconfig), all_contexts));
                } else {
                    errors.push(format!(
                        "Failed to create HTTPS connector for context: {}",
                        context_name
                    ));
                }
            }
            Err(e) => {
                errors.push(format!(
                    "Failed to create configuration for context: {}: {}",
                    context_name, e
                ));
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

fn merge_kubeconfigs(paths: &[PathBuf]) -> Result<(Kubeconfig, Vec<String>, Vec<String>)> {
    let mut errors = Vec::new();
    let mut all_contexts = Vec::new();
    let mut merged_kubeconfig = Kubeconfig::default();

    for path in paths {
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

    Ok((merged_kubeconfig, all_contexts, errors))
}

async fn create_config_with_context(kubeconfig: &Kubeconfig, context_name: &str) -> Result<Config> {
    info!("Creating configuration for context: {}", context_name);
    let mut kubeconfig = kubeconfig.clone();

    for auth_info in &mut kubeconfig.auth_infos {
        if let Some(client_key_data) = &auth_info.auth_info.clone().unwrap().client_key_data {
            let decoded_key = decode_block(client_key_data.expose_secret())
                .context("Failed to decode client key data")?;

            if is_pkcs8_key(&decoded_key) {
                let converted_key = convert_pkcs8_to_pkcs1(&decoded_key)
                    .context("Failed to convert PKCS#8 key to PKCS#1")?;
                let encoded_key = encode_block(&converted_key);
                auth_info.auth_info.clone().unwrap().client_key_data = Some(encoded_key.into());
            }
        }
    }

    Config::from_custom_kubeconfig(
        kubeconfig,
        &KubeConfigOptions {
            context: Some(context_name.to_owned()),
            ..Default::default()
        },
    )
    .await
    .context("Failed to create configuration from kubeconfig")
}

async fn create_client_with_config(config: &Config) -> Option<Client> {
    let config_with_invalid_certs_true = config.clone_with_invalid_certs(true);
    let config_with_invalid_certs_false = config.clone_with_invalid_certs(false);

    let strategies = create_strategies(
        config_with_invalid_certs_true,
        config_with_invalid_certs_false,
    );

    let futures: Vec<_> = strategies
        .into_iter()
        .map(|(description, strategy)| try_create_client(description, strategy))
        .collect();

    let results = join_all(futures).await;

    results.into_iter().flatten().next()
}

fn create_strategies<'a>(
    config_with_invalid_certs_true: Config, config_with_invalid_certs_false: Config,
) -> Vec<Strategy<'a>> {
    vec![
        (
            "OpenSSL HTTPS connector (with verification)",
            Box::pin({
                let config = config_with_invalid_certs_false.clone();
                async move { create_openssl_https_connector(&config, SslVerifyMode::PEER).await }
            }),
        ),
        (
            "OpenSSL HTTPS connector (without verification)",
            Box::pin({
                let config = config_with_invalid_certs_false.clone();
                async move { create_openssl_https_connector(&config, SslVerifyMode::NONE).await }
            }),
        ),
        (
            "OpenSSL HTTPS connector (accept invalid certs and without verification)",
            Box::pin({
                let config = config_with_invalid_certs_true.clone();
                async move { create_openssl_https_connector(&config, SslVerifyMode::NONE).await }
            }),
        ),
        (
            "OpenSSL HTTPS connector (accept invalid certs and with verification)",
            Box::pin({
                let config = config_with_invalid_certs_true.clone();
                async move { create_openssl_https_connector(&config, SslVerifyMode::PEER).await }
            }),
        ),
        (
            "Rustls HTTPS connector (accept invalid certs)",
            Box::pin({
                let config = config_with_invalid_certs_true.clone();
                async move { create_rustls_https_connector(&config).await }
            }),
        ),
        (
            "Rustls HTTPS connector (do not accept invalid certs)",
            Box::pin({
                let config = config_with_invalid_certs_false.clone();
                async move { create_rustls_https_connector(&config).await }
            }),
        ),
        (
            "Insecure HTTP connector (do not accept invalid certs)",
            Box::pin({
                let config = config_with_invalid_certs_false.clone();
                async move { create_insecure_http_client(&config).await }
            }),
        ),
        (
            "Insecure HTTP connector (accept invalid certs)",
            Box::pin({
                let config = config_with_invalid_certs_true.clone();
                async move { create_insecure_http_client(&config).await }
            }),
        ),
    ]
}

async fn try_create_client(description: &str, strategy: StrategyFuture<'_>) -> Option<Client> {
    info!("Attempting to create client with {}", description);
    match strategy.await {
        Ok(client) => {
            if test_client(&client).await.is_ok() {
                info!("Successfully created client with {}", description);
                Some(client)
            } else {
                warn!("{} failed to connect.", description);
                None
            }
        }
        Err(e) => {
            warn!("Failed to create {}: {}", description, e);
            None
        }
    }
}

async fn create_openssl_https_connector(
    config: &Config, verify_mode: SslVerifyMode,
) -> Result<Client, Box<dyn std::error::Error + Send + Sync>> {
    let mut builder = SslConnector::builder(SslMethod::tls())?;
    builder.set_verify(verify_mode);
    let https_connector = HttpsConnector::with_connector(HttpConnector::new(), builder)?;

    let auth_layer = match config.auth_layer() {
        Ok(Some(layer)) => Some(layer),
        Ok(None) => {
            warn!("No auth layer found");
            None
        }
        Err(e) => {
            warn!("Failed to get auth layer: {}", e);
            return Err(Box::new(e));
        }
    };

    let service = ServiceBuilder::new()
        .layer(config.base_uri_layer())
        .option_layer(auth_layer)
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
        .service(
            hyper_util::client::legacy::Client::builder(TokioExecutor::new())
                .build(https_connector),
        );

    let client = Client::new(service, config.default_namespace.clone());
    Ok(client)
}

async fn create_rustls_https_connector(
    config: &Config,
) -> Result<Client, Box<dyn std::error::Error + Send + Sync>> {
    let https_connector = config.rustls_https_connector()?;

    let auth_layer = match config.auth_layer() {
        Ok(Some(layer)) => Some(layer),
        Ok(None) => {
            warn!("No auth layer found");
            None
        }
        Err(e) => {
            warn!("Failed to get auth layer: {}", e);
            return Err(Box::new(e));
        }
    };

    let service = ServiceBuilder::new()
        .layer(config.base_uri_layer())
        .option_layer(auth_layer)
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
        .service(
            hyper_util::client::legacy::Client::builder(TokioExecutor::new())
                .build(https_connector),
        );

    let client = Client::new(service, config.default_namespace.clone());
    Ok(client)
}

async fn create_insecure_http_client<'a>(
    config: &Config,
) -> Result<Client, Box<dyn std::error::Error + Send + Sync>> {
    let http_connector = HttpConnector::new();

    let service = ServiceBuilder::new()
        .layer(config.base_uri_layer())
        .option_layer(
            config.auth_layer().or_else(|_| {
                Ok::<Option<kube::client::middleware::AuthLayer>, anyhow::Error>(None)
            })?,
        )
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
        .service(
            hyper_util::client::legacy::Client::builder(TokioExecutor::new()).build(http_connector),
        );

    let client = Client::new(service, config.default_namespace.clone());
    Ok(client)
}

async fn test_client(client: &Client) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let api: Api<Namespace> = Api::all(client.clone());
    api.list(&ListParams::default())
        .await
        .map(|_| ())
        .map_err(|e| {
            let err_msg = format!("Failed to list namespaces: {}", e);
            Box::new(std::io::Error::new(std::io::ErrorKind::Other, err_msg))
                as Box<dyn std::error::Error + Send + Sync>
        })
}

fn is_pkcs8_key(key_data: &[u8]) -> bool {
    key_data.starts_with(b"-----BEGIN PRIVATE KEY-----")
}

fn convert_pkcs8_to_pkcs1(pkcs8_key: &[u8]) -> Result<Vec<u8>> {
    let pkey = PKey::private_key_from_pem(pkcs8_key).context("Failed to parse PKCS#8 key")?;
    let rsa = pkey.rsa().context("Failed to extract RSA key from PKey")?;
    let pkcs1_key = rsa
        .private_key_to_pem()
        .context("Failed to convert to PKCS#1")?;
    Ok(pkcs1_key)
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
        Ok(kubeconfig
            .contexts
            .into_iter()
            .map(|c| KubeContextInfo { name: c.name })
            .collect())
    } else if !contexts.is_empty() {
        Ok(contexts
            .into_iter()
            .map(|name| KubeContextInfo { name })
            .collect())
    } else {
        Err("Failed to retrieve kubeconfig".to_string())
    }
}

pub async fn list_all_namespaces(client: Client) -> Result<Vec<String>, anyhow::Error> {
    let namespaces: Api<Namespace> = Api::all(client);
    let namespace_list = namespaces.list(&ListParams::default()).await?;

    let namespace_names: Vec<String> = namespace_list
        .into_iter()
        .filter_map(|namespace| namespace.metadata.name)
        .collect();

    Ok(namespace_names)
}

pub async fn get_services_with_annotation(
    client: Client, namespace: &str, _: &str,
) -> Result<Vec<ServiceInfo>, Box<dyn std::error::Error>> {
    let services: Api<Service> = Api::namespaced(client, namespace);
    let lp = ListParams::default();

    let service_list = services.list(&lp).await?;

    let results: Vec<ServiceInfo> = service_list
        .into_iter()
        .filter_map(|service| {
            let service_name = service.metadata.name.clone()?;
            let annotations = service.metadata.annotations.clone()?;
            if annotations
                .get("kftray.app/enabled")
                .map_or(false, |v| v == "true")
            {
                let ports = extract_ports_from_service(&service);
                let annotations_hashmap: HashMap<String, String> =
                    annotations.into_iter().collect();
                Some((service_name, annotations_hashmap, ports))
            } else {
                None
            }
        })
        .collect();

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

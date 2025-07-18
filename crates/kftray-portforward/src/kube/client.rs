use std::collections::HashMap;
use std::env;
use std::error::Error;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use anyhow::{
    Context,
    Result,
};
use futures::future::select_ok;
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

use crate::kube::models::KubeContextInfo;

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
    env::remove_var("PYTHONHOME");
    env::remove_var("PYTHONPATH");

    let kubeconfig_paths = get_kubeconfig_paths_from_option(kubeconfig)?;
    let (merged_kubeconfig, all_contexts, mut errors) = merge_kubeconfigs(&kubeconfig_paths)?;

    if let Some(context_name) = context_name {
        match create_config_with_context(&merged_kubeconfig, context_name).await {
            Ok(config) => {
                if let Some(client) = create_client_with_config(&config).await {
                    return Ok((Some(client), Some(merged_kubeconfig), all_contexts));
                } else {
                    errors.push(format!(
                        "Failed to create HTTPS connector for context: {context_name}"
                    ));
                }
            }
            Err(e) => {
                errors.push(format!(
                    "Failed to create configuration for context: {context_name}: {e}"
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
            info!("Using provided kubeconfig paths: {path}");
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
        info!("Attempting to read kubeconfig from path: {path:?}");
        match Kubeconfig::read_from(path)
            .context(format!("Failed to read kubeconfig from {path:?}"))
        {
            Ok(kubeconfig) => {
                info!("Successfully read kubeconfig from {path:?}");
                let contexts = list_contexts(&kubeconfig);
                all_contexts.extend(contexts.clone());
                info!("Available contexts in {path:?}: {contexts:?}");
                merged_kubeconfig = merged_kubeconfig.merge(kubeconfig)?;
            }
            Err(e) => {
                let error_msg = format!("Failed to read kubeconfig from {path:?}: {e}");
                error!("{error_msg}");
                errors.push(error_msg);
            }
        }
    }

    Ok((merged_kubeconfig, all_contexts, errors))
}

async fn create_config_with_context(kubeconfig: &Kubeconfig, context_name: &str) -> Result<Config> {
    info!("Creating configuration for context: {context_name}");
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

    let context_to_use = if context_name == "current-context" {
        kubeconfig.current_context.clone()
    } else {
        Some(context_name.to_owned())
    };

    Config::from_custom_kubeconfig(
        kubeconfig,
        &KubeConfigOptions {
            context: context_to_use,
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
        .map(|(description, strategy)| Box::pin(try_create_client(description, strategy)))
        .collect();

    match select_ok(futures).await {
        Ok((client, _)) => Some(client),
        Err(_) => None,
    }
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

async fn try_create_client(description: &str, strategy: StrategyFuture<'_>) -> Result<Client, ()> {
    info!("Attempting to create client with {description}");
    match strategy.await {
        Ok(client) => {
            if test_client(&client).await.is_ok() {
                info!("Successfully created client with {description}");
                Ok(client)
            } else {
                warn!("{description} failed to connect.");
                Err(())
            }
        }
        Err(e) => {
            warn!("Failed to create {description}: {e}");
            Err(())
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
            warn!("Failed to get auth layer: {e}");
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
            warn!("Failed to get auth layer: {e}");
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

async fn create_insecure_http_client(
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

async fn test_client(client: &Client) -> Result<(), Box<dyn Error + Send + Sync>> {
    match client.apiserver_version().await {
        Ok(version) => {
            info!("Kubernetes API server version: {version:?}");
            Ok(())
        }
        Err(e) => {
            let err_msg = format!("Failed to get API server version: {e}");
            Err(Box::new(std::io::Error::other(err_msg)) as Box<dyn Error + Send + Sync>)
        }
    }
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
        .map_err(|err| format!("Failed to create client: {err}"))?;

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
                .is_some_and(|v| v == "true")
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn test_get_kubeconfig_paths_from_option() {
        use std::env;
        use std::fs;
        use std::sync::Mutex;

        use tempfile::TempDir;

        lazy_static::lazy_static! {
            static ref ENV_LOCK: Mutex<()> = Mutex::new(());
        }
        let _env_guard = ENV_LOCK.lock().unwrap();

        let original_kubeconfig = env::var("KUBECONFIG").ok();
        let original_home = env::var("HOME").ok();

        let temp_dir = TempDir::new().unwrap();

        let explicit_paths =
            get_kubeconfig_paths_from_option(Some("/path1:/path2".to_string())).unwrap();
        assert_eq!(explicit_paths.len(), 2);
        assert_eq!(explicit_paths[0], Path::new("/path1"));
        assert_eq!(explicit_paths[1], Path::new("/path2"));

        let mock_kubeconfig_path = temp_dir.path().join("mock_kubeconfig");
        fs::write(&mock_kubeconfig_path, "mock kubeconfig content").unwrap();

        env::set_var("KUBECONFIG", mock_kubeconfig_path.to_str().unwrap());

        let default_path_result = get_kubeconfig_paths_from_option(Some("default".to_string()));
        assert!(default_path_result.is_ok());
        let default_path = default_path_result.unwrap();
        assert!(!default_path.is_empty());
        assert_eq!(default_path[0], mock_kubeconfig_path);

        let none_path_result = get_kubeconfig_paths_from_option(None);
        assert!(none_path_result.is_ok());
        let none_path = none_path_result.unwrap();
        assert!(!none_path.is_empty());
        assert_eq!(none_path[0], mock_kubeconfig_path);

        let fake_home = temp_dir.path().join("fake_home");
        fs::create_dir_all(fake_home.join(".kube")).unwrap();
        let fake_kubeconfig = fake_home.join(".kube").join("config");
        fs::write(&fake_kubeconfig, "home dir kubeconfig content").unwrap();

        env::remove_var("KUBECONFIG");
        env::set_var("HOME", fake_home.to_str().unwrap());

        let home_fallback_result = get_kubeconfig_paths_from_option(None);
        assert!(home_fallback_result.is_ok());
        let home_fallback_path = home_fallback_result.unwrap();
        assert!(!home_fallback_path.is_empty());
        assert_eq!(home_fallback_path[0], fake_kubeconfig);

        let nonexistent_dir = temp_dir.path().join("nonexistent");
        env::set_var("HOME", nonexistent_dir.to_str().unwrap());
        env::set_var(
            "KUBECONFIG",
            temp_dir.path().join("nonexistent_file").to_str().unwrap(),
        );

        let error_result = get_kubeconfig_paths_from_option(None);
        assert!(error_result.is_err());

        match original_kubeconfig {
            Some(val) => env::set_var("KUBECONFIG", val),
            None => env::remove_var("KUBECONFIG"),
        }

        match original_home {
            Some(val) => env::set_var("HOME", val),
            None => env::remove_var("HOME"),
        }
    }

    #[test]
    fn test_merge_kubeconfigs_empty() {
        let (_config, contexts, errors) = merge_kubeconfigs(&[]).unwrap();
        assert!(contexts.is_empty());
        assert!(errors.is_empty());
    }

    #[test]
    fn test_merge_kubeconfigs_with_invalid_path() {
        let paths = vec![PathBuf::from("/invalid/path/that/should/not/exist")];
        let (_config, contexts, errors) = merge_kubeconfigs(&paths).unwrap();
        assert!(contexts.is_empty());
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_list_contexts() {
        let kubeconfig = Kubeconfig {
            contexts: vec![
                kube::config::NamedContext {
                    name: "context1".to_string(),
                    context: Some(kube::config::Context::default()),
                },
                kube::config::NamedContext {
                    name: "context2".to_string(),
                    context: Some(kube::config::Context::default()),
                },
            ],
            ..Default::default()
        };

        let contexts = list_contexts(&kubeconfig);
        assert_eq!(contexts.len(), 2);
        assert_eq!(contexts[0], "context1");
        assert_eq!(contexts[1], "context2");
    }

    #[test]
    fn test_is_pkcs8_key() {
        assert!(is_pkcs8_key(
            b"-----BEGIN PRIVATE KEY-----\ndata\n-----END PRIVATE KEY-----"
        ));
        assert!(!is_pkcs8_key(
            b"-----BEGIN RSA PRIVATE KEY-----\ndata\n-----END RSA PRIVATE KEY-----"
        ));
        assert!(!is_pkcs8_key(b"random data"));
    }

    #[test]
    fn test_config_ext_clone() {
        let mut config = Config::new("https://example.com".parse().unwrap());
        config.accept_invalid_certs = false;

        let cloned_config = config.clone_with_invalid_certs(true);
        assert!(cloned_config.accept_invalid_certs);

        let cloned_config_false = config.clone_with_invalid_certs(false);
        assert!(!cloned_config_false.accept_invalid_certs);
    }

    #[tokio::test]
    async fn test_create_config_with_context() {
        let mut kubeconfig = Kubeconfig::default();
        let context_name = "test-context";

        let named_context = kube::config::NamedContext {
            name: context_name.to_string(),
            context: Some(kube::config::Context::default()),
        };
        kubeconfig.contexts = vec![named_context];

        let result = create_config_with_context(&kubeconfig, context_name).await;
        assert!(result.is_err());
    }
    #[tokio::test]
    async fn test_list_kube_contexts_empty() {
        let result = list_kube_contexts(Some("invalid".to_string())).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_ports_from_service() {
        let mut service = k8s_openapi::api::core::v1::Service::default();

        let spec = k8s_openapi::api::core::v1::ServiceSpec {
            ports: Some(vec![
                k8s_openapi::api::core::v1::ServicePort {
                    name: Some("http".to_string()),
                    port: 80,
                    target_port: Some(IntOrString::Int(8080)),
                    ..Default::default()
                },
                k8s_openapi::api::core::v1::ServicePort {
                    name: Some("https".to_string()),
                    port: 443,
                    target_port: Some(IntOrString::Int(8443)),
                    ..Default::default()
                },
                k8s_openapi::api::core::v1::ServicePort {
                    name: Some("named-port".to_string()),
                    port: 9000,
                    target_port: Some(IntOrString::String("web".to_string())),
                    ..Default::default()
                },
                k8s_openapi::api::core::v1::ServicePort {
                    name: None,
                    port: 9090,
                    target_port: Some(IntOrString::Int(9090)),
                    ..Default::default()
                },
                k8s_openapi::api::core::v1::ServicePort {
                    name: Some("no-target".to_string()),
                    port: 8888,
                    target_port: None,
                    ..Default::default()
                },
            ]),
            ..Default::default()
        };

        service.spec = Some(spec.clone());

        let ports = extract_ports_from_service(&service);

        assert_eq!(ports.len(), 4);
        assert_eq!(ports.get("http"), Some(&8080));
        assert_eq!(ports.get("https"), Some(&8443));
        assert_eq!(ports.get("named-port"), Some(&0));
        assert_eq!(ports.get("9090"), Some(&9090));
        assert_eq!(ports.get("no-target"), None);

        service.spec = None;
        let ports = extract_ports_from_service(&service);
        assert!(ports.is_empty());
    }

    #[test]
    fn test_resolve_named_port() {
        let spec = k8s_openapi::api::core::v1::ServiceSpec {
            ports: Some(vec![
                k8s_openapi::api::core::v1::ServicePort {
                    name: Some("http".to_string()),
                    port: 80,
                    ..Default::default()
                },
                k8s_openapi::api::core::v1::ServicePort {
                    name: Some("https".to_string()),
                    port: 443,
                    ..Default::default()
                },
            ]),
            ..Default::default()
        };

        assert_eq!(resolve_named_port(&spec, "http"), Some(80));
        assert_eq!(resolve_named_port(&spec, "https"), Some(443));
        assert_eq!(resolve_named_port(&spec, "nonexistent"), None);

        let empty_spec = k8s_openapi::api::core::v1::ServiceSpec {
            ports: None,
            ..Default::default()
        };
        assert_eq!(resolve_named_port(&empty_spec, "http"), None);

        let spec_no_names = k8s_openapi::api::core::v1::ServiceSpec {
            ports: Some(vec![k8s_openapi::api::core::v1::ServicePort {
                name: None,
                port: 80,
                ..Default::default()
            }]),
            ..Default::default()
        };
        assert_eq!(resolve_named_port(&spec_no_names, "http"), None);
    }

    #[test]
    fn test_get_services_with_annotation_filter() {
        let mut service = k8s_openapi::api::core::v1::Service::default();
        let mut metadata = k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta::default();

        let mut annotations = std::collections::BTreeMap::new();
        annotations.insert("kftray.app/enabled".to_string(), "true".to_string());

        metadata.name = Some("test-service".to_string());
        metadata.annotations = Some(annotations);
        service.metadata = metadata;

        service.spec = Some(k8s_openapi::api::core::v1::ServiceSpec {
            ports: Some(vec![k8s_openapi::api::core::v1::ServicePort {
                name: Some("http".to_string()),
                port: 80,
                target_port: Some(IntOrString::Int(8080)),
                ..Default::default()
            }]),
            ..Default::default()
        });

        let ports = extract_ports_from_service(&service);
        assert_eq!(ports.len(), 1);
        assert_eq!(ports.get("http"), Some(&8080));
    }
}

use std::collections::HashMap;

use anyhow::Result;
use futures::stream::{
    self,
    StreamExt,
};
use kftray_commons::models::config_model::Config;
use kube::Client;
use log::{
    debug,
    error,
    info,
};

use crate::kube::client::error::{
    KubeClientError,
    KubeResult,
};
use crate::kube::operations::{
    get_services_with_annotation,
    list_all_namespaces,
};
use crate::kube::shared_client::{
    SHARED_CLIENT_MANAGER,
    ServiceClientKey,
};

const CONFIGS_ANNOTATION: &str = "kftray.app/configs";

pub async fn retrieve_service_configs(
    context: &str, kubeconfig: Option<String>, namespace: Option<&str>,
) -> Result<Vec<Config>, String> {
    let client_key = ServiceClientKey::new(Some(context.to_string()), kubeconfig.clone());

    let client = SHARED_CLIENT_MANAGER
        .get_connection(client_key)
        .await
        .map_err(|e| e.to_string())?;

    retrieve_service_configs_direct(context, kubeconfig.as_deref(), namespace, &client.client).await
}

async fn retrieve_service_configs_direct(
    context: &str, kubeconfig: Option<&str>, namespace: Option<&str>, client: &Client,
) -> Result<Vec<Config>, String> {
    if let Some(namespace) = namespace.map(str::trim).filter(|ns| !ns.is_empty()) {
        return namespace_configs(client, context, kubeconfig, namespace)
            .await
            .map_err(|e| services_error(namespace, &e));
    }

    let namespaces = match list_all_namespaces(Client::clone(client)).await {
        Ok(namespaces) => namespaces,
        Err(KubeClientError::KubeError(kube::Error::Api(status))) if status.code == 403 => {
            let namespace = client.default_namespace();
            info!(
                "Listing namespaces is forbidden, importing from the context namespace \
                 '{namespace}'"
            );
            return namespace_configs(client, context, kubeconfig, namespace)
                .await
                .map_err(|e| {
                    format!(
                        "{} (listing namespaces is forbidden, so only the context namespace \
                         was checked)",
                        services_error(namespace, &e)
                    )
                });
        }
        Err(e) => return Err(e.to_string()),
    };

    debug!(
        "Found {} namespaces for direct API discovery",
        namespaces.len()
    );

    let concurrency_limit = 10;

    let configs = stream::iter(namespaces)
        .map(|namespace| async move {
            info!("Processing namespace with direct API: {namespace}");
            namespace_configs(client, context, kubeconfig, &namespace)
                .await
                .map_err(|e| services_error(&namespace, &e))
        })
        .buffer_unordered(concurrency_limit)
        .fold(Vec::new(), |mut configs, result| async move {
            match result {
                Ok(mut namespace_configs) => configs.append(&mut namespace_configs),
                Err(e) => error!("Error processing namespace with direct API: {e}"),
            }
            configs
        })
        .await;

    Ok(configs)
}

async fn namespace_configs(
    client: &Client, context: &str, kubeconfig: Option<&str>, namespace: &str,
) -> KubeResult<Vec<Config>> {
    let services =
        get_services_with_annotation(Client::clone(client), namespace, CONFIGS_ANNOTATION).await?;

    let kubeconfig = kubeconfig.map(str::to_string);
    let mut configs = Vec::new();

    for (service_name, annotations, ports) in services {
        debug!("Processing service: {service_name} in namespace: {namespace}");
        if let Some(configs_str) = annotations.get(CONFIGS_ANNOTATION) {
            configs.extend(parse_configs(
                configs_str,
                context,
                namespace,
                &service_name,
                &ports,
                kubeconfig.clone(),
            ));
        } else {
            configs.extend(create_default_configs(
                context,
                namespace,
                &service_name,
                &ports,
                kubeconfig.clone(),
            ));
        }
    }

    Ok(configs)
}

fn services_error(namespace: &str, error: &KubeClientError) -> String {
    match error {
        KubeClientError::KubeError(kube::Error::Api(status)) => {
            format!(
                "cannot list services in namespace \"{namespace}\": {}",
                status.message
            )
        }
        _ => format!("cannot list services in namespace \"{namespace}\": {error}"),
    }
}

fn parse_configs(
    configs_str: &str, context: &str, namespace: &str, service_name: &str,
    ports: &HashMap<String, i32>, kubeconfig: Option<String>,
) -> Vec<Config> {
    configs_str
        .split(',')
        .filter_map(|config_str| {
            let config_str = config_str.trim();
            let Some((alias, local_port, remote_port)) = parse_config_entry(config_str, ports)
            else {
                debug!("Invalid config format: {config_str}");
                return None;
            };

            Some(Config {
                id: None,
                context: Some(context.to_string()),
                kubeconfig: kubeconfig.clone(),
                namespace: namespace.to_string(),
                service: Some(service_name.to_string()),
                alias: Some(alias.to_string()),
                local_port: Some(local_port),
                remote_port: Some(remote_port),
                protocol: "tcp".to_string(),
                workload_type: Some("service".to_string()),
                target: None,
                local_address: None,
                auto_loopback_address: false,
                remote_address: None,
                domain_enabled: None,
                http_logs_enabled: None,
                http_logs_max_file_size: None,
                http_logs_retention_days: None,
                http_logs_auto_cleanup: None,
                exposure_type: None,
                exposure_protocol: None,
                cert_manager_enabled: None,
                cert_issuer: None,
                cert_issuer_kind: None,
                ingress_class: None,
                ingress_annotations: None,
                tags: Default::default(),
            })
        })
        .collect()
}

fn parse_config_entry<'a>(
    entry: &'a str, ports: &HashMap<String, i32>,
) -> Option<(&'a str, u16, u16)> {
    entry.match_indices('-').rev().find_map(|(index, _)| {
        let (local_port, remote_port) = entry[index + 1..].split_once('-')?;
        let local_port = local_port.parse().ok()?;
        let remote_port = remote_port.parse().ok().or_else(|| {
            ports
                .get(remote_port)
                .and_then(|&port| u16::try_from(port).ok())
        })?;
        Some((&entry[..index], local_port, remote_port))
    })
}

fn create_default_configs(
    context: &str, namespace: &str, service_name: &str, ports: &HashMap<String, i32>,
    kubeconfig: Option<String>,
) -> Vec<Config> {
    ports
        .iter()
        .map(|(_port_name, &port)| Config {
            id: None,
            context: Some(context.to_string()),
            kubeconfig: kubeconfig.clone(),
            namespace: namespace.to_string(),
            service: Some(service_name.to_string()),
            alias: Some(service_name.to_string()),
            local_port: Some(port as u16),
            remote_port: Some(port as u16),
            protocol: "tcp".to_string(),
            workload_type: Some("service".to_string()),
            target: None,
            local_address: None,
            auto_loopback_address: false,
            remote_address: None,
            domain_enabled: None,
            http_logs_enabled: None,
            http_logs_max_file_size: None,
            http_logs_retention_days: None,
            http_logs_auto_cleanup: None,
            exposure_type: None,
            exposure_protocol: None,
            cert_manager_enabled: None,
            cert_issuer: None,
            cert_issuer_kind: None,
            ingress_class: None,
            ingress_annotations: None,
            tags: Default::default(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use http::{
        Request,
        Response,
    };
    use k8s_openapi::api::core::v1::{
        Service,
        ServicePort,
        ServiceSpec,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
    use kube::client::Body;
    use tower_test::mock;

    use super::*;

    type Handle = mock::Handle<Request<Body>, Response<Body>>;

    fn mock_client(default_namespace: &str) -> (Client, Handle) {
        let (service, handle) = mock::pair::<Request<Body>, Response<Body>>();
        (Client::new(service, default_namespace), handle)
    }

    async fn answer_services(handle: &mut Handle, namespace: &str) {
        let (request, send) = handle.next_request().await.unwrap();
        assert_eq!(
            request.uri().path(),
            format!("/api/v1/namespaces/{namespace}/services")
        );
        let service = Service {
            metadata: ObjectMeta {
                name: Some("web".to_string()),
                namespace: Some(namespace.to_string()),
                annotations: Some(
                    [
                        ("kftray.app/enabled".to_string(), "true".to_string()),
                        (CONFIGS_ANNOTATION.to_string(), "web-8080-80".to_string()),
                    ]
                    .into(),
                ),
                ..Default::default()
            },
            spec: Some(ServiceSpec {
                ports: Some(vec![ServicePort {
                    port: 80,
                    target_port: Some(IntOrString::Int(80)),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };
        let list = k8s_openapi::List::<Service> {
            items: vec![service],
            metadata: Default::default(),
        };
        send.send_response(
            Response::builder()
                .status(200)
                .body(Body::from(serde_json::to_vec(&list).unwrap()))
                .unwrap(),
        );
    }

    async fn answer_forbidden(handle: &mut Handle, path: &str, message: &str) {
        let (request, send) = handle.next_request().await.unwrap();
        assert_eq!(request.uri().path(), path);
        let status = serde_json::json!({
            "kind": "Status",
            "apiVersion": "v1",
            "status": "Failure",
            "message": message,
            "reason": "Forbidden",
            "code": 403
        });
        send.send_response(
            Response::builder()
                .status(403)
                .body(Body::from(serde_json::to_vec(&status).unwrap()))
                .unwrap(),
        );
    }

    const SERVICES_FORBIDDEN: &str = "services is forbidden: User \"dev\" cannot list resource \
                                      \"services\" in API group \"\" in the namespace \"team-a\"";

    #[tokio::test]
    async fn a_set_namespace_is_imported_without_listing_namespaces() {
        let (client, mut handle) = mock_client("default");
        let server = tokio::spawn(async move { answer_services(&mut handle, "team-a").await });

        let configs = retrieve_service_configs_direct("ctx", None, Some(" team-a "), &client)
            .await
            .unwrap();

        server.await.unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].namespace, "team-a");
        assert_eq!(configs[0].alias.as_deref(), Some("web"));
    }

    #[tokio::test]
    async fn a_forbidden_namespace_list_falls_back_to_the_context_namespace() {
        let (client, mut handle) = mock_client("team-a");
        let server = tokio::spawn(async move {
            answer_forbidden(&mut handle, "/api/v1/namespaces", "namespaces is forbidden").await;
            answer_services(&mut handle, "team-a").await;
        });

        let configs = retrieve_service_configs_direct("ctx", None, Some(""), &client)
            .await
            .unwrap();

        server.await.unwrap();
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].namespace, "team-a");
    }

    #[tokio::test]
    async fn a_forbidden_service_list_in_a_set_namespace_names_the_namespace() {
        let (client, mut handle) = mock_client("default");
        let server = tokio::spawn(async move {
            answer_forbidden(
                &mut handle,
                "/api/v1/namespaces/team-a/services",
                SERVICES_FORBIDDEN,
            )
            .await;
        });

        let error = retrieve_service_configs_direct("ctx", None, Some("team-a"), &client)
            .await
            .unwrap_err();

        server.await.unwrap();
        assert_eq!(
            error,
            format!("cannot list services in namespace \"team-a\": {SERVICES_FORBIDDEN}")
        );
    }

    #[tokio::test]
    async fn a_failed_fallback_says_only_the_context_namespace_was_checked() {
        let (client, mut handle) = mock_client("team-a");
        let server = tokio::spawn(async move {
            answer_forbidden(&mut handle, "/api/v1/namespaces", "namespaces is forbidden").await;
            answer_forbidden(
                &mut handle,
                "/api/v1/namespaces/team-a/services",
                SERVICES_FORBIDDEN,
            )
            .await;
        });

        let error = retrieve_service_configs_direct("ctx", None, None, &client)
            .await
            .unwrap_err();

        server.await.unwrap();
        assert_eq!(
            error,
            format!(
                "cannot list services in namespace \"team-a\": {SERVICES_FORBIDDEN} (listing \
                 namespaces is forbidden, so only the context namespace was checked)"
            )
        );
    }

    #[test]
    fn test_parse_configs() {
        let mut ports = HashMap::new();
        ports.insert("http".to_string(), 8080);
        ports.insert("https".to_string(), 8443);
        ports.insert("grpc".to_string(), 9090);

        let configs_str = "web-3000-8080,api-3001-http,admin-3002-grpc";
        let configs = parse_configs(
            configs_str,
            "test-context",
            "test-namespace",
            "test-service",
            &ports,
            Some("/path/to/config".to_string()),
        );

        assert_eq!(configs.len(), 3);

        let web_config = &configs[0];
        assert_eq!(web_config.context, Some("test-context".to_string()));
        assert_eq!(web_config.namespace, "test-namespace");
        assert_eq!(web_config.service, Some("test-service".to_string()));
        assert_eq!(web_config.alias, Some("web".to_string()));
        assert_eq!(web_config.local_port, Some(3000));
        assert_eq!(web_config.remote_port, Some(8080));
        assert_eq!(web_config.protocol, "tcp");
        assert_eq!(web_config.kubeconfig, Some("/path/to/config".to_string()));
        assert_eq!(web_config.workload_type, Some("service".to_string()));

        let api_config = &configs[1];
        assert_eq!(api_config.alias, Some("api".to_string()));
        assert_eq!(api_config.local_port, Some(3001));
        assert_eq!(api_config.remote_port, Some(8080));

        let admin_config = &configs[2];
        assert_eq!(admin_config.alias, Some("admin".to_string()));
        assert_eq!(admin_config.local_port, Some(3002));
        assert_eq!(admin_config.remote_port, Some(9090));
    }

    #[test]
    fn test_parse_configs_invalid_format() {
        let mut ports = HashMap::new();
        ports.insert("http".to_string(), 8080);

        let configs_str = "invalid-format,web-3000-8080,missing-parts";
        let configs = parse_configs(
            configs_str,
            "test-context",
            "test-namespace",
            "test-service",
            &ports,
            None,
        );

        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].alias, Some("web".to_string()));
        assert_eq!(configs[0].local_port, Some(3000));
        assert_eq!(configs[0].remote_port, Some(8080));
    }

    #[test]
    fn test_parse_configs_invalid_ports() {
        let ports = HashMap::new();

        let configs_str = "web-invalid-8080,api-3001-unknown";
        let configs = parse_configs(
            configs_str,
            "test-context",
            "test-namespace",
            "test-service",
            &ports,
            None,
        );

        assert_eq!(configs.len(), 0);
    }

    #[test]
    fn test_parse_configs_hyphenated_alias() {
        let ports = HashMap::new();

        let configs = parse_configs(
            "my-api-8080-9090",
            "test-context",
            "test-namespace",
            "test-service",
            &ports,
            None,
        );

        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].alias, Some("my-api".to_string()));
        assert_eq!(configs[0].local_port, Some(8080));
        assert_eq!(configs[0].remote_port, Some(9090));
    }

    #[test]
    fn test_parse_configs_hyphenated_port_name() {
        let mut ports = HashMap::new();
        ports.insert("http-alt".to_string(), 8081);

        let configs = parse_configs(
            "api-8080-http-alt,my-api-8082-http-alt",
            "test-context",
            "test-namespace",
            "test-service",
            &ports,
            None,
        );

        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].alias, Some("api".to_string()));
        assert_eq!(configs[0].local_port, Some(8080));
        assert_eq!(configs[0].remote_port, Some(8081));
        assert_eq!(configs[1].alias, Some("my-api".to_string()));
        assert_eq!(configs[1].local_port, Some(8082));
        assert_eq!(configs[1].remote_port, Some(8081));
    }

    #[test]
    fn test_parse_configs_out_of_range_remote_port() {
        let ports = HashMap::new();

        let configs = parse_configs(
            "api-3000-70000",
            "test-context",
            "test-namespace",
            "test-service",
            &ports,
            None,
        );

        assert!(configs.is_empty());
    }

    #[test]
    fn test_create_default_configs() {
        let mut ports = HashMap::new();
        ports.insert("http".to_string(), 8080);
        ports.insert("https".to_string(), 8443);

        let configs = create_default_configs(
            "test-context",
            "test-namespace",
            "test-service",
            &ports,
            Some("/path/to/config".to_string()),
        );

        assert_eq!(configs.len(), 2);

        let http_config = configs
            .iter()
            .find(|c| c.remote_port == Some(8080))
            .unwrap();
        assert_eq!(http_config.context, Some("test-context".to_string()));
        assert_eq!(http_config.namespace, "test-namespace");
        assert_eq!(http_config.service, Some("test-service".to_string()));
        assert_eq!(http_config.alias, Some("test-service".to_string()));
        assert_eq!(http_config.local_port, Some(8080));
        assert_eq!(http_config.protocol, "tcp");
        assert_eq!(http_config.kubeconfig, Some("/path/to/config".to_string()));
        assert_eq!(http_config.workload_type, Some("service".to_string()));

        let https_config = configs
            .iter()
            .find(|c| c.remote_port == Some(8443))
            .unwrap();
        assert_eq!(https_config.local_port, Some(8443));
    }

    #[test]
    fn test_create_default_configs_empty_ports() {
        let ports = HashMap::new();
        let configs = create_default_configs(
            "test-context",
            "test-namespace",
            "test-service",
            &ports,
            None,
        );
        assert!(configs.is_empty());
    }
}

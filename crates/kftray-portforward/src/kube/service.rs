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

use crate::kube::operations::{
    get_services_with_annotation,
    list_all_namespaces,
};
use crate::kube::shared_client::{
    SHARED_CLIENT_MANAGER,
    ServiceClientKey,
};
pub async fn retrieve_service_configs(
    context: &str, kubeconfig: Option<String>,
) -> Result<Vec<Config>, String> {
    let client_key = ServiceClientKey::new(Some(context.to_string()), kubeconfig.clone());

    let client = SHARED_CLIENT_MANAGER
        .get_connection(client_key)
        .await
        .map_err(|e| e.to_string())?;

    retrieve_service_configs_direct(context, kubeconfig, &client.client).await
}

async fn retrieve_service_configs_direct(
    context: &str, kubeconfig: Option<String>, client: &Client,
) -> Result<Vec<Config>, String> {
    let annotation = "kftray.app/configs";

    let namespaces = list_all_namespaces(Client::clone(client))
        .await
        .map_err(|e| e.to_string())?;

    debug!(
        "Found {} namespaces for direct API discovery",
        namespaces.len()
    );

    let concurrency_limit = 10;

    stream::iter(namespaces)
        .map(|namespace| {
            let client = Client::clone(client);
            let context = context.to_string();
            let kubeconfig = kubeconfig.clone();
            let annotation = annotation.to_string();

            async move {
                info!("Processing namespace with direct API: {namespace}");
                let services =
                    get_services_with_annotation(client.clone(), &namespace, &annotation)
                        .await
                        .map_err(|e| e.to_string())?;

                let mut namespace_configs = Vec::new();

                for (service_name, annotations, ports) in services {
                    debug!("Processing service: {service_name} in namespace: {namespace}");
                    if let Some(configs_str) = annotations.get(&annotation) {
                        namespace_configs.extend(parse_configs(
                            configs_str,
                            &context,
                            &namespace,
                            &service_name,
                            &ports,
                            kubeconfig.clone(),
                        ));
                    } else {
                        namespace_configs.extend(create_default_configs(
                            &context,
                            &namespace,
                            &service_name,
                            &ports,
                            kubeconfig.clone(),
                        ));
                    }
                }

                Ok(namespace_configs)
            }
        })
        .buffer_unordered(concurrency_limit)
        .fold(
            Ok(Vec::new()),
            |mut acc: Result<Vec<Config>, String>, result: Result<Vec<Config>, String>| async {
                match (&mut acc, result) {
                    (Ok(configs), Ok(mut namespace_configs)) => {
                        configs.append(&mut namespace_configs);
                        acc
                    }
                    (Ok(_), Err(e)) => {
                        error!("Error processing namespace with direct API: {e}");
                        acc
                    }
                    (Err(_), _) => acc,
                }
            },
        )
        .await
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
    use super::*;

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

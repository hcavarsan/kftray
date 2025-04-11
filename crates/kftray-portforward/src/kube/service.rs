use std::collections::HashMap;

use futures::stream::{
    self,
    StreamExt,
};
use kftray_commons::models::config_model::Config;
use log::{
    debug,
    error,
    info,
};

use crate::create_client_with_specific_context;
use crate::get_services_with_annotation;
use crate::list_all_namespaces;
pub async fn retrieve_service_configs(
    context: &str, kubeconfig: Option<String>,
) -> Result<Vec<Config>, String> {
    let (client_opt, _, _) = create_client_with_specific_context(kubeconfig.clone(), Some(context))
        .await
        .map_err(|e| e.to_string())?;

    let client = client_opt.ok_or_else(|| "Client not created".to_string())?;
    let annotation = "kftray.app/configs";

    let namespaces = list_all_namespaces(client.clone())
        .await
        .map_err(|e| e.to_string())?;

    debug!("Found {} namespaces", namespaces.len());

    let concurrency_limit = 10;

    stream::iter(namespaces)
        .map(|namespace| {
            let client = client.clone();
            let context = context.to_string();
            let kubeconfig = kubeconfig.clone();
            let annotation = annotation.to_string();

            async move {
                info!("Processing namespace: {}", namespace);
                let services =
                    get_services_with_annotation(client.clone(), &namespace, &annotation)
                        .await
                        .map_err(|e| e.to_string())?;

                let mut namespace_configs = Vec::new();

                for (service_name, annotations, ports) in services {
                    debug!(
                        "Processing service: {} in namespace: {}",
                        service_name, namespace
                    );
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
                        error!("Error processing namespace: {}", e);
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
            let parts: Vec<&str> = config_str.trim().split('-').collect();
            if parts.len() != 3 {
                debug!("Invalid config format: {}", config_str);
                return None;
            }

            let alias = parts[0].to_string();
            let local_port: u16 = match parts[1].parse() {
                Ok(port) => port,
                Err(e) => {
                    debug!("Failed to parse local port '{}': {}", parts[1], e);
                    return None;
                }
            };

            let target_port = parts[2]
                .parse()
                .ok()
                .or_else(|| ports.get(parts[2]).cloned())?;

            Some(Config {
                id: None,
                context: context.to_string(),
                kubeconfig: kubeconfig.clone(),
                namespace: namespace.to_string(),
                service: Some(service_name.to_string()),
                alias: Some(alias),
                local_port: Some(local_port),
                remote_port: Some(target_port as u16),
                protocol: "tcp".to_string(),
                workload_type: Some("service".to_string()),
                target: None,
                local_address: None,
                remote_address: None,
                domain_enabled: None,
            })
        })
        .collect()
}

fn create_default_configs(
    context: &str, namespace: &str, service_name: &str, ports: &HashMap<String, i32>,
    kubeconfig: Option<String>,
) -> Vec<Config> {
    ports
        .iter()
        .map(|(_port_name, &port)| Config {
            id: None,
            context: context.to_string(),
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
            remote_address: None,
            domain_enabled: None,
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
        assert_eq!(web_config.context, "test-context");
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
        assert_eq!(http_config.context, "test-context");
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

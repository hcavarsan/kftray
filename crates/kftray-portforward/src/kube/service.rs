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

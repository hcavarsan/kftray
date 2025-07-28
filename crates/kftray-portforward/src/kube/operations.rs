use std::collections::HashMap;

use k8s_openapi::api::core::v1::{
    Namespace,
    Service,
    ServiceSpec,
};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::api::ListParams;
use kube::config::Kubeconfig;
use kube::{
    Api,
    Client,
};
use log::info;

use super::client::create_client_with_specific_context;
use super::client::error::{
    KubeClientError,
    KubeResult,
};
use crate::kube::models::KubeContextInfo;

pub type ServiceInfo = (String, HashMap<String, String>, HashMap<String, i32>);

pub async fn list_all_namespaces(client: Client) -> KubeResult<Vec<String>> {
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
) -> KubeResult<Vec<ServiceInfo>> {
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

pub fn extract_ports_from_service(service: &Service) -> HashMap<String, i32> {
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

pub fn list_contexts(kubeconfig: &Kubeconfig) -> Vec<String> {
    kubeconfig
        .contexts
        .iter()
        .map(|context| context.name.clone())
        .collect()
}

pub async fn list_kube_contexts(kubeconfig: Option<String>) -> KubeResult<Vec<KubeContextInfo>> {
    info!("list_kube_contexts {}", kubeconfig.as_deref().unwrap_or(""));

    let (_, kubeconfig, contexts) = create_client_with_specific_context(kubeconfig, None)
        .await
        .map_err(|err| {
            KubeClientError::config_error(format!("Failed to read kubeconfig contexts: {err}"))
        })?;

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
        Err(KubeClientError::config_error("No kubeconfig found or no contexts available. Please check your kubeconfig file exists and contains valid contexts"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[tokio::test]
    async fn test_list_kube_contexts_empty() {
        let result = list_kube_contexts(Some("invalid".to_string())).await;
        assert!(result.is_err());
    }
}

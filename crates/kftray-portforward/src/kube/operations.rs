use std::collections::{
    HashMap,
    HashSet,
};

use k8s_openapi::api::core::v1::{
    Namespace,
    Pod,
    Service,
};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::api::ListParams;
use kube::config::Kubeconfig;
use kube::{
    Api,
    Client,
};
use log::{
    info,
    warn,
};

use super::client::error::{
    KubeClientError,
    KubeResult,
};
use super::client::get_kubeconfig_paths_from_option;
use crate::kube::models::KubeContextInfo;
use crate::kube::target::service_label_selector;

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
    let services: Api<Service> = Api::namespaced(client.clone(), namespace);
    let pods: Api<Pod> = Api::namespaced(client, namespace);
    let lp = ListParams::default();

    let service_list = services.list(&lp).await?;

    let mut results = Vec::new();
    for service in service_list {
        let Some(service_name) = service.metadata.name.clone() else {
            continue;
        };
        let Some(annotations) = service.metadata.annotations.clone() else {
            continue;
        };
        if !annotations
            .get("kftray.app/enabled")
            .is_some_and(|v| v == "true")
        {
            continue;
        }

        let selected_pods = list_pods_for_named_target_ports(&pods, &service).await?;
        let ports = extract_ports_from_service(&service, &selected_pods);
        let annotations_hashmap: HashMap<String, String> = annotations.into_iter().collect();
        results.push((service_name, annotations_hashmap, ports));
    }

    Ok(results)
}

async fn list_pods_for_named_target_ports(
    pods: &Api<Pod>, service: &Service,
) -> KubeResult<Vec<Pod>> {
    let Some(spec) = service.spec.as_ref() else {
        return Ok(Vec::new());
    };
    let has_named_target_port = spec
        .ports
        .iter()
        .flatten()
        .any(|port| matches!(port.target_port, Some(IntOrString::String(_))));
    if !has_named_target_port {
        return Ok(Vec::new());
    }
    let Some(selector) = spec
        .selector
        .as_ref()
        .filter(|selector| !selector.is_empty())
    else {
        return Ok(Vec::new());
    };

    let lp = ListParams::default().labels(&service_label_selector(selector));
    Ok(pods.list(&lp).await?.items)
}

pub fn extract_ports_from_service(service: &Service, pods: &[Pod]) -> HashMap<String, i32> {
    let mut ports = HashMap::new();
    if let Some(spec) = &service.spec {
        for port in spec.ports.as_ref().unwrap_or(&vec![]) {
            let port_number = match &port.target_port {
                Some(IntOrString::Int(number)) => *number,
                Some(IntOrString::String(name)) => match resolve_named_port(pods, name) {
                    Some(number) => number,
                    None => {
                        warn!(
                            "Skipping port '{}' of service '{}': no selected pod declares a \
                             container port named '{name}'",
                            port.name.as_deref().unwrap_or_default(),
                            service.metadata.name.as_deref().unwrap_or_default(),
                        );
                        continue;
                    }
                },
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

fn resolve_named_port(pods: &[Pod], name: &str) -> Option<i32> {
    pods.iter()
        .filter_map(|pod| pod.spec.as_ref())
        .flat_map(|spec| &spec.containers)
        .filter_map(|container| container.ports.as_ref())
        .flatten()
        .find(|port| port.name.as_deref() == Some(name))
        .map(|port| port.container_port)
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

    let contexts = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<String>> {
        let paths = get_kubeconfig_paths_from_option(kubeconfig)?;
        // Each readable kubeconfig contributes its own contexts directly,
        // rather than going through a single merged `Kubeconfig`: a name
        // collision or other merge failure in one file would otherwise drop
        // every context from that file, not just the conflicting one.
        let mut contexts = Vec::new();
        let mut seen = HashSet::new();
        let mut errors = Vec::new();
        for path in &paths {
            match Kubeconfig::read_from(path) {
                Ok(parsed) => {
                    for name in list_contexts(&parsed) {
                        if seen.insert(name.clone()) {
                            contexts.push(name);
                        }
                    }
                }
                Err(e) => errors.push(format!("Failed to read kubeconfig from {path:?}: {e}")),
            }
        }
        if contexts.is_empty() && !errors.is_empty() {
            anyhow::bail!(errors.join("\n"));
        }
        if !errors.is_empty() {
            log::warn!(
                "Some kubeconfig paths failed to load and were skipped: {}",
                errors.join("; ")
            );
        }
        Ok(contexts)
    })
    .await
    .map_err(|err| KubeClientError::config_error(format!("Kubeconfig loading task failed: {err}")))?
    .map_err(|err| {
        KubeClientError::config_error(format!("Failed to read kubeconfig contexts: {err}"))
    })?;

    if contexts.is_empty() {
        return Err(KubeClientError::config_error(
            "No kubeconfig found or no contexts available. Please check your kubeconfig file exists and contains valid contexts",
        ));
    }

    Ok(contexts
        .into_iter()
        .map(|name| KubeContextInfo { name })
        .collect())
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
                    name: Some("unresolved".to_string()),
                    port: 9001,
                    target_port: Some(IntOrString::String("named-port".to_string())),
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

        let pods = vec![
            k8s_openapi::api::core::v1::Pod {
                spec: Some(k8s_openapi::api::core::v1::PodSpec {
                    containers: vec![k8s_openapi::api::core::v1::Container {
                        name: "sidecar".to_string(),
                        ports: None,
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            },
            k8s_openapi::api::core::v1::Pod {
                spec: Some(k8s_openapi::api::core::v1::PodSpec {
                    containers: vec![k8s_openapi::api::core::v1::Container {
                        name: "app".to_string(),
                        ports: Some(vec![k8s_openapi::api::core::v1::ContainerPort {
                            name: Some("web".to_string()),
                            container_port: 3000,
                            ..Default::default()
                        }]),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            },
        ];

        let ports = extract_ports_from_service(&service, &pods);

        assert_eq!(ports.len(), 4);
        assert_eq!(ports.get("http"), Some(&8080));
        assert_eq!(ports.get("https"), Some(&8443));
        assert_eq!(ports.get("named-port"), Some(&3000));
        assert_eq!(ports.get("unresolved"), None);
        assert_eq!(ports.get("9090"), Some(&9090));
        assert_eq!(ports.get("no-target"), None);

        let ports = extract_ports_from_service(&service, &[]);
        assert_eq!(ports.get("named-port"), None);
        assert_eq!(ports.get("http"), Some(&8080));

        service.spec = None;
        let ports = extract_ports_from_service(&service, &pods);
        assert!(ports.is_empty());
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

        let ports = extract_ports_from_service(&service, &[]);
        assert_eq!(ports.len(), 1);
        assert_eq!(ports.get("http"), Some(&8080));
    }

    #[tokio::test]
    async fn test_get_services_with_annotation_resolves_named_target_port_from_pods() {
        use http::{
            Request,
            Response,
        };
        use k8s_openapi::api::core::v1::{
            Container,
            ContainerPort,
            Pod,
            PodSpec,
            ServicePort,
            ServiceSpec,
        };
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
        use k8s_openapi::{
            List,
            ListableResource,
        };
        use kube::client::Body;
        use tower_test::mock;

        fn list_body<K: ListableResource + serde::Serialize>(items: Vec<K>) -> Body {
            let list = List::<K> {
                items,
                metadata: Default::default(),
            };
            Body::from(serde_json::to_vec(&list).unwrap())
        }

        let (mock_service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
        let client = Client::new(mock_service, "default");

        let server = tokio::spawn(async move {
            let (request, send) = handle.next_request().await.unwrap();
            assert!(request.uri().path().ends_with("/namespaces/apps/services"));

            let service = Service {
                metadata: ObjectMeta {
                    name: Some("web".to_string()),
                    namespace: Some("apps".to_string()),
                    annotations: Some(
                        [("kftray.app/enabled".to_string(), "true".to_string())].into(),
                    ),
                    ..Default::default()
                },
                spec: Some(ServiceSpec {
                    selector: Some([("app".to_string(), "web".to_string())].into()),
                    ports: Some(vec![
                        ServicePort {
                            name: Some("http".to_string()),
                            port: 80,
                            target_port: Some(IntOrString::String("http".to_string())),
                            ..Default::default()
                        },
                        ServicePort {
                            name: Some("metrics".to_string()),
                            port: 9100,
                            target_port: Some(IntOrString::String("metrics".to_string())),
                            ..Default::default()
                        },
                    ]),
                    ..Default::default()
                }),
                ..Default::default()
            };
            send.send_response(
                Response::builder()
                    .status(200)
                    .body(list_body(vec![service]))
                    .unwrap(),
            );

            let (request, send) = handle.next_request().await.unwrap();
            assert!(request.uri().path().ends_with("/namespaces/apps/pods"));
            assert!(
                request
                    .uri()
                    .query()
                    .is_some_and(|query| query.contains("labelSelector=app%3Dweb")),
                "{:?}",
                request.uri().query()
            );

            let pod = Pod {
                metadata: ObjectMeta {
                    name: Some("web-0".to_string()),
                    ..Default::default()
                },
                spec: Some(PodSpec {
                    containers: vec![Container {
                        name: "web".to_string(),
                        ports: Some(vec![ContainerPort {
                            name: Some("http".to_string()),
                            container_port: 8080,
                            ..Default::default()
                        }]),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            };
            send.send_response(
                Response::builder()
                    .status(200)
                    .body(list_body(vec![pod]))
                    .unwrap(),
            );
        });

        let services = get_services_with_annotation(client, "apps", "kftray.app/configs")
            .await
            .unwrap();

        assert_eq!(services.len(), 1);
        let (name, _, ports) = &services[0];
        assert_eq!(name, "web");
        assert_eq!(ports.get("http"), Some(&8080));
        assert_eq!(ports.get("metrics"), None);
        server.await.unwrap();
    }

    #[test]
    fn test_list_contexts() {
        let kubeconfig = Kubeconfig {
            contexts: vec![
                kube::config::NamedContext {
                    name: "context1".to_string(),
                    context: Some(kube::config::Context::default()),
                    ..Default::default()
                },
                kube::config::NamedContext {
                    name: "context2".to_string(),
                    context: Some(kube::config::Context::default()),
                    ..Default::default()
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

    #[tokio::test]
    async fn test_list_kube_contexts_preserves_load_error() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let kubeconfig_path = temp_dir.path().join("kubeconfig");
        std::fs::write(&kubeconfig_path, "not: valid: yaml: [").unwrap();

        let result = list_kube_contexts(Some(kubeconfig_path.to_string_lossy().to_string())).await;

        let err = result
            .err()
            .expect("malformed kubeconfig must fail")
            .to_string();
        assert!(
            err.contains(
                kubeconfig_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .expect("kubeconfig path must have a file name")
            ),
            "{err}"
        );
    }

    #[tokio::test]
    async fn test_list_kube_contexts_keeps_other_files_contexts_when_one_fails_to_merge() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let path_a = temp_dir.path().join("kubeconfig-a");
        let path_b = temp_dir.path().join("kubeconfig-b");

        std::fs::write(
            &path_a,
            "apiVersion: v1\n\
             kind: Config\n\
             current-context: context-a\n\
             contexts:\n\
             - name: context-a\n  \
               context:\n    \
                 cluster: cluster-a\n    \
                 user: user-a\n\
             clusters:\n\
             - name: cluster-a\n  \
               cluster:\n    \
                 server: https://127.0.0.1:1\n\
             users:\n\
             - name: user-a\n  \
               user: {}\n",
        )
        .unwrap();

        // A different apiVersion makes `Kubeconfig::merge` fail with
        // `ApiVersionMismatch` when combining this file with `path_a`; with
        // the old whole-object merge, that error dropped every context from
        // this file, not just the conflicting field.
        std::fs::write(
            &path_b,
            "apiVersion: v1beta1\n\
             kind: Config\n\
             current-context: context-b\n\
             contexts:\n\
             - name: context-b\n  \
               context:\n    \
                 cluster: cluster-b\n    \
                 user: user-b\n\
             clusters:\n\
             - name: cluster-b\n  \
               cluster:\n    \
                 server: https://127.0.0.1:1\n\
             users:\n\
             - name: user-b\n  \
               user: {}\n",
        )
        .unwrap();

        let separator = if cfg!(windows) { ';' } else { ':' };
        let combined = format!(
            "{}{separator}{}",
            path_a.to_string_lossy(),
            path_b.to_string_lossy()
        );

        let result = list_kube_contexts(Some(combined))
            .await
            .expect("both files parse individually and must contribute their contexts");
        let names: Vec<_> = result.into_iter().map(|ctx| ctx.name).collect();

        assert!(
            names.contains(&"context-a".to_string()),
            "expected context-a in {names:?}"
        );
        assert!(
            names.contains(&"context-b".to_string()),
            "a merge failure between the two files must not hide context-b's own context: \
             {names:?}"
        );
    }
}

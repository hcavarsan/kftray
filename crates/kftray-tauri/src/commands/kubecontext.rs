use std::collections::HashSet;

use anyhow::Result;
use k8s_openapi::api::core::v1::{
    Namespace,
    Pod,
    Service,
};
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kftray_commons::config_model::Config;
use kftray_portforward::kube::client::create_client_with_specific_context;
use kftray_portforward::kube::models::{
    KubeContextInfo,
    KubeNamespaceInfo,
    KubeServiceInfo,
    KubeServicePortInfo,
    PodInfo,
};
use kftray_portforward::kube::retrieve_service_configs;
use kube::Resource;
use kube::{
    ResourceExt,
    api::{
        Api,
        ListParams,
    },
};
use log::info;

#[tauri::command]
pub async fn list_kube_contexts(
    kubeconfig: Option<String>,
) -> Result<Vec<KubeContextInfo>, String> {
    kftray_portforward::list_kube_contexts(kubeconfig)
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub async fn list_pods(
    context_name: &str, namespace: &str, kubeconfig: Option<String>,
) -> Result<Vec<PodInfo>, String> {
    if namespace.trim().is_empty() {
        return Err("Namespace parameter cannot be empty".to_string());
    }

    let connection = create_client_with_specific_context(kubeconfig, context_name)
        .await
        .map_err(|err| format!("Failed to create client for context '{context_name}': {err}"))?;

    let api: Api<Pod> = Api::namespaced(connection.client, namespace);

    let pod_list = api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?;

    let unique_labels: HashSet<String> = pod_list
        .iter()
        .filter_map(|pod| pod.meta().labels.as_ref())
        .flat_map(|labels| labels.iter().map(|(key, value)| format!("{key}={value}")))
        .collect();

    let label_infos = unique_labels
        .into_iter()
        .map(|label_str| PodInfo {
            labels_str: label_str,
        })
        .collect();

    Ok(label_infos)
}

#[tauri::command]
pub async fn list_namespaces(
    context_name: &str, kubeconfig: Option<String>,
) -> Result<Vec<KubeNamespaceInfo>, String> {
    let connection = create_client_with_specific_context(kubeconfig, context_name)
        .await
        .map_err(|err| format!("Failed to create client for context '{context_name}': {err}"))?;

    let api: Api<Namespace> = Api::all(connection.client);

    let ns_list = api
        .list(&ListParams::default())
        .await
        .map_err(|e| format!("Failed to list namespaces: {e}"))?
        .iter()
        .map(|ns| KubeNamespaceInfo {
            name: ns.name_any(),
        })
        .collect();

    Ok(ns_list)
}

#[tauri::command]
pub async fn list_services(
    context_name: &str, namespace: &str, kubeconfig: Option<String>,
) -> Result<Vec<KubeServiceInfo>, String> {
    if namespace.trim().is_empty() {
        return Err("Namespace parameter cannot be empty".to_string());
    }

    let connection = create_client_with_specific_context(kubeconfig, context_name)
        .await
        .map_err(|err| format!("Failed to create client for context '{context_name}': {err}"))?;

    let api: Api<Service> = Api::namespaced(connection.client, namespace);

    let svc_list = api
        .list(&ListParams::default())
        .await
        .map_err(|e| e.to_string())?
        .iter()
        .map(|svc| KubeServiceInfo {
            name: svc.name_any(),
        })
        .collect();

    Ok(svc_list)
}

#[tauri::command]
pub async fn list_ports(
    context_name: &str, namespace: &str, service_name: &str, kubeconfig: Option<String>,
) -> Result<Vec<KubeServicePortInfo>, String> {
    let connection = create_client_with_specific_context(kubeconfig, context_name)
        .await
        .map_err(|err| format!("Failed to create client for context '{context_name}': {err}"))?;

    let api_svc: Api<Service> = Api::namespaced(connection.client.clone(), namespace);
    let api_pod: Api<Pod> = Api::namespaced(connection.client, namespace);

    match api_svc.get(service_name).await {
        Ok(service) => {
            let mut service_port_infos = Vec::new();

            if let Some(spec) = service.spec
                && let Some(service_ports) = spec.ports
            {
                for sp in service_ports {
                    if let Some(IntOrString::String(ref name)) = sp.target_port {
                        let selector_string =
                            spec.selector.as_ref().map_or_else(String::new, |s| {
                                s.iter()
                                    .map(|(key, value)| format!("{key}={value}"))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            });

                        let pods = api_pod
                            .list(&ListParams::default().labels(&selector_string))
                            .await
                            .map_err(|e| format!("Failed to list pods: {e}"))?;

                        'port_search: for pod in pods {
                            if let Some(spec) = &pod.spec {
                                for container in &spec.containers {
                                    if let Some(ports) = &container.ports {
                                        for cp in ports {
                                            if cp.name.as_deref() == Some(name) {
                                                service_port_infos.push(KubeServicePortInfo {
                                                    name: cp.name.clone(),
                                                    port: Some(IntOrString::Int(cp.container_port)),
                                                });
                                                break 'port_search;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else if let Some(IntOrString::Int(port)) = sp.target_port {
                        service_port_infos.push(KubeServicePortInfo {
                            name: sp.name,
                            port: Some(IntOrString::Int(port)),
                        });
                    }
                }
            }

            if service_port_infos.is_empty() {
                Err(format!(
                    "No ports found for service '{service_name}' in namespace '{namespace}'"
                ))
            } else {
                Ok(service_port_infos)
            }
        }
        Err(_) => {
            let pods = api_pod
                .list(&ListParams::default().labels(service_name))
                .await
                .map_err(|e| format!("Failed to list pods: {e}"))?;

            let mut seen = HashSet::new();
            let pod_port_infos: Vec<KubeServicePortInfo> = pods
                .iter()
                .filter_map(|pod| pod.spec.as_ref())
                .flat_map(|spec| spec.containers.iter())
                .filter_map(|container| container.ports.as_ref())
                .flat_map(|ports| ports.iter())
                .filter(|cp| seen.insert((cp.name.as_deref(), cp.container_port)))
                .map(|cp| KubeServicePortInfo {
                    name: cp.name.clone(),
                    port: Some(IntOrString::Int(cp.container_port)),
                })
                .collect();

            if pod_port_infos.is_empty() {
                Err(format!(
                    "No ports found for label '{service_name}' in namespace '{namespace}'"
                ))
            } else {
                Ok(pod_port_infos)
            }
        }
    }
}

#[tauri::command]
pub async fn get_services_with_annotations(
    context_name: String, kubeconfig_path: Option<String>,
) -> Result<Vec<Config>, String> {
    info!(
        "get_services_with_annotations called with context: '{context_name}' and kubeconfig: {kubeconfig_path:?}"
    );

    retrieve_service_configs(&context_name, kubeconfig_path).await
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use k8s_openapi::api::core::v1::{
        Container,
        ContainerPort,
        Namespace as K8sNamespace,
        NamespaceSpec,
        Pod as K8sPod,
        PodSpec,
        Service as K8sService,
        ServicePort,
        ServiceSpec,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    use super::*;

    struct MockKubeClient {
        namespaces: Vec<K8sNamespace>,
        pods: BTreeMap<String, Vec<K8sPod>>,
        services: BTreeMap<String, Vec<K8sService>>,
        contexts: Vec<String>,
    }

    impl MockKubeClient {
        fn new() -> Self {
            Self {
                namespaces: Vec::new(),
                pods: BTreeMap::new(),
                services: BTreeMap::new(),
                contexts: vec!["context-1".to_string(), "context-2".to_string()],
            }
        }

        fn add_namespace(&mut self, name: &str) {
            self.namespaces.push(K8sNamespace {
                metadata: ObjectMeta {
                    name: Some(name.to_string()),
                    ..Default::default()
                },
                spec: Some(NamespaceSpec { finalizers: None }),
                status: None,
            });
        }

        fn add_pod(
            &mut self, namespace: &str, name: &str, labels: BTreeMap<String, String>,
            container_ports: Vec<(String, i32)>,
        ) {
            let namespace_pods = self.pods.entry(namespace.to_string()).or_default();

            let containers = vec![Container {
                name: "container-1".to_string(),
                image: Some("test-image".to_string()),
                ports: Some(
                    container_ports
                        .into_iter()
                        .map(|(name, port)| ContainerPort {
                            name: Some(name),
                            container_port: port,
                            host_ip: None,
                            host_port: None,
                            protocol: None,
                        })
                        .collect(),
                ),
                ..Default::default()
            }];

            namespace_pods.push(K8sPod {
                metadata: ObjectMeta {
                    name: Some(name.to_string()),
                    namespace: Some(namespace.to_string()),
                    labels: Some(labels),
                    ..Default::default()
                },
                spec: Some(PodSpec {
                    containers,
                    ..Default::default()
                }),
                status: None,
            });
        }

        fn add_service(
            &mut self, namespace: &str, name: &str, selector: BTreeMap<String, String>,
            ports: Vec<(String, i32, Option<IntOrString>)>,
        ) {
            let namespace_services = self.services.entry(namespace.to_string()).or_default();

            namespace_services.push(K8sService {
                metadata: ObjectMeta {
                    name: Some(name.to_string()),
                    namespace: Some(namespace.to_string()),
                    ..Default::default()
                },
                spec: Some(ServiceSpec {
                    selector: Some(selector),
                    ports: Some(
                        ports
                            .into_iter()
                            .map(|(name, port, target_port)| ServicePort {
                                name: Some(name),
                                port,
                                target_port,
                                node_port: None,
                                protocol: None,
                                app_protocol: None,
                            })
                            .collect(),
                    ),
                    ..Default::default()
                }),
                status: None,
            });
        }

        fn get_context_infos(&self) -> Vec<KubeContextInfo> {
            self.contexts
                .iter()
                .map(|ctx_name| KubeContextInfo {
                    name: ctx_name.clone(),
                })
                .collect()
        }
    }

    fn setup_test_env() -> MockKubeClient {
        let mut mock_client = MockKubeClient::new();

        mock_client.add_namespace("default");
        mock_client.add_namespace("kube-system");
        mock_client.add_namespace("test-ns");

        let mut app_labels = BTreeMap::new();
        app_labels.insert("app".to_string(), "test-app".to_string());

        let container_ports = vec![("http".to_string(), 8080), ("metrics".to_string(), 9090)];

        mock_client.add_pod(
            "test-ns",
            "test-pod-1",
            app_labels.clone(),
            container_ports.clone(),
        );

        mock_client.add_service(
            "test-ns",
            "test-service",
            app_labels.clone(),
            vec![
                (
                    "http".to_string(),
                    80,
                    Some(IntOrString::String("http".to_string())),
                ),
                ("metrics".to_string(), 9090, Some(IntOrString::Int(9090))),
            ],
        );

        mock_client
    }

    #[test]
    fn test_list_kube_contexts() {
        let mock_client = setup_test_env();
        let contexts = mock_client.get_context_infos();

        assert_eq!(contexts.len(), 2);
        assert_eq!(contexts[0].name, "context-1");
        assert_eq!(contexts[1].name, "context-2");
    }

    #[test]
    fn test_list_namespaces() {
        let mock_client = setup_test_env();

        let namespaces = mock_client
            .namespaces
            .iter()
            .map(|ns| KubeNamespaceInfo {
                name: ns.metadata.name.clone().unwrap(),
            })
            .collect::<Vec<_>>();

        assert_eq!(namespaces.len(), 3);
        assert_eq!(namespaces[0].name, "default");
        assert_eq!(namespaces[1].name, "kube-system");
        assert_eq!(namespaces[2].name, "test-ns");
    }

    #[test]
    fn test_list_services() {
        let mock_client = setup_test_env();

        let services = mock_client
            .services
            .get("test-ns")
            .unwrap()
            .iter()
            .map(|svc| KubeServiceInfo {
                name: svc.metadata.name.clone().unwrap(),
            })
            .collect::<Vec<_>>();

        assert_eq!(services.len(), 1);
        assert_eq!(services[0].name, "test-service");
    }

    #[test]
    fn test_list_pods() {
        let mock_client = setup_test_env();

        let pods = mock_client.pods.get("test-ns").unwrap();
        let pod_labels = pods[0].metadata.labels.as_ref().unwrap();
        let label_strings: HashSet<String> =
            pod_labels.iter().map(|(k, v)| format!("{k}={v}")).collect();

        assert_eq!(label_strings.len(), 1);
        assert!(label_strings.contains("app=test-app"));
    }

    #[test]
    fn test_list_ports() {
        let mock_client = setup_test_env();

        let service = &mock_client.services.get("test-ns").unwrap()[0];

        let ports = service.spec.as_ref().unwrap().ports.as_ref().unwrap();
        let port_infos: Vec<KubeServicePortInfo> = ports
            .iter()
            .filter_map(|port| {
                port.target_port.as_ref().map(|target| KubeServicePortInfo {
                    name: port.name.clone(),
                    port: Some(target.clone()),
                })
            })
            .collect();

        assert_eq!(port_infos.len(), 2);

        let port_names: Vec<String> = port_infos.iter().filter_map(|p| p.name.clone()).collect();

        assert!(port_names.contains(&"http".to_string()));
        assert!(port_names.contains(&"metrics".to_string()));
    }

    fn kube_response(status: &str, body: serde_json::Value) -> String {
        let body = body.to_string();
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn pod_json(name: &str, ports: &[(&str, i32)]) -> serde_json::Value {
        let ports: Vec<_> = ports
            .iter()
            .map(|(port_name, port)| serde_json::json!({"name": port_name, "containerPort": port}))
            .collect();
        serde_json::json!({
            "metadata": {"name": name, "labels": {"app": "web"}},
            "spec": {"containers": [{"name": "app", "image": "app", "ports": ports}]}
        })
    }

    async fn serve_fake_kube_api(pods: Vec<serde_json::Value>) -> std::net::SocketAddr {
        use tokio::io::{
            AsyncReadExt,
            AsyncWriteExt,
        };

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let pods = pods.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&buf[..n]);
                    let line = request.lines().next().unwrap_or_default();
                    let response = if line.contains("/version") {
                        kube_response(
                            "200 OK",
                            serde_json::json!({
                                "major": "1", "minor": "30", "gitVersion": "v1.30.0",
                                "gitCommit": "x", "gitTreeState": "clean",
                                "buildDate": "2024-01-01T00:00:00Z", "goVersion": "go1",
                                "compiler": "gc", "platform": "linux/amd64"
                            }),
                        )
                    } else if line.contains("/pods") {
                        kube_response(
                            "200 OK",
                            serde_json::json!({
                                "apiVersion": "v1", "kind": "PodList",
                                "metadata": {"resourceVersion": "1"}, "items": pods
                            }),
                        )
                    } else {
                        kube_response(
                            "404 Not Found",
                            serde_json::json!({
                                "kind": "Status", "apiVersion": "v1", "metadata": {},
                                "status": "Failure", "reason": "NotFound", "code": 404,
                                "message": "not found"
                            }),
                        )
                    };
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        addr
    }

    #[tokio::test]
    async fn list_ports_for_pod_label_lists_each_port_once_across_replicas() {
        kftray_portforward::ssl::ensure_crypto_provider_installed();

        let addr = serve_fake_kube_api(vec![
            pod_json("web-1", &[("http", 8080)]),
            pod_json("web-2", &[("http", 8080)]),
            pod_json("web-3", &[("http", 8080), ("metrics", 9090)]),
        ])
        .await;

        let dir = tempfile::tempdir().unwrap();
        let kubeconfig = dir.path().join("config");
        std::fs::write(
            &kubeconfig,
            format!(
                "apiVersion: v1\nkind: Config\nclusters:\n- name: c\n  cluster:\n    server: http://{addr}\nusers:\n- name: u\n  user: {{}}\ncontexts:\n- name: ctx\n  context:\n    cluster: c\n    user: u\n    namespace: ns\ncurrent-context: ctx\n"
            ),
        )
        .unwrap();

        let ports = list_ports(
            "ctx",
            "ns",
            "app=web",
            Some(kubeconfig.to_string_lossy().into_owned()),
        )
        .await
        .unwrap();

        let ports: Vec<_> = ports.into_iter().map(|p| (p.name, p.port)).collect();
        assert_eq!(
            ports,
            vec![
                (Some("http".to_string()), Some(IntOrString::Int(8080))),
                (Some("metrics".to_string()), Some(IntOrString::Int(9090))),
            ]
        );
    }
}

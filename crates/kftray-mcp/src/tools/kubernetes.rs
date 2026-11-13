//! Kubernetes discovery tools for MCP.
//!
//! These tools allow LLMs to discover Kubernetes resources like contexts,
//! namespaces, services, pods, and ports.

use serde::{
    Deserialize,
    Serialize,
};
use serde_json::{
    Value,
    json,
};

use crate::protocol::{
    CallToolResult,
    Tool,
};
use crate::tools::McpTool;

// ============================================================================
// List Kube Contexts Tool
// ============================================================================

pub struct ListKubeContextsTool;

#[derive(Debug, Deserialize)]
struct ListKubeContextsArgs {
    kubeconfig: Option<String>,
}

#[derive(Debug, Serialize)]
struct KubeContextResponse {
    contexts: Vec<String>,
}

#[async_trait::async_trait]
impl McpTool for ListKubeContextsTool {
    fn definition(&self) -> Tool {
        Tool::with_schema(
            "list_kube_contexts",
            "List all available Kubernetes contexts from kubeconfig. Returns the names of all contexts that can be used for port-forwarding.",
            json!({
                "kubeconfig": {
                    "type": "string",
                    "description": "Optional path to a kubeconfig file or a folder of kubeconfig files. If not provided, uses default kubeconfig location."
                }
            }),
            None,
        )
    }

    async fn execute(&self, arguments: Option<Value>) -> CallToolResult {
        let args: ListKubeContextsArgs = match arguments {
            Some(v) => {
                serde_json::from_value(v).unwrap_or(ListKubeContextsArgs { kubeconfig: None })
            }
            None => ListKubeContextsArgs { kubeconfig: None },
        };

        match kftray_portforward::list_kube_contexts(args.kubeconfig).await {
            Ok(contexts) => {
                let context_names: Vec<String> = contexts.into_iter().map(|c| c.name).collect();
                let response = KubeContextResponse {
                    contexts: context_names,
                };
                CallToolResult::json(&response).unwrap_or_else(|e| {
                    CallToolResult::error(format!("Failed to serialize response: {e}"))
                })
            }
            Err(e) => CallToolResult::error(format!("Failed to list contexts: {e}")),
        }
    }
}

// ============================================================================
// List Namespaces Tool
// ============================================================================

pub struct ListNamespacesTool;

#[derive(Debug, Deserialize)]
struct ListNamespacesArgs {
    context: String,
    kubeconfig: Option<String>,
}

#[derive(Debug, Serialize)]
struct NamespacesResponse {
    namespaces: Vec<String>,
}

#[async_trait::async_trait]
impl McpTool for ListNamespacesTool {
    fn definition(&self) -> Tool {
        Tool::with_schema(
            "list_namespaces",
            "List all namespaces in a Kubernetes cluster for the specified context.",
            json!({
                "context": {
                    "type": "string",
                    "description": "The Kubernetes context to use"
                },
                "kubeconfig": {
                    "type": "string",
                    "description": "Optional path to a kubeconfig file or a folder of kubeconfig files"
                }
            }),
            Some(vec!["context".to_string()]),
        )
    }

    async fn execute(&self, arguments: Option<Value>) -> CallToolResult {
        let args: ListNamespacesArgs = match arguments {
            Some(v) => match serde_json::from_value(v) {
                Ok(a) => a,
                Err(e) => return CallToolResult::error(format!("Invalid arguments: {e}")),
            },
            None => return CallToolResult::error("Missing required argument: context"),
        };

        match kftray_portforward::create_client_with_specific_context(
            args.kubeconfig,
            &args.context,
        )
        .await
        {
            Ok(connection) => {
                match kftray_portforward::list_all_namespaces(connection.client).await {
                    Ok(namespaces) => {
                        let response = NamespacesResponse { namespaces };
                        CallToolResult::json(&response).unwrap_or_else(|e| {
                            CallToolResult::error(format!("Failed to serialize response: {e}"))
                        })
                    }
                    Err(e) => CallToolResult::error(format!("Failed to list namespaces: {e}")),
                }
            }
            Err(e) => CallToolResult::error(format!("Failed to create Kubernetes client: {e}")),
        }
    }
}

// ============================================================================
// List Services Tool
// ============================================================================

pub struct ListServicesTool;

#[derive(Debug, Deserialize)]
struct ListServicesArgs {
    context: String,
    namespace: String,
    kubeconfig: Option<String>,
}

#[derive(Debug, Serialize)]
struct ServicesResponse {
    services: Vec<String>,
}

#[async_trait::async_trait]
impl McpTool for ListServicesTool {
    fn definition(&self) -> Tool {
        Tool::with_schema(
            "list_services",
            "List all services in a specific namespace of a Kubernetes cluster.",
            json!({
                "context": {
                    "type": "string",
                    "description": "The Kubernetes context to use"
                },
                "namespace": {
                    "type": "string",
                    "description": "The namespace to list services from"
                },
                "kubeconfig": {
                    "type": "string",
                    "description": "Optional path to a kubeconfig file or a folder of kubeconfig files"
                }
            }),
            Some(vec!["context".to_string(), "namespace".to_string()]),
        )
    }

    async fn execute(&self, arguments: Option<Value>) -> CallToolResult {
        let args: ListServicesArgs = match arguments {
            Some(v) => match serde_json::from_value(v) {
                Ok(a) => a,
                Err(e) => return CallToolResult::error(format!("Invalid arguments: {e}")),
            },
            None => return CallToolResult::error("Missing required arguments: context, namespace"),
        };

        use k8s_openapi::api::core::v1::Service;
        use kube::{
            Api,
            ResourceExt,
            api::ListParams,
        };

        match kftray_portforward::create_client_with_specific_context(
            args.kubeconfig,
            &args.context,
        )
        .await
        {
            Ok(connection) => {
                let api: Api<Service> = Api::namespaced(connection.client, &args.namespace);
                match api.list(&ListParams::default()).await {
                    Ok(service_list) => {
                        let services: Vec<String> =
                            service_list.iter().map(|svc| svc.name_any()).collect();
                        let response = ServicesResponse { services };
                        CallToolResult::json(&response).unwrap_or_else(|e| {
                            CallToolResult::error(format!("Failed to serialize response: {e}"))
                        })
                    }
                    Err(e) => CallToolResult::error(format!("Failed to list services: {e}")),
                }
            }
            Err(e) => CallToolResult::error(format!("Failed to create Kubernetes client: {e}")),
        }
    }
}

// ============================================================================
// List Pods Tool
// ============================================================================

pub struct ListPodsTool;

#[derive(Debug, Deserialize)]
struct ListPodsArgs {
    context: String,
    namespace: String,
    kubeconfig: Option<String>,
    label_selector: Option<String>,
}

#[derive(Debug, Serialize)]
struct PodResponse {
    name: String,
    status: String,
    labels: std::collections::HashMap<String, String>,
}

#[derive(Debug, Serialize)]
struct PodsResponse {
    pods: Vec<PodResponse>,
}

#[async_trait::async_trait]
impl McpTool for ListPodsTool {
    fn definition(&self) -> Tool {
        Tool::with_schema(
            "list_pods",
            "List pods in a specific namespace, optionally filtered by label selector. Returns pod names, status, and labels.",
            json!({
                "context": {
                    "type": "string",
                    "description": "The Kubernetes context to use"
                },
                "namespace": {
                    "type": "string",
                    "description": "The namespace to list pods from"
                },
                "kubeconfig": {
                    "type": "string",
                    "description": "Optional path to a kubeconfig file or a folder of kubeconfig files"
                },
                "label_selector": {
                    "type": "string",
                    "description": "Optional label selector to filter pods (e.g., 'app=nginx')"
                }
            }),
            Some(vec!["context".to_string(), "namespace".to_string()]),
        )
    }

    async fn execute(&self, arguments: Option<Value>) -> CallToolResult {
        let args: ListPodsArgs = match arguments {
            Some(v) => match serde_json::from_value(v) {
                Ok(a) => a,
                Err(e) => return CallToolResult::error(format!("Invalid arguments: {e}")),
            },
            None => return CallToolResult::error("Missing required arguments: context, namespace"),
        };

        use k8s_openapi::api::core::v1::Pod;
        use kube::{
            Api,
            ResourceExt,
            api::ListParams,
        };

        match kftray_portforward::create_client_with_specific_context(
            args.kubeconfig,
            &args.context,
        )
        .await
        {
            Ok(connection) => {
                let api: Api<Pod> = Api::namespaced(connection.client, &args.namespace);
                let mut list_params = ListParams::default();
                if let Some(selector) = args.label_selector {
                    list_params = list_params.labels(&selector);
                }

                match api.list(&list_params).await {
                    Ok(pod_list) => {
                        let pods: Vec<PodResponse> = pod_list
                            .iter()
                            .map(|pod| {
                                let status = pod
                                    .status
                                    .as_ref()
                                    .and_then(|s| s.phase.clone())
                                    .unwrap_or_else(|| "Unknown".to_string());

                                let labels = pod
                                    .metadata
                                    .labels
                                    .clone()
                                    .unwrap_or_default()
                                    .into_iter()
                                    .collect();

                                PodResponse {
                                    name: pod.name_any(),
                                    status,
                                    labels,
                                }
                            })
                            .collect();

                        let response = PodsResponse { pods };
                        CallToolResult::json(&response).unwrap_or_else(|e| {
                            CallToolResult::error(format!("Failed to serialize response: {e}"))
                        })
                    }
                    Err(e) => CallToolResult::error(format!("Failed to list pods: {e}")),
                }
            }
            Err(e) => CallToolResult::error(format!("Failed to create Kubernetes client: {e}")),
        }
    }
}

// ============================================================================
// List Ports Tool
// ============================================================================

pub struct ListPortsTool;

#[derive(Debug, Deserialize)]
struct ListPortsArgs {
    context: String,
    namespace: String,
    service: String,
    kubeconfig: Option<String>,
}

#[derive(Debug, Serialize)]
struct PortInfo {
    name: Option<String>,
    port: i32,
    protocol: String,
}

fn protocol_or_tcp(protocol: Option<&String>) -> String {
    protocol.cloned().unwrap_or_else(|| "TCP".to_string())
}

async fn list_forwardable_ports(
    client: kube::Client, namespace: &str, service_name: &str,
) -> Result<Vec<PortInfo>, String> {
    use k8s_openapi::api::core::v1::{
        Pod,
        Service,
    };
    use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
    use kube::{
        Api,
        api::ListParams,
    };

    let service_api: Api<Service> = Api::namespaced(client.clone(), namespace);
    let pod_api: Api<Pod> = Api::namespaced(client, namespace);

    match service_api.get(service_name).await {
        Ok(service) => {
            let spec = service.spec.unwrap_or_default();
            let selector = spec
                .selector
                .filter(|selector| !selector.is_empty())
                .map(|selector| {
                    selector
                        .iter()
                        .map(|(key, value)| format!("{key}={value}"))
                        .collect::<Vec<_>>()
                        .join(",")
                });
            let mut selected_pods: Option<Vec<Pod>> = None;
            let mut ports = Vec::new();

            for sp in spec.ports.unwrap_or_default() {
                let port = match &sp.target_port {
                    Some(IntOrString::Int(port)) => Some(*port),
                    None => Some(sp.port),
                    Some(IntOrString::String(port_name)) => {
                        let Some(selector) = &selector else {
                            continue;
                        };
                        if selected_pods.is_none() {
                            let pods = pod_api
                                .list(&ListParams::default().labels(selector))
                                .await
                                .map_err(|e| format!("Failed to list pods: {e}"))?;
                            selected_pods = Some(pods.items);
                        }
                        selected_pods
                            .iter()
                            .flatten()
                            .filter_map(|pod| pod.spec.as_ref())
                            .flat_map(|spec| spec.containers.iter())
                            .filter_map(|container| container.ports.as_ref())
                            .flatten()
                            .find(|cp| cp.name.as_deref() == Some(port_name.as_str()))
                            .map(|cp| cp.container_port)
                    }
                };
                if let Some(port) = port {
                    ports.push(PortInfo {
                        name: sp.name.clone(),
                        port,
                        protocol: protocol_or_tcp(sp.protocol.as_ref()),
                    });
                }
            }

            if ports.is_empty() {
                Err(format!(
                    "No ports found for service '{service_name}' in namespace '{namespace}'"
                ))
            } else {
                Ok(ports)
            }
        }
        Err(kube::Error::Api(status)) if status.code == 404 => {
            let pods = pod_api
                .list(&ListParams::default().labels(service_name))
                .await
                .map_err(|e| {
                    format!("Service '{service_name}' not found and failed to list pods: {e}")
                })?;

            let mut ports: Vec<PortInfo> = pods
                .iter()
                .filter_map(|pod| pod.spec.as_ref())
                .flat_map(|spec| spec.containers.iter())
                .filter_map(|container| container.ports.as_ref())
                .flatten()
                .map(|cp| PortInfo {
                    name: cp.name.clone(),
                    port: cp.container_port,
                    protocol: protocol_or_tcp(cp.protocol.as_ref()),
                })
                .collect();
            ports.sort_by_key(|a| a.port);
            ports.dedup_by(|a, b| a.port == b.port && a.name == b.name);

            if ports.is_empty() {
                Err(format!(
                    "Service '{service_name}' not found and no ports found for label '{service_name}' in namespace '{namespace}'"
                ))
            } else {
                Ok(ports)
            }
        }
        Err(e) => Err(format!("Failed to get service '{service_name}': {e}")),
    }
}

#[derive(Debug, Serialize)]
struct PortsResponse {
    ports: Vec<PortInfo>,
}

#[async_trait::async_trait]
impl McpTool for ListPortsTool {
    fn definition(&self) -> Tool {
        Tool::with_schema(
            "list_ports",
            "List the ports kftray forwards to for a Kubernetes service: the target port of each service port, with a named target port resolved to the container port of a selected pod. Pass `port` as remote_port. If the service does not exist, lists the container ports of pods matching the label selector.",
            json!({
                "context": {
                    "type": "string",
                    "description": "The Kubernetes context to use"
                },
                "namespace": {
                    "type": "string",
                    "description": "The namespace containing the service"
                },
                "service": {
                    "type": "string",
                    "description": "The service name or pod label selector"
                },
                "kubeconfig": {
                    "type": "string",
                    "description": "Optional path to a kubeconfig file or a folder of kubeconfig files"
                }
            }),
            Some(vec![
                "context".to_string(),
                "namespace".to_string(),
                "service".to_string(),
            ]),
        )
    }

    async fn execute(&self, arguments: Option<Value>) -> CallToolResult {
        let args: ListPortsArgs = match arguments {
            Some(v) => match serde_json::from_value(v) {
                Ok(a) => a,
                Err(e) => return CallToolResult::error(format!("Invalid arguments: {e}")),
            },
            None => {
                return CallToolResult::error(
                    "Missing required arguments: context, namespace, service",
                );
            }
        };

        match kftray_portforward::create_client_with_specific_context(
            args.kubeconfig,
            &args.context,
        )
        .await
        {
            Ok(connection) => {
                match list_forwardable_ports(connection.client, &args.namespace, &args.service)
                    .await
                {
                    Ok(ports) => {
                        CallToolResult::json(&PortsResponse { ports }).unwrap_or_else(|e| {
                            CallToolResult::error(format!("Failed to serialize response: {e}"))
                        })
                    }
                    Err(e) => CallToolResult::error(e),
                }
            }
            Err(e) => CallToolResult::error(format!("Failed to create Kubernetes client: {e}")),
        }
    }
}

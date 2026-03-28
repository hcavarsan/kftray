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
                    "description": "Optional path to kubeconfig file. If not provided, uses default kubeconfig location."
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
                    "description": "Optional path to kubeconfig file"
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

        // Create a Kubernetes client for the specified context
        match kftray_portforward::create_client_with_specific_context(
            args.kubeconfig,
            Some(&args.context),
        )
        .await
        {
            Ok((Some(client), _, _)) => {
                match kftray_portforward::list_all_namespaces(client).await {
                    Ok(namespaces) => {
                        let response = NamespacesResponse { namespaces };
                        CallToolResult::json(&response).unwrap_or_else(|e| {
                            CallToolResult::error(format!("Failed to serialize response: {e}"))
                        })
                    }
                    Err(e) => CallToolResult::error(format!("Failed to list namespaces: {e}")),
                }
            }
            Ok((None, _, _)) => CallToolResult::error(format!(
                "Could not create client for context: {}",
                args.context
            )),
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
                    "description": "Optional path to kubeconfig file"
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
            Some(&args.context),
        )
        .await
        {
            Ok((Some(client), _, _)) => {
                let api: Api<Service> = Api::namespaced(client, &args.namespace);
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
            Ok((None, _, _)) => CallToolResult::error(format!(
                "Could not create client for context: {}",
                args.context
            )),
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
                    "description": "Optional path to kubeconfig file"
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
            Some(&args.context),
        )
        .await
        {
            Ok((Some(client), _, _)) => {
                let api: Api<Pod> = Api::namespaced(client, &args.namespace);
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
            Ok((None, _, _)) => CallToolResult::error(format!(
                "Could not create client for context: {}",
                args.context
            )),
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
    target_port: String,
    protocol: String,
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
            "List all ports exposed by a Kubernetes service. If the service is not found, attempts to list ports from pods matching the label selector.",
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
                    "description": "Optional path to kubeconfig file"
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

        use k8s_openapi::api::core::v1::Service;
        use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
        use kube::{
            Api,
            api::ListParams,
        };

        match kftray_portforward::create_client_with_specific_context(
            args.kubeconfig,
            Some(&args.context),
        )
        .await
        {
            Ok((Some(client), _, _)) => {
                let api: Api<Service> = Api::namespaced(client.clone(), &args.namespace);

                match api.get(&args.service).await {
                    Ok(service) => {
                        let ports: Vec<PortInfo> = service
                            .spec
                            .as_ref()
                            .and_then(|spec| spec.ports.as_ref())
                            .map(|ports| {
                                ports
                                    .iter()
                                    .map(|p| {
                                        let target_port = match &p.target_port {
                                            Some(IntOrString::Int(port)) => port.to_string(),
                                            Some(IntOrString::String(name)) => name.clone(),
                                            None => p.port.to_string(),
                                        };
                                        PortInfo {
                                            name: p.name.clone(),
                                            port: p.port,
                                            target_port,
                                            protocol: p
                                                .protocol
                                                .clone()
                                                .unwrap_or_else(|| "TCP".to_string()),
                                        }
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();

                        let response = PortsResponse { ports };
                        CallToolResult::json(&response).unwrap_or_else(|e| {
                            CallToolResult::error(format!("Failed to serialize response: {e}"))
                        })
                    }
                    Err(_) => {
                        // Service not found, try to find pods with matching labels
                        use k8s_openapi::api::core::v1::Pod;

                        let pod_api: Api<Pod> = Api::namespaced(client, &args.namespace);
                        match pod_api
                            .list(&ListParams::default().labels(&args.service))
                            .await
                        {
                            Ok(pod_list) => {
                                let mut ports: Vec<PortInfo> = Vec::new();

                                for pod in pod_list.iter() {
                                    if let Some(spec) = &pod.spec {
                                        for container in &spec.containers {
                                            if let Some(container_ports) = &container.ports {
                                                for cp in container_ports {
                                                    ports.push(PortInfo {
                                                        name: cp.name.clone(),
                                                        port: cp.container_port,
                                                        target_port: cp.container_port.to_string(),
                                                        protocol: cp
                                                            .protocol
                                                            .clone()
                                                            .unwrap_or_else(|| "TCP".to_string()),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }

                                // Deduplicate ports
                                ports.sort_by(|a, b| a.port.cmp(&b.port));
                                ports.dedup_by(|a, b| a.port == b.port && a.name == b.name);

                                let response = PortsResponse { ports };
                                CallToolResult::json(&response).unwrap_or_else(|e| {
                                    CallToolResult::error(format!(
                                        "Failed to serialize response: {e}"
                                    ))
                                })
                            }
                            Err(e) => CallToolResult::error(format!(
                                "Service '{}' not found and failed to list pods: {e}",
                                args.service
                            )),
                        }
                    }
                }
            }
            Ok((None, _, _)) => CallToolResult::error(format!(
                "Could not create client for context: {}",
                args.context
            )),
            Err(e) => CallToolResult::error(format!("Failed to create Kubernetes client: {e}")),
        }
    }
}

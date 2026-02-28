//! Port-forward management tools for MCP.
//!
//! These tools allow LLMs to manage port-forwarding sessions: listing active
//! forwards, starting new ones, and stopping existing ones.

use crate::protocol::{CallToolResult, Tool};
use crate::tools::McpTool;
use kftray_commons::models::config_model::Config;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

// ============================================================================
// List Active Port Forwards Tool
// ============================================================================

pub struct ListActivePortForwardsTool;

#[derive(Debug, Serialize)]
struct ActivePortForward {
    config_id: String,
    service: Option<String>,
    namespace: String,
    local_port: Option<u16>,
    remote_port: Option<u16>,
    context: Option<String>,
    protocol: String,
    workload_type: Option<String>,
    alias: Option<String>,
}

#[derive(Debug, Serialize)]
struct ActivePortForwardsResponse {
    port_forwards: Vec<ActivePortForward>,
    count: usize,
}

#[async_trait::async_trait]
impl McpTool for ListActivePortForwardsTool {
    fn definition(&self) -> Tool {
        Tool::new(
            "list_active_port_forwards",
            "List all currently active port-forwarding sessions. Returns details about each active port-forward including config ID, service, ports, and status.",
        )
    }

    async fn execute(&self, _arguments: Option<Value>) -> CallToolResult {
        use kftray_commons::utils::config_state::get_configs_state;
        use kftray_commons::config::get_configs;

        // Get all config states
        let config_states = match get_configs_state().await {
            Ok(states) => states,
            Err(e) => return CallToolResult::error(format!("Failed to get config states: {e}")),
        };

        // Get all configs
        let configs = match get_configs().await {
            Ok(c) => c,
            Err(e) => return CallToolResult::error(format!("Failed to get configs: {e}")),
        };

        // Filter for running configs
        let running_config_ids: std::collections::HashSet<i64> = config_states
            .iter()
            .filter(|s| s.is_running)
            .map(|s| s.config_id)
            .collect();

        let active_forwards: Vec<ActivePortForward> = configs
            .into_iter()
            .filter(|c| c.id.is_some_and(|id| running_config_ids.contains(&id)))
            .map(|c| ActivePortForward {
                config_id: c.id.map_or("unknown".to_string(), |id| id.to_string()),
                service: c.service,
                namespace: c.namespace,
                local_port: c.local_port,
                remote_port: c.remote_port,
                context: c.context,
                protocol: c.protocol,
                workload_type: c.workload_type,
                alias: c.alias,
            })
            .collect();

        let count = active_forwards.len();
        let response = ActivePortForwardsResponse {
            port_forwards: active_forwards,
            count,
        };

        CallToolResult::json(&response)
            .unwrap_or_else(|e| CallToolResult::error(format!("Failed to serialize response: {e}")))
    }
}

// ============================================================================
// Start Port Forward Tool
// ============================================================================

pub struct StartPortForwardTool;

#[derive(Debug, Deserialize)]
struct StartPortForwardArgs {
    config_id: Option<i64>,
    // Or create a new port-forward with these parameters:
    context: Option<String>,
    namespace: Option<String>,
    service: Option<String>,
    target: Option<String>,
    local_port: Option<u16>,
    remote_port: Option<u16>,
    protocol: Option<String>,
    workload_type: Option<String>,
    kubeconfig: Option<String>,
    alias: Option<String>,
}

#[derive(Debug, Serialize)]
struct StartPortForwardResponse {
    success: bool,
    message: String,
    config_id: Option<i64>,
    local_port: Option<u16>,
    remote_port: Option<u16>,
}

#[async_trait::async_trait]
impl McpTool for StartPortForwardTool {
    fn definition(&self) -> Tool {
        Tool::with_schema(
            "start_port_forward",
            "Start a port-forward session. You can either provide a config_id to start an existing configuration, or provide all necessary parameters to create and start a new port-forward.",
            json!({
                "config_id": {
                    "type": "integer",
                    "description": "ID of an existing configuration to start. If provided, other parameters are ignored."
                },
                "context": {
                    "type": "string",
                    "description": "Kubernetes context to use (required for new port-forward)"
                },
                "namespace": {
                    "type": "string",
                    "description": "Kubernetes namespace (required for new port-forward)"
                },
                "service": {
                    "type": "string",
                    "description": "Service name (required for workload_type 'service')"
                },
                "target": {
                    "type": "string",
                    "description": "Pod label selector (required for workload_type 'pod', e.g., 'app=nginx')"
                },
                "local_port": {
                    "type": "integer",
                    "description": "Local port to listen on. If not provided, a random available port is used."
                },
                "remote_port": {
                    "type": "integer",
                    "description": "Remote port to forward to (required for new port-forward)"
                },
                "protocol": {
                    "type": "string",
                    "enum": ["tcp", "udp"],
                    "description": "Protocol to use. Defaults to 'tcp'."
                },
                "workload_type": {
                    "type": "string",
                    "enum": ["service", "pod", "proxy"],
                    "description": "Type of workload to forward to. Defaults to 'service'."
                },
                "kubeconfig": {
                    "type": "string",
                    "description": "Optional path to kubeconfig file"
                },
                "alias": {
                    "type": "string",
                    "description": "Optional friendly name for this port-forward"
                }
            }),
            None,
        )
    }

    async fn execute(&self, arguments: Option<Value>) -> CallToolResult {
        let args: StartPortForwardArgs = match arguments {
            Some(v) => match serde_json::from_value(v) {
                Ok(a) => a,
                Err(e) => return CallToolResult::error(format!("Invalid arguments: {e}")),
            },
            None => {
                return CallToolResult::error(
                    "Must provide either config_id or parameters for a new port-forward",
                )
            }
        };

        // If config_id is provided, start that config
        if let Some(config_id) = args.config_id {
            match kftray_commons::config::get_config(config_id).await {
                Ok(config) => {
                    let protocol = config.protocol.clone();
                    let configs = vec![config];

                    let result = if protocol == "udp" {
                        kftray_portforward::start_port_forward(configs, "udp").await
                    } else {
                        kftray_portforward::start_port_forward(configs, "tcp").await
                    };

                    match result {
                        Ok(responses) => {
                            if let Some(resp) = responses.first() {
                                let response = StartPortForwardResponse {
                                    success: resp.status == 0,
                                    message: if resp.status == 0 {
                                        format!(
                                            "Port-forward started: {}:{} -> {}:{}",
                                            resp.local_port,
                                            resp.service,
                                            resp.namespace,
                                            resp.remote_port
                                        )
                                    } else {
                                        resp.stderr.clone()
                                    },
                                    config_id: Some(config_id),
                                    local_port: Some(resp.local_port),
                                    remote_port: Some(resp.remote_port),
                                };
                                CallToolResult::json(&response).unwrap_or_else(|e| {
                                    CallToolResult::error(format!(
                                        "Failed to serialize response: {e}"
                                    ))
                                })
                            } else {
                                CallToolResult::error("No response received from port-forward")
                            }
                        }
                        Err(e) => CallToolResult::error(format!("Failed to start port-forward: {e}")),
                    }
                }
                Err(e) => CallToolResult::error(format!("Config not found: {e}")),
            }
        } else {
            // Create a new config
            let namespace = match args.namespace {
                Some(ns) => ns,
                None => return CallToolResult::error("namespace is required for new port-forward"),
            };

            let remote_port = match args.remote_port {
                Some(p) => p,
                None => return CallToolResult::error("remote_port is required for new port-forward"),
            };

            let workload_type = args.workload_type.unwrap_or_else(|| "service".to_string());

            // Validate workload-specific requirements
            if workload_type == "service" && args.service.is_none() {
                return CallToolResult::error("service is required when workload_type is 'service'");
            }
            if workload_type == "pod" && args.target.is_none() {
                return CallToolResult::error(
                    "target (pod label selector) is required when workload_type is 'pod'",
                );
            }

            let config = Config {
                id: None,
                service: args.service,
                namespace: namespace.clone(),
                local_port: args.local_port,
                remote_port: Some(remote_port),
                context: args.context,
                workload_type: Some(workload_type),
                protocol: args.protocol.unwrap_or_else(|| "tcp".to_string()),
                remote_address: None,
                local_address: Some("127.0.0.1".to_string()),
                auto_loopback_address: false,
                alias: args.alias,
                domain_enabled: Some(false),
                kubeconfig: args.kubeconfig,
                target: args.target,
                http_logs_enabled: Some(false),
                http_logs_max_file_size: None,
                http_logs_retention_days: None,
                http_logs_auto_cleanup: None,
                exposure_type: None,
                cert_manager_enabled: None,
                cert_issuer: None,
                cert_issuer_kind: None,
                ingress_class: None,
                ingress_annotations: None,
            };

            // Insert the config first
            if let Err(e) = kftray_commons::config::insert_config(config.clone()).await {
                return CallToolResult::error(format!("Failed to create config: {e}"));
            }

            // Get the newly created config
            let configs = match kftray_commons::config::get_configs().await {
                Ok(c) => c,
                Err(e) => return CallToolResult::error(format!("Failed to get configs: {e}")),
            };

            let new_config = match configs.into_iter().last() {
                Some(c) => c,
                None => return CallToolResult::error("Failed to retrieve created config"),
            };

            let protocol = new_config.protocol.clone();
            let config_id = new_config.id;
            let configs = vec![new_config];

            let result = if protocol == "udp" {
                kftray_portforward::start_port_forward(configs, "udp").await
            } else {
                kftray_portforward::start_port_forward(configs, "tcp").await
            };

            match result {
                Ok(responses) => {
                    if let Some(resp) = responses.first() {
                        let response = StartPortForwardResponse {
                            success: resp.status == 0,
                            message: if resp.status == 0 {
                                format!(
                                    "Port-forward started: localhost:{} -> {}:{}/{}",
                                    resp.local_port, namespace, resp.service, resp.remote_port
                                )
                            } else {
                                resp.stderr.clone()
                            },
                            config_id,
                            local_port: Some(resp.local_port),
                            remote_port: Some(resp.remote_port),
                        };
                        CallToolResult::json(&response).unwrap_or_else(|e| {
                            CallToolResult::error(format!("Failed to serialize response: {e}"))
                        })
                    } else {
                        CallToolResult::error("No response received from port-forward")
                    }
                }
                Err(e) => CallToolResult::error(format!("Failed to start port-forward: {e}")),
            }
        }
    }
}

// ============================================================================
// Stop Port Forward Tool
// ============================================================================

pub struct StopPortForwardTool;

#[derive(Debug, Deserialize)]
struct StopPortForwardArgs {
    config_id: i64,
}

#[derive(Debug, Serialize)]
struct StopPortForwardResponse {
    success: bool,
    message: String,
    config_id: i64,
}

#[async_trait::async_trait]
impl McpTool for StopPortForwardTool {
    fn definition(&self) -> Tool {
        Tool::with_schema(
            "stop_port_forward",
            "Stop a specific port-forwarding session by its configuration ID.",
            json!({
                "config_id": {
                    "type": "integer",
                    "description": "The configuration ID of the port-forward to stop"
                }
            }),
            Some(vec!["config_id".to_string()]),
        )
    }

    async fn execute(&self, arguments: Option<Value>) -> CallToolResult {
        let args: StopPortForwardArgs = match arguments {
            Some(v) => match serde_json::from_value(v) {
                Ok(a) => a,
                Err(e) => return CallToolResult::error(format!("Invalid arguments: {e}")),
            },
            None => return CallToolResult::error("Missing required argument: config_id"),
        };

        match kftray_portforward::stop_port_forward(args.config_id.to_string()).await {
            Ok(resp) => {
                let response = StopPortForwardResponse {
                    success: resp.status == 0,
                    message: if resp.status == 0 {
                        format!(
                            "Port-forward stopped: {} in namespace {}",
                            resp.service, resp.namespace
                        )
                    } else {
                        resp.stderr
                    },
                    config_id: args.config_id,
                };
                CallToolResult::json(&response).unwrap_or_else(|e| {
                    CallToolResult::error(format!("Failed to serialize response: {e}"))
                })
            }
            Err(e) => CallToolResult::error(format!("Failed to stop port-forward: {e}")),
        }
    }
}

// ============================================================================
// Stop All Port Forwards Tool
// ============================================================================

pub struct StopAllPortForwardsTool;

#[derive(Debug, Serialize)]
struct StopAllPortForwardsResponse {
    success: bool,
    stopped_count: usize,
    message: String,
}

#[async_trait::async_trait]
impl McpTool for StopAllPortForwardsTool {
    fn definition(&self) -> Tool {
        Tool::new(
            "stop_all_port_forwards",
            "Stop all active port-forwarding sessions. Use with caution as this will terminate all running port-forwards.",
        )
    }

    async fn execute(&self, _arguments: Option<Value>) -> CallToolResult {
        match kftray_portforward::stop_all_port_forward().await {
            Ok(responses) => {
                let stopped_count = responses.iter().filter(|r| r.status == 0).count();
                let response = StopAllPortForwardsResponse {
                    success: true,
                    stopped_count,
                    message: format!("Stopped {stopped_count} port-forward(s)"),
                };
                CallToolResult::json(&response).unwrap_or_else(|e| {
                    CallToolResult::error(format!("Failed to serialize response: {e}"))
                })
            }
            Err(e) => CallToolResult::error(format!("Failed to stop port-forwards: {e}")),
        }
    }
}

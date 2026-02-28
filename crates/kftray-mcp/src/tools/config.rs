//! Configuration management tools for MCP.
//!
//! These tools allow LLMs to manage port-forward configurations: listing,
//! creating, updating, deleting, and importing/exporting configurations.

use crate::protocol::{CallToolResult, Tool};
use crate::tools::McpTool;
use kftray_commons::models::config_model::Config;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

// ============================================================================
// List Configs Tool
// ============================================================================

pub struct ListConfigsTool;

#[derive(Debug, Serialize)]
struct ConfigSummary {
    id: i64,
    alias: Option<String>,
    service: Option<String>,
    namespace: String,
    context: Option<String>,
    local_port: Option<u16>,
    remote_port: Option<u16>,
    protocol: String,
    workload_type: Option<String>,
}

#[derive(Debug, Serialize)]
struct ListConfigsResponse {
    configs: Vec<ConfigSummary>,
    count: usize,
}

#[async_trait::async_trait]
impl McpTool for ListConfigsTool {
    fn definition(&self) -> Tool {
        Tool::new(
            "list_configs",
            "List all saved port-forward configurations. Returns a summary of each configuration including ID, alias, service, namespace, and ports.",
        )
    }

    async fn execute(&self, _arguments: Option<Value>) -> CallToolResult {
        match kftray_commons::config::get_configs().await {
            Ok(configs) => {
                let summaries: Vec<ConfigSummary> = configs
                    .into_iter()
                    .filter_map(|c| {
                        c.id.map(|id| ConfigSummary {
                            id,
                            alias: c.alias,
                            service: c.service,
                            namespace: c.namespace,
                            context: c.context,
                            local_port: c.local_port,
                            remote_port: c.remote_port,
                            protocol: c.protocol,
                            workload_type: c.workload_type,
                        })
                    })
                    .collect();

                let count = summaries.len();
                let response = ListConfigsResponse {
                    configs: summaries,
                    count,
                };

                CallToolResult::json(&response).unwrap_or_else(|e| {
                    CallToolResult::error(format!("Failed to serialize response: {e}"))
                })
            }
            Err(e) => CallToolResult::error(format!("Failed to list configs: {e}")),
        }
    }
}

// ============================================================================
// Get Config Tool
// ============================================================================

pub struct GetConfigTool;

#[derive(Debug, Deserialize)]
struct GetConfigArgs {
    config_id: i64,
}

#[async_trait::async_trait]
impl McpTool for GetConfigTool {
    fn definition(&self) -> Tool {
        Tool::with_schema(
            "get_config",
            "Get the full details of a specific port-forward configuration by its ID.",
            json!({
                "config_id": {
                    "type": "integer",
                    "description": "The ID of the configuration to retrieve"
                }
            }),
            Some(vec!["config_id".to_string()]),
        )
    }

    async fn execute(&self, arguments: Option<Value>) -> CallToolResult {
        let args: GetConfigArgs = match arguments {
            Some(v) => match serde_json::from_value(v) {
                Ok(a) => a,
                Err(e) => return CallToolResult::error(format!("Invalid arguments: {e}")),
            },
            None => return CallToolResult::error("Missing required argument: config_id"),
        };

        match kftray_commons::config::get_config(args.config_id).await {
            Ok(config) => CallToolResult::json(&config)
                .unwrap_or_else(|e| CallToolResult::error(format!("Failed to serialize config: {e}"))),
            Err(e) => CallToolResult::error(format!("Failed to get config: {e}")),
        }
    }
}

// ============================================================================
// Create Config Tool
// ============================================================================

pub struct CreateConfigTool;

#[derive(Debug, Deserialize)]
struct CreateConfigArgs {
    context: String,
    namespace: String,
    service: Option<String>,
    target: Option<String>,
    local_port: Option<u16>,
    remote_port: u16,
    protocol: Option<String>,
    workload_type: Option<String>,
    kubeconfig: Option<String>,
    alias: Option<String>,
    remote_address: Option<String>,
    local_address: Option<String>,
    domain_enabled: Option<bool>,
}

#[derive(Debug, Serialize)]
struct CreateConfigResponse {
    success: bool,
    message: String,
    config_id: Option<i64>,
}

#[async_trait::async_trait]
impl McpTool for CreateConfigTool {
    fn definition(&self) -> Tool {
        Tool::with_schema(
            "create_config",
            "Create a new port-forward configuration. The configuration is saved but not started automatically.",
            json!({
                "context": {
                    "type": "string",
                    "description": "Kubernetes context to use"
                },
                "namespace": {
                    "type": "string",
                    "description": "Kubernetes namespace"
                },
                "service": {
                    "type": "string",
                    "description": "Service name (required for workload_type 'service')"
                },
                "target": {
                    "type": "string",
                    "description": "Pod label selector (required for workload_type 'pod')"
                },
                "local_port": {
                    "type": "integer",
                    "description": "Local port to listen on. If not provided, a random port is assigned."
                },
                "remote_port": {
                    "type": "integer",
                    "description": "Remote port to forward to"
                },
                "protocol": {
                    "type": "string",
                    "enum": ["tcp", "udp"],
                    "description": "Protocol to use. Defaults to 'tcp'."
                },
                "workload_type": {
                    "type": "string",
                    "enum": ["service", "pod", "proxy"],
                    "description": "Type of workload. Defaults to 'service'."
                },
                "kubeconfig": {
                    "type": "string",
                    "description": "Optional path to kubeconfig file"
                },
                "alias": {
                    "type": "string",
                    "description": "Optional friendly name for this configuration"
                },
                "remote_address": {
                    "type": "string",
                    "description": "Remote address for proxy workload type"
                },
                "local_address": {
                    "type": "string",
                    "description": "Local address to bind to. Defaults to '127.0.0.1'."
                },
                "domain_enabled": {
                    "type": "boolean",
                    "description": "Whether to enable domain name resolution"
                }
            }),
            Some(vec![
                "context".to_string(),
                "namespace".to_string(),
                "remote_port".to_string(),
            ]),
        )
    }

    async fn execute(&self, arguments: Option<Value>) -> CallToolResult {
        let args: CreateConfigArgs = match arguments {
            Some(v) => match serde_json::from_value(v) {
                Ok(a) => a,
                Err(e) => return CallToolResult::error(format!("Invalid arguments: {e}")),
            },
            None => {
                return CallToolResult::error(
                    "Missing required arguments: context, namespace, remote_port",
                )
            }
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
        if workload_type == "proxy" && args.remote_address.is_none() {
            return CallToolResult::error("remote_address is required when workload_type is 'proxy'");
        }

        let config = Config {
            id: None,
            service: args.service,
            namespace: args.namespace,
            local_port: args.local_port,
            remote_port: Some(args.remote_port),
            context: Some(args.context),
            workload_type: Some(workload_type),
            protocol: args.protocol.unwrap_or_else(|| "tcp".to_string()),
            remote_address: args.remote_address,
            local_address: args.local_address.or_else(|| Some("127.0.0.1".to_string())),
            auto_loopback_address: false,
            alias: args.alias,
            domain_enabled: args.domain_enabled,
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

        match kftray_commons::config::insert_config(config).await {
            Ok(()) => {
                // Get the newly created config ID
                let configs = match kftray_commons::config::get_configs().await {
                    Ok(c) => c,
                    Err(e) => {
                        return CallToolResult::error(format!(
                            "Config created but failed to retrieve ID: {e}"
                        ))
                    }
                };

                let config_id = configs.into_iter().last().and_then(|c| c.id);

                let response = CreateConfigResponse {
                    success: true,
                    message: "Configuration created successfully".to_string(),
                    config_id,
                };

                CallToolResult::json(&response).unwrap_or_else(|e| {
                    CallToolResult::error(format!("Failed to serialize response: {e}"))
                })
            }
            Err(e) => CallToolResult::error(format!("Failed to create config: {e}")),
        }
    }
}

// ============================================================================
// Update Config Tool
// ============================================================================

pub struct UpdateConfigTool;

#[derive(Debug, Deserialize)]
struct UpdateConfigArgs {
    config_id: i64,
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
    remote_address: Option<String>,
    local_address: Option<String>,
    domain_enabled: Option<bool>,
}

#[derive(Debug, Serialize)]
struct UpdateConfigResponse {
    success: bool,
    message: String,
}

#[async_trait::async_trait]
impl McpTool for UpdateConfigTool {
    fn definition(&self) -> Tool {
        Tool::with_schema(
            "update_config",
            "Update an existing port-forward configuration. Only the provided fields will be updated; others remain unchanged.",
            json!({
                "config_id": {
                    "type": "integer",
                    "description": "The ID of the configuration to update"
                },
                "context": {
                    "type": "string",
                    "description": "Kubernetes context to use"
                },
                "namespace": {
                    "type": "string",
                    "description": "Kubernetes namespace"
                },
                "service": {
                    "type": "string",
                    "description": "Service name"
                },
                "target": {
                    "type": "string",
                    "description": "Pod label selector"
                },
                "local_port": {
                    "type": "integer",
                    "description": "Local port to listen on"
                },
                "remote_port": {
                    "type": "integer",
                    "description": "Remote port to forward to"
                },
                "protocol": {
                    "type": "string",
                    "enum": ["tcp", "udp"],
                    "description": "Protocol to use"
                },
                "workload_type": {
                    "type": "string",
                    "enum": ["service", "pod", "proxy"],
                    "description": "Type of workload"
                },
                "kubeconfig": {
                    "type": "string",
                    "description": "Path to kubeconfig file"
                },
                "alias": {
                    "type": "string",
                    "description": "Friendly name for this configuration"
                },
                "remote_address": {
                    "type": "string",
                    "description": "Remote address for proxy workload type"
                },
                "local_address": {
                    "type": "string",
                    "description": "Local address to bind to"
                },
                "domain_enabled": {
                    "type": "boolean",
                    "description": "Whether to enable domain name resolution"
                }
            }),
            Some(vec!["config_id".to_string()]),
        )
    }

    async fn execute(&self, arguments: Option<Value>) -> CallToolResult {
        let args: UpdateConfigArgs = match arguments {
            Some(v) => match serde_json::from_value(v) {
                Ok(a) => a,
                Err(e) => return CallToolResult::error(format!("Invalid arguments: {e}")),
            },
            None => return CallToolResult::error("Missing required argument: config_id"),
        };

        // Get existing config
        let mut config = match kftray_commons::config::get_config(args.config_id).await {
            Ok(c) => c,
            Err(e) => return CallToolResult::error(format!("Config not found: {e}")),
        };

        // Update fields if provided
        if let Some(v) = args.context {
            config.context = Some(v);
        }
        if let Some(v) = args.namespace {
            config.namespace = v;
        }
        if args.service.is_some() {
            config.service = args.service;
        }
        if args.target.is_some() {
            config.target = args.target;
        }
        if let Some(v) = args.local_port {
            config.local_port = Some(v);
        }
        if let Some(v) = args.remote_port {
            config.remote_port = Some(v);
        }
        if let Some(v) = args.protocol {
            config.protocol = v;
        }
        if args.workload_type.is_some() {
            config.workload_type = args.workload_type;
        }
        if args.kubeconfig.is_some() {
            config.kubeconfig = args.kubeconfig;
        }
        if args.alias.is_some() {
            config.alias = args.alias;
        }
        if args.remote_address.is_some() {
            config.remote_address = args.remote_address;
        }
        if args.local_address.is_some() {
            config.local_address = args.local_address;
        }
        if args.domain_enabled.is_some() {
            config.domain_enabled = args.domain_enabled;
        }

        match kftray_commons::config::update_config(config).await {
            Ok(()) => {
                let response = UpdateConfigResponse {
                    success: true,
                    message: format!("Configuration {} updated successfully", args.config_id),
                };
                CallToolResult::json(&response).unwrap_or_else(|e| {
                    CallToolResult::error(format!("Failed to serialize response: {e}"))
                })
            }
            Err(e) => CallToolResult::error(format!("Failed to update config: {e}")),
        }
    }
}

// ============================================================================
// Delete Config Tool
// ============================================================================

pub struct DeleteConfigTool;

#[derive(Debug, Deserialize)]
struct DeleteConfigArgs {
    config_id: i64,
}

#[derive(Debug, Serialize)]
struct DeleteConfigResponse {
    success: bool,
    message: String,
}

#[async_trait::async_trait]
impl McpTool for DeleteConfigTool {
    fn definition(&self) -> Tool {
        Tool::with_schema(
            "delete_config",
            "Delete a port-forward configuration. If the port-forward is currently active, it will be stopped first.",
            json!({
                "config_id": {
                    "type": "integer",
                    "description": "The ID of the configuration to delete"
                }
            }),
            Some(vec!["config_id".to_string()]),
        )
    }

    async fn execute(&self, arguments: Option<Value>) -> CallToolResult {
        let args: DeleteConfigArgs = match arguments {
            Some(v) => match serde_json::from_value(v) {
                Ok(a) => a,
                Err(e) => return CallToolResult::error(format!("Invalid arguments: {e}")),
            },
            None => return CallToolResult::error("Missing required argument: config_id"),
        };

        // Try to stop if running (ignore errors)
        let _ = kftray_portforward::stop_port_forward(args.config_id.to_string()).await;

        match kftray_commons::config::delete_config(args.config_id).await {
            Ok(()) => {
                let response = DeleteConfigResponse {
                    success: true,
                    message: format!("Configuration {} deleted successfully", args.config_id),
                };
                CallToolResult::json(&response).unwrap_or_else(|e| {
                    CallToolResult::error(format!("Failed to serialize response: {e}"))
                })
            }
            Err(e) => CallToolResult::error(format!("Failed to delete config: {e}")),
        }
    }
}

// ============================================================================
// Export Configs Tool
// ============================================================================

pub struct ExportConfigsTool;

#[derive(Debug, Serialize)]
struct ExportConfigsResponse {
    configs_json: String,
    count: usize,
}

#[async_trait::async_trait]
impl McpTool for ExportConfigsTool {
    fn definition(&self) -> Tool {
        Tool::new(
            "export_configs",
            "Export all port-forward configurations as JSON. The exported JSON can be imported later or shared with others.",
        )
    }

    async fn execute(&self, _arguments: Option<Value>) -> CallToolResult {
        match kftray_commons::config::export_configs().await {
            Ok(json_str) => {
                // Count configs
                let count = serde_json::from_str::<Vec<Value>>(&json_str)
                    .map(|v| v.len())
                    .unwrap_or(0);

                let response = ExportConfigsResponse {
                    configs_json: json_str,
                    count,
                };
                CallToolResult::json(&response).unwrap_or_else(|e| {
                    CallToolResult::error(format!("Failed to serialize response: {e}"))
                })
            }
            Err(e) => CallToolResult::error(format!("Failed to export configs: {e}")),
        }
    }
}

// ============================================================================
// Import Configs Tool
// ============================================================================

pub struct ImportConfigsTool;

#[derive(Debug, Deserialize)]
struct ImportConfigsArgs {
    configs_json: String,
}

#[derive(Debug, Serialize)]
struct ImportConfigsResponse {
    success: bool,
    message: String,
}

#[async_trait::async_trait]
impl McpTool for ImportConfigsTool {
    fn definition(&self) -> Tool {
        Tool::with_schema(
            "import_configs",
            "Import port-forward configurations from JSON. Existing configurations with matching identity (same context, namespace, service/target, protocol) will be updated; new configurations will be created.",
            json!({
                "configs_json": {
                    "type": "string",
                    "description": "JSON string containing configuration(s) to import. Can be a single config object or an array of configs."
                }
            }),
            Some(vec!["configs_json".to_string()]),
        )
    }

    async fn execute(&self, arguments: Option<Value>) -> CallToolResult {
        let args: ImportConfigsArgs = match arguments {
            Some(v) => match serde_json::from_value(v) {
                Ok(a) => a,
                Err(e) => return CallToolResult::error(format!("Invalid arguments: {e}")),
            },
            None => return CallToolResult::error("Missing required argument: configs_json"),
        };

        match kftray_commons::config::import_configs(args.configs_json).await {
            Ok(()) => {
                let response = ImportConfigsResponse {
                    success: true,
                    message: "Configurations imported successfully".to_string(),
                };
                CallToolResult::json(&response).unwrap_or_else(|e| {
                    CallToolResult::error(format!("Failed to serialize response: {e}"))
                })
            }
            Err(e) => CallToolResult::error(format!("Failed to import configs: {e}")),
        }
    }
}

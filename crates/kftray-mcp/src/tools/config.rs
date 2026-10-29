//! Configuration management tools for MCP.
//!
//! These tools allow LLMs to manage port-forward configurations: listing,
//! creating, updating, deleting, and importing/exporting configurations.

use std::collections::BTreeMap;

use kftray_commons::models::config_model::Config;
use kftray_commons::models::ssh_provider::{
    SshProvider,
    SshProviderKind,
    WORKLOAD_SSH_PORT_FORWARD,
};
use kftray_commons::utils::config_view::{
    Condition,
    ConfigView,
};
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

const PROTOCOLS: &[&str] = &["tcp", "udp"];
const WORKLOAD_TYPES: &[&str] = &["service", "pod", "proxy", WORKLOAD_SSH_PORT_FORWARD];

fn check_choice(field: &str, value: Option<&str>, allowed: &[&str]) -> Result<(), CallToolResult> {
    match value {
        Some(value) if !allowed.contains(&value) => Err(CallToolResult::error(format!(
            "{field} must be one of {}, got '{value}'",
            allowed.join(", ")
        ))),
        _ => Ok(()),
    }
}

fn is_missing(value: Option<&str>) -> bool {
    value.is_none_or(str::is_empty)
}

fn is_blank(value: Option<&str>) -> bool {
    value.is_none_or(|value| value.trim().is_empty())
}

fn check_workload_fields(config: &Config) -> Result<(), CallToolResult> {
    let workload_type = config.workload_type.as_deref();
    if workload_type == Some(WORKLOAD_SSH_PORT_FORWARD) {
        let provider = config.ssh_provider.as_ref().ok_or_else(|| {
            CallToolResult::error(
                "ssh_provider is required when workload_type is 'ssh-port-forward'",
            )
        })?;
        provider
            .kind()
            .and_then(SshProviderKind::validate)
            .map_err(CallToolResult::error)?;
        if is_blank(config.remote_address.as_deref()) {
            return Err(CallToolResult::error(
                "remote_address is required when workload_type is 'ssh-port-forward'",
            ));
        }
        if config.protocol != "tcp" {
            return Err(CallToolResult::error(
                "protocol must be 'tcp' when workload_type is 'ssh-port-forward'",
            ));
        }
        return Ok(());
    }
    if config.ssh_provider.is_some() {
        return Err(CallToolResult::error(
            "ssh_provider is only valid when workload_type is 'ssh-port-forward'",
        ));
    }
    // No context check: a saved config without one uses the kubeconfig's
    // current context, and updating its alias must keep working.
    if matches!(workload_type, Some("service" | "pod" | "proxy"))
        && is_blank(Some(&config.namespace))
    {
        return Err(CallToolResult::error(
            "namespace is required for Kubernetes workloads",
        ));
    }
    match workload_type {
        Some("service") if is_blank(config.service.as_deref()) => Err(CallToolResult::error(
            "service is required when workload_type is 'service'",
        )),
        Some("pod") if is_blank(config.target.as_deref()) => Err(CallToolResult::error(
            "target (pod label selector) is required when workload_type is 'pod'",
        )),
        Some("proxy") if is_missing(config.remote_address.as_deref()) => Err(
            CallToolResult::error("remote_address is required when workload_type is 'proxy'"),
        ),
        _ => Ok(()),
    }
}

/// One JSON schema per SSH provider, offered as `oneOf` for `ssh_provider`.
pub(crate) fn ssh_provider_schemas() -> Vec<Value> {
    vec![json!({
        "type": "object",
        "description": "GCP Compute Engine VM reached through IAP, login with OS Login",
        "properties": {
            "type": { "const": "gcp-bastion" },
            "project": { "type": "string", "description": "GCP project id" },
            "zone": { "type": "string", "description": "Zone of the VM, e.g. us-central1-b" },
            "instance": { "type": "string", "description": "Name of the VM" },
            "interface": {
                "type": "string",
                "default": "nic0",
                "description": "Network interface IAP connects to"
            }
        },
        "required": ["type", "project", "zone", "instance"],
        "additionalProperties": false
    })]
}

// ============================================================================
// List Configs Tool
// ============================================================================

pub struct ListConfigsTool;

#[derive(Debug, Default, Deserialize)]
struct ListConfigsArgs {
    #[serde(default)]
    filters: Vec<String>,
}

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
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    tags: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
struct ListConfigsResponse {
    configs: Vec<ConfigSummary>,
    count: usize,
}

#[async_trait::async_trait]
impl McpTool for ListConfigsTool {
    fn definition(&self) -> Tool {
        Tool::with_schema(
            "list_configs",
            "List saved port-forward configurations. Returns a summary of each configuration including ID, alias, service, namespace, ports and tags. Pass filters to only list matching configurations.",
            json!({
                "filters": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Conditions that must all match, in the same syntax as kftui --filter: '<field>=<value>[,<value>...]' or '<field>' for 'field is set'. Fields are context, namespace, kubeconfig, workload_type, protocol and tag:<key>, e.g. ['tag:team=payments', 'namespace=api,web', 'tag:pinned']."
                }
            }),
            None,
        )
    }

    async fn execute(&self, arguments: Option<Value>) -> CallToolResult {
        let args: ListConfigsArgs = match arguments {
            Some(v) => match serde_json::from_value(v) {
                Ok(a) => a,
                Err(e) => return CallToolResult::error(format!("Invalid arguments: {e}")),
            },
            None => ListConfigsArgs::default(),
        };

        let mut view = ConfigView {
            group_by: None,
            filters: Vec::with_capacity(args.filters.len()),
        };
        for filter in &args.filters {
            match filter.parse::<Condition>() {
                Ok(condition) => view.filters.push(condition),
                Err(e) => return CallToolResult::error(format!("Invalid filter '{filter}': {e}")),
            }
        }

        match kftray_commons::config::get_configs().await {
            Ok(configs) => {
                let summaries: Vec<ConfigSummary> = configs
                    .into_iter()
                    .filter(|c| view.matches(c))
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
                            tags: c.tags,
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
            Ok(config) => CallToolResult::json(&config).unwrap_or_else(|e| {
                CallToolResult::error(format!("Failed to serialize config: {e}"))
            }),
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
    context: Option<String>,
    namespace: Option<String>,
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
    tags: Option<BTreeMap<String, String>>,
    ssh_provider: Option<SshProvider>,
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
                    "description": "Kubernetes context to use (required for service, pod and proxy)"
                },
                "namespace": {
                    "type": "string",
                    "description": "Kubernetes namespace (required for service, pod and proxy)"
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
                    "enum": PROTOCOLS,
                    "description": "Protocol to use. Defaults to 'tcp'."
                },
                "workload_type": {
                    "type": "string",
                    "enum": WORKLOAD_TYPES,
                    "description": "Type of workload. Defaults to 'service'."
                },
                "kubeconfig": {
                    "type": "string",
                    "description": "Optional path to a kubeconfig file or a folder of kubeconfig files"
                },
                "alias": {
                    "type": "string",
                    "description": "Optional friendly name for this configuration"
                },
                "remote_address": {
                    "type": "string",
                    "description": "Remote address for proxy and ssh-port-forward workload types"
                },
                "ssh_provider": {
                    "oneOf": ssh_provider_schemas(),
                    "description": "Required for ssh-port-forward: how to reach the SSH bastion"
                },
                "local_address": {
                    "type": "string",
                    "description": "Local address to bind to. Defaults to '127.0.0.1'."
                },
                "domain_enabled": {
                    "type": "boolean",
                    "description": "Whether to enable domain name resolution"
                },
                "tags": {
                    "type": "object",
                    "additionalProperties": { "type": "string" },
                    "description": "Optional tags for grouping and filtering, e.g. {\"team\": \"payments\", \"pinned\": \"\"}. An empty value makes a key-only tag. Keys may only contain lowercase letters, digits, '.', '_', '-' and '/'."
                }
            }),
            Some(vec!["remote_port".to_string()]),
        )
    }

    async fn execute(&self, arguments: Option<Value>) -> CallToolResult {
        let args: CreateConfigArgs = match arguments {
            Some(v) => match serde_json::from_value(v) {
                Ok(a) => a,
                Err(e) => return CallToolResult::error(format!("Invalid arguments: {e}")),
            },
            None => {
                return CallToolResult::error("Missing required argument: remote_port");
            }
        };

        let workload_type = args.workload_type.unwrap_or_else(|| "service".to_string());
        if let Err(error) = check_choice("protocol", args.protocol.as_deref(), PROTOCOLS)
            .and_then(|()| check_choice("workload_type", Some(&workload_type), WORKLOAD_TYPES))
        {
            return error;
        }
        if workload_type != WORKLOAD_SSH_PORT_FORWARD && is_blank(args.context.as_deref()) {
            return CallToolResult::error("context is required for Kubernetes workloads");
        }

        let config = Config {
            id: None,
            service: args.service,
            namespace: args.namespace.unwrap_or_default(),
            local_port: args.local_port,
            remote_port: Some(args.remote_port),
            context: args.context,
            workload_type: Some(workload_type),
            protocol: args.protocol.unwrap_or_else(|| "tcp".to_string()),
            remote_address: args.remote_address,
            ssh_provider: args.ssh_provider,
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
            exposure_protocol: None,
            cert_manager_enabled: None,
            cert_issuer: None,
            cert_issuer_kind: None,
            ingress_class: None,
            ingress_annotations: None,
            tags: args.tags.unwrap_or_default(),
        };

        if let Err(error) = check_workload_fields(&config) {
            return error;
        }

        match kftray_commons::config::insert_config(config).await {
            Ok(config_id) => {
                let response = CreateConfigResponse {
                    success: true,
                    message: "Configuration created successfully".to_string(),
                    config_id: Some(config_id),
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
    tags: Option<BTreeMap<String, String>>,
    ssh_provider: Option<SshProvider>,
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
                    "enum": PROTOCOLS,
                    "description": "Protocol to use"
                },
                "workload_type": {
                    "type": "string",
                    "enum": WORKLOAD_TYPES,
                    "description": "Type of workload"
                },
                "kubeconfig": {
                    "type": "string",
                    "description": "Path to a kubeconfig file or a folder of kubeconfig files"
                },
                "alias": {
                    "type": "string",
                    "description": "Friendly name for this configuration"
                },
                "remote_address": {
                    "type": "string",
                    "description": "Remote address for proxy and ssh-port-forward workload types"
                },
                "ssh_provider": {
                    "oneOf": ssh_provider_schemas(),
                    "description": "How to reach the SSH bastion of an ssh-port-forward config"
                },
                "local_address": {
                    "type": "string",
                    "description": "Local address to bind to"
                },
                "domain_enabled": {
                    "type": "boolean",
                    "description": "Whether to enable domain name resolution"
                },
                "tags": {
                    "type": "object",
                    "additionalProperties": { "type": "string" },
                    "description": "Replaces all tags of the configuration, e.g. {\"team\": \"payments\", \"pinned\": \"\"}. An empty object removes every tag."
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
        if let Err(error) =
            check_choice("protocol", args.protocol.as_deref(), PROTOCOLS).and_then(|()| {
                check_choice(
                    "workload_type",
                    args.workload_type.as_deref(),
                    WORKLOAD_TYPES,
                )
            })
        {
            return error;
        }

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
            // A provider only belongs to ssh configs, and a JSON null cannot
            // clear it, so switching away from ssh drops it.
            if config.workload_type.as_deref() != Some(WORKLOAD_SSH_PORT_FORWARD) {
                config.ssh_provider = None;
            }
        }
        if args.ssh_provider.is_some() {
            config.ssh_provider = args.ssh_provider;
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
        if let Some(tags) = args.tags {
            config.tags = tags;
        }

        if let Err(error) = check_workload_fields(&config) {
            return error;
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
            "Delete a port-forward configuration. If the port-forward is currently active, it will be stopped first. A configuration that another kftray or kftui process is forwarding is not deleted.",
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

        let id = args.config_id;
        let stopped = kftray_portforward::stop_port_forward(id.to_string()).await;
        // Deleting only removes the row, so a forward another process runs or
        // still owes cleanup for would be left with nothing to stop it by.
        let deleted = kftray_portforward::kube::delete_configs_if_idle(
            &[id],
            kftray_commons::utils::db_mode::DatabaseMode::File,
            || async move {
                kftray_commons::config::get_config(id).await?;
                kftray_commons::config::delete_config(id).await
            },
        )
        .await;

        match (deleted, stopped) {
            (Ok(()), _) => {
                let response = DeleteConfigResponse {
                    success: true,
                    message: format!("Configuration {id} deleted successfully"),
                };
                CallToolResult::json(&response).unwrap_or_else(|e| {
                    CallToolResult::error(format!("Failed to serialize response: {e}"))
                })
            }
            (Err(e), Err(stop)) => {
                CallToolResult::error(format!("Failed to delete config: {e} ({stop})"))
            }
            (Err(e), Ok(_)) => CallToolResult::error(format!("Failed to delete config: {e}")),
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

#[cfg(test)]
mod tests {
    use kftray_commons::models::ssh_provider::GcpBastionProvider;

    use super::*;

    fn ssh_config(provider: Option<SshProvider>) -> Config {
        Config {
            workload_type: Some(WORKLOAD_SSH_PORT_FORWARD.to_string()),
            protocol: "tcp".to_string(),
            context: None,
            namespace: String::new(),
            service: None,
            remote_address: Some("db.internal".to_string()),
            remote_port: Some(5432),
            ssh_provider: provider,
            ..Config::default()
        }
    }

    fn gcp_bastion() -> SshProvider {
        SshProvider::Known(SshProviderKind::GcpBastion(GcpBastionProvider {
            project: "my-project".to_string(),
            zone: "us-central1-b".to_string(),
            instance: "jumper".to_string(),
            interface: "nic0".to_string(),
        }))
    }

    #[test]
    fn ssh_provider_schemas_should_cover_every_provider_type() {
        let types: Vec<String> = ssh_provider_schemas()
            .iter()
            .filter_map(|schema| schema["properties"]["type"]["const"].as_str())
            .map(str::to_owned)
            .collect();
        assert_eq!(types, SshProviderKind::TYPE_NAMES);
    }

    #[test]
    fn ssh_config_without_provider_should_be_rejected() {
        assert!(check_workload_fields(&ssh_config(None)).is_err());
    }

    #[test]
    fn ssh_config_with_provider_should_not_need_context_or_namespace() {
        assert!(check_workload_fields(&ssh_config(Some(gcp_bastion()))).is_ok());
    }

    #[test]
    fn saved_service_config_without_context_should_stay_updatable() {
        let config = Config {
            workload_type: Some("service".to_string()),
            context: None,
            namespace: "default".to_string(),
            service: Some("api".to_string()),
            ..Config::default()
        };
        assert!(check_workload_fields(&config).is_ok());
    }

    #[tokio::test]
    async fn creating_a_service_config_without_context_should_be_rejected() {
        let result = CreateConfigTool
            .execute(Some(json!({
                "namespace": "default",
                "service": "api",
                "remote_port": 80
            })))
            .await;
        assert_eq!(result.is_error, Some(true));
    }

    #[test]
    fn provider_on_a_service_config_should_be_rejected() {
        let config = Config {
            workload_type: Some("service".to_string()),
            context: Some("kind".to_string()),
            namespace: "default".to_string(),
            service: Some("api".to_string()),
            ssh_provider: Some(gcp_bastion()),
            ..Config::default()
        };
        assert!(check_workload_fields(&config).is_err());
    }
}

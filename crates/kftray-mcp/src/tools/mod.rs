//! MCP Tools for kftray operations.
//!
//! This module provides all the tools that can be called by LLMs via the MCP
//! protocol.

pub mod config;
pub mod kubernetes;
pub mod portforward;

#[cfg(test)]
mod tests;

use serde_json::Value;

use crate::protocol::{
    CallToolResult,
    Tool,
};

/// Trait for implementing MCP tools
#[async_trait::async_trait]
pub trait McpTool: Send + Sync {
    /// Get the tool definition
    fn definition(&self) -> Tool;

    /// Execute the tool with the given arguments
    async fn execute(&self, arguments: Option<Value>) -> CallToolResult;
}

/// Get all available tools
pub fn get_all_tools() -> Vec<Tool> {
    vec![
        // Kubernetes discovery tools
        kubernetes::ListKubeContextsTool.definition(),
        kubernetes::ListNamespacesTool.definition(),
        kubernetes::ListServicesTool.definition(),
        kubernetes::ListPodsTool.definition(),
        kubernetes::ListPortsTool.definition(),
        // Port-forward management tools
        portforward::ListActivePortForwardsTool.definition(),
        portforward::StartPortForwardTool.definition(),
        portforward::StopPortForwardTool.definition(),
        portforward::StopAllPortForwardsTool.definition(),
        // Configuration management tools
        config::ListConfigsTool.definition(),
        config::GetConfigTool.definition(),
        config::CreateConfigTool.definition(),
        config::UpdateConfigTool.definition(),
        config::DeleteConfigTool.definition(),
        config::ExportConfigsTool.definition(),
        config::ImportConfigsTool.definition(),
    ]
}

/// Execute a tool by name
pub async fn execute_tool(name: &str, arguments: Option<Value>) -> CallToolResult {
    match name {
        // Kubernetes tools
        "list_kube_contexts" => kubernetes::ListKubeContextsTool.execute(arguments).await,
        "list_namespaces" => kubernetes::ListNamespacesTool.execute(arguments).await,
        "list_services" => kubernetes::ListServicesTool.execute(arguments).await,
        "list_pods" => kubernetes::ListPodsTool.execute(arguments).await,
        "list_ports" => kubernetes::ListPortsTool.execute(arguments).await,

        // Port-forward tools
        "list_active_port_forwards" => {
            portforward::ListActivePortForwardsTool
                .execute(arguments)
                .await
        }
        "start_port_forward" => portforward::StartPortForwardTool.execute(arguments).await,
        "stop_port_forward" => portforward::StopPortForwardTool.execute(arguments).await,
        "stop_all_port_forwards" => {
            portforward::StopAllPortForwardsTool
                .execute(arguments)
                .await
        }

        // Config tools
        "list_configs" => config::ListConfigsTool.execute(arguments).await,
        "get_config" => config::GetConfigTool.execute(arguments).await,
        "create_config" => config::CreateConfigTool.execute(arguments).await,
        "update_config" => config::UpdateConfigTool.execute(arguments).await,
        "delete_config" => config::DeleteConfigTool.execute(arguments).await,
        "export_configs" => config::ExportConfigsTool.execute(arguments).await,
        "import_configs" => config::ImportConfigsTool.execute(arguments).await,

        _ => CallToolResult::error(format!("Unknown tool: {name}")),
    }
}

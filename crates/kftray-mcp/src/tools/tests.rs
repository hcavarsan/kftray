//! Tests for MCP tools module.

use super::*;
use crate::protocol::ToolContent;

#[test]
fn test_get_all_tools_returns_expected_tools() {
    let tools = get_all_tools();

    // Check that we have a reasonable number of tools
    assert!(tools.len() >= 10, "Expected at least 10 tools, got {}", tools.len());

    // Check that each tool has a name and description
    for tool in &tools {
        assert!(!tool.name.is_empty(), "Tool name should not be empty");
        assert!(tool.description.is_some(), "Tool {} should have a description", tool.name);
    }

    // Check for specific expected tools
    let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();

    assert!(tool_names.contains(&"list_configs"), "Should have list_configs tool");
    assert!(tool_names.contains(&"get_config"), "Should have get_config tool");
    assert!(tool_names.contains(&"create_config"), "Should have create_config tool");
    assert!(tool_names.contains(&"update_config"), "Should have update_config tool");
    assert!(tool_names.contains(&"delete_config"), "Should have delete_config tool");
    assert!(tool_names.contains(&"export_configs"), "Should have export_configs tool");
    assert!(tool_names.contains(&"import_configs"), "Should have import_configs tool");
    assert!(
        tool_names.contains(&"list_active_port_forwards"),
        "Should have list_active_port_forwards tool"
    );
    assert!(
        tool_names.contains(&"start_port_forward"),
        "Should have start_port_forward tool"
    );
    assert!(tool_names.contains(&"stop_port_forward"), "Should have stop_port_forward tool");
    assert!(
        tool_names.contains(&"stop_all_port_forwards"),
        "Should have stop_all_port_forwards tool"
    );
    assert!(
        tool_names.contains(&"list_kube_contexts"),
        "Should have list_kube_contexts tool"
    );
    assert!(tool_names.contains(&"list_namespaces"), "Should have list_namespaces tool");
    assert!(tool_names.contains(&"list_services"), "Should have list_services tool");
    assert!(tool_names.contains(&"list_pods"), "Should have list_pods tool");
    assert!(tool_names.contains(&"list_ports"), "Should have list_ports tool");
}

#[test]
fn test_tool_definitions_have_valid_schemas() {
    let tools = get_all_tools();

    for tool in &tools {
        // All tools should have object type schema
        assert_eq!(
            tool.input_schema.schema_type, "object",
            "Tool {} should have object schema type",
            tool.name
        );

        // Check additionalProperties is set (typically false for strict validation)
        assert!(
            tool.input_schema.additional_properties.is_some(),
            "Tool {} should have additionalProperties defined",
            tool.name
        );
    }
}

#[tokio::test]
async fn test_execute_unknown_tool_returns_error() {
    let result = execute_tool("nonexistent_tool", None).await;

    assert!(result.is_error == Some(true), "Unknown tool should return error");

    // Check the error message
    if let Some(ToolContent::Text { text }) = result.content.first() {
        assert!(
            text.contains("Unknown tool"),
            "Error message should mention unknown tool"
        );
    } else {
        panic!("Expected text content in error result");
    }
}

#[tokio::test]
async fn test_execute_tool_with_invalid_arguments() {
    // Test get_config with missing config_id
    let result = execute_tool("get_config", None).await;

    assert!(result.is_error == Some(true), "Missing required args should return error");

    // Test get_config with invalid argument type
    let result = execute_tool(
        "get_config",
        Some(serde_json::json!({"config_id": "not_a_number"})),
    )
    .await;

    assert!(result.is_error == Some(true), "Invalid argument type should return error");
}

#[tokio::test]
async fn test_create_config_validation_service_required() {
    // workload_type service requires service field
    let result = execute_tool(
        "create_config",
        Some(serde_json::json!({
            "context": "test-context",
            "namespace": "default",
            "remote_port": 8080,
            "workload_type": "service"
            // missing "service" field
        })),
    )
    .await;

    assert!(result.is_error == Some(true));
    if let Some(ToolContent::Text { text }) = result.content.first() {
        assert!(
            text.contains("service") && text.contains("required"),
            "Error should mention service is required, got: {}",
            text
        );
    }
}

#[tokio::test]
async fn test_create_config_validation_target_required_for_pod() {
    // workload_type pod requires target field
    let result = execute_tool(
        "create_config",
        Some(serde_json::json!({
            "context": "test-context",
            "namespace": "default",
            "remote_port": 8080,
            "workload_type": "pod"
            // missing "target" field
        })),
    )
    .await;

    assert!(result.is_error == Some(true));
    if let Some(ToolContent::Text { text }) = result.content.first() {
        assert!(
            text.contains("target") && text.contains("required"),
            "Error should mention target is required, got: {}",
            text
        );
    }
}

#[tokio::test]
async fn test_create_config_validation_remote_address_required_for_proxy() {
    // workload_type proxy requires remote_address field
    let result = execute_tool(
        "create_config",
        Some(serde_json::json!({
            "context": "test-context",
            "namespace": "default",
            "remote_port": 8080,
            "workload_type": "proxy"
            // missing "remote_address" field
        })),
    )
    .await;

    assert!(result.is_error == Some(true));
    if let Some(ToolContent::Text { text }) = result.content.first() {
        assert!(
            text.contains("remote_address") && text.contains("required"),
            "Error should mention remote_address is required, got: {}",
            text
        );
    }
}

#[tokio::test]
async fn test_start_port_forward_validation() {
    // start_port_forward without config_id requires namespace and remote_port
    let result = execute_tool(
        "start_port_forward",
        Some(serde_json::json!({
            "context": "test-context"
            // missing namespace and remote_port
        })),
    )
    .await;

    assert!(result.is_error == Some(true));
    if let Some(ToolContent::Text { text }) = result.content.first() {
        assert!(
            text.contains("namespace") || text.contains("required"),
            "Error should mention missing required field, got: {}",
            text
        );
    }
}

#[tokio::test]
async fn test_start_port_forward_service_validation() {
    // workload_type service requires service field
    let result = execute_tool(
        "start_port_forward",
        Some(serde_json::json!({
            "namespace": "default",
            "remote_port": 8080,
            "workload_type": "service"
            // missing "service" field
        })),
    )
    .await;

    assert!(result.is_error == Some(true));
    if let Some(ToolContent::Text { text }) = result.content.first() {
        assert!(
            text.contains("service") && text.contains("required"),
            "Error should mention service is required, got: {}",
            text
        );
    }
}

#[tokio::test]
async fn test_start_port_forward_pod_validation() {
    // workload_type pod requires target field
    let result = execute_tool(
        "start_port_forward",
        Some(serde_json::json!({
            "namespace": "default",
            "remote_port": 8080,
            "workload_type": "pod"
            // missing "target" field
        })),
    )
    .await;

    assert!(result.is_error == Some(true));
    if let Some(ToolContent::Text { text }) = result.content.first() {
        assert!(
            text.contains("target") && text.contains("required"),
            "Error should mention target is required, got: {}",
            text
        );
    }
}

#[tokio::test]
async fn test_stop_port_forward_missing_config_id() {
    let result = execute_tool("stop_port_forward", None).await;

    assert!(result.is_error == Some(true));
    if let Some(ToolContent::Text { text }) = result.content.first() {
        assert!(
            text.contains("config_id"),
            "Error should mention missing config_id, got: {}",
            text
        );
    }
}

#[tokio::test]
async fn test_update_config_missing_config_id() {
    let result = execute_tool("update_config", None).await;

    assert!(result.is_error == Some(true));
    if let Some(ToolContent::Text { text }) = result.content.first() {
        assert!(
            text.contains("config_id"),
            "Error should mention missing config_id, got: {}",
            text
        );
    }
}

#[tokio::test]
async fn test_delete_config_missing_config_id() {
    let result = execute_tool("delete_config", None).await;

    assert!(result.is_error == Some(true));
    if let Some(ToolContent::Text { text }) = result.content.first() {
        assert!(
            text.contains("config_id"),
            "Error should mention missing config_id, got: {}",
            text
        );
    }
}

#[tokio::test]
async fn test_import_configs_missing_json() {
    let result = execute_tool("import_configs", None).await;

    assert!(result.is_error == Some(true));
    if let Some(ToolContent::Text { text }) = result.content.first() {
        assert!(
            text.contains("configs_json"),
            "Error should mention missing configs_json, got: {}",
            text
        );
    }
}

#[tokio::test]
async fn test_list_namespaces_missing_context() {
    let result = execute_tool("list_namespaces", None).await;

    assert!(result.is_error == Some(true));
    if let Some(ToolContent::Text { text }) = result.content.first() {
        assert!(
            text.contains("context"),
            "Error should mention missing context, got: {}",
            text
        );
    }
}

#[tokio::test]
async fn test_list_services_missing_required() {
    let result = execute_tool("list_services", None).await;

    assert!(result.is_error == Some(true));
}

#[tokio::test]
async fn test_list_pods_missing_required() {
    let result = execute_tool("list_pods", None).await;

    assert!(result.is_error == Some(true));
}

#[tokio::test]
async fn test_list_ports_missing_required() {
    let result = execute_tool("list_ports", None).await;

    assert!(result.is_error == Some(true));
}

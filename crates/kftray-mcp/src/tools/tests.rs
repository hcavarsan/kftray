//! Tests for MCP tools module.

use super::*;
use crate::protocol::ToolContent;

#[test]
fn test_get_all_tools_returns_expected_tools() {
    let tools = get_all_tools();

    // Check that we have a reasonable number of tools
    assert!(
        tools.len() >= 10,
        "Expected at least 10 tools, got {}",
        tools.len()
    );

    // Check that each tool has a name and description
    for tool in &tools {
        assert!(!tool.name.is_empty(), "Tool name should not be empty");
        assert!(
            tool.description.is_some(),
            "Tool {} should have a description",
            tool.name
        );
    }

    // Check for specific expected tools
    let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();

    let expected_tools = [
        "list_configs",
        "get_config",
        "create_config",
        "update_config",
        "delete_config",
        "export_configs",
        "import_configs",
        "list_active_port_forwards",
        "start_port_forward",
        "stop_port_forward",
        "stop_all_port_forwards",
        "list_kube_contexts",
        "list_namespaces",
        "list_services",
        "list_pods",
        "list_ports",
    ];

    for expected in expected_tools {
        assert!(
            tool_names.contains(&expected),
            "Should have {} tool",
            expected
        );
    }
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

    assert!(
        result.is_error == Some(true),
        "Unknown tool should return error"
    );

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

    assert!(
        result.is_error == Some(true),
        "Missing required args should return error"
    );

    // Test get_config with invalid argument type
    let result = execute_tool(
        "get_config",
        Some(serde_json::json!({"config_id": "not_a_number"})),
    )
    .await;

    assert!(
        result.is_error == Some(true),
        "Invalid argument type should return error"
    );
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
    if let Some(ToolContent::Text { text }) = result.content.first() {
        assert!(
            text.contains("context") || text.contains("namespace"),
            "Error should mention missing required field, got: {}",
            text
        );
    }
}

#[tokio::test]
async fn test_list_pods_missing_required() {
    let result = execute_tool("list_pods", None).await;

    assert!(result.is_error == Some(true));
    if let Some(ToolContent::Text { text }) = result.content.first() {
        assert!(
            text.contains("context") || text.contains("namespace"),
            "Error should mention missing required field, got: {}",
            text
        );
    }
}

#[tokio::test]
async fn test_list_ports_missing_required() {
    let result = execute_tool("list_ports", None).await;

    assert!(result.is_error == Some(true));
    if let Some(ToolContent::Text { text }) = result.content.first() {
        assert!(
            text.contains("context") || text.contains("namespace") || text.contains("service"),
            "Error should mention missing required field, got: {}",
            text
        );
    }
}

struct TempConfigDb {
    _db: kftray_commons::test_utils::TestDb,
    _env: kftray_commons::test_utils::EnvVarGuard,
    _dir: tempfile::TempDir,
}

async fn temp_config_db() -> TempConfigDb {
    let db = kftray_commons::test_utils::test_db().await;
    let dir = tempfile::tempdir().unwrap();
    let env =
        kftray_commons::test_utils::EnvVarGuard::set("KFTRAY_CONFIG", dir.path().to_str().unwrap());
    kftray_commons::utils::db::init().await.unwrap();
    kftray_commons::utils::migration::migrate_configs(None)
        .await
        .unwrap();
    TempConfigDb {
        _db: db,
        _env: env,
        _dir: dir,
    }
}

fn result_text(result: &CallToolResult) -> &str {
    match result.content.first() {
        Some(ToolContent::Text { text }) => text,
        _ => panic!("Expected text content"),
    }
}

fn result_json(result: &CallToolResult) -> serde_json::Value {
    assert_ne!(result.is_error, Some(true), "{}", result_text(result));
    serde_json::from_str(result_text(result)).unwrap()
}

async fn create_tagged(service: &str, namespace: &str, tags: serde_json::Value) -> i64 {
    let result = execute_tool(
        "create_config",
        Some(serde_json::json!({
            "context": "kind",
            "namespace": namespace,
            "service": service,
            "remote_port": 80,
            "tags": tags,
        })),
    )
    .await;
    result_json(&result)["config_id"].as_i64().unwrap()
}

async fn list_services(filters: serde_json::Value) -> Vec<String> {
    let result = execute_tool(
        "list_configs",
        Some(serde_json::json!({ "filters": filters })),
    )
    .await;
    result_json(&result)["configs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["service"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn config_tools_work_after_startup_on_a_fresh_config_dir() {
    let _db = kftray_commons::test_utils::test_db().await;
    let dir = tempfile::tempdir().unwrap();
    let _env =
        kftray_commons::test_utils::EnvVarGuard::set("KFTRAY_CONFIG", dir.path().to_str().unwrap());

    crate::init_database().await.unwrap();

    let result = execute_tool("list_configs", None).await;
    assert_eq!(result_json(&result)["configs"], serde_json::json!([]));
}

#[tokio::test]
async fn list_configs_filters_by_tags_and_fields_like_kftui() {
    let _db = temp_config_db().await;
    create_tagged(
        "billing",
        "api",
        serde_json::json!({"Team": " payments ", "pinned": ""}),
    )
    .await;
    create_tagged("ledger", "web", serde_json::json!({"team": "payments"})).await;
    create_tagged("search", "api", serde_json::json!({"team": "core"})).await;

    let all = execute_tool("list_configs", None).await;
    let all = result_json(&all);
    assert_eq!(all["count"], 3);
    let billing = all["configs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["service"] == "billing")
        .unwrap();
    assert_eq!(
        billing["tags"],
        serde_json::json!({"pinned": "", "team": "payments"})
    );

    assert_eq!(
        list_services(serde_json::json!(["tag:team=payments"])).await,
        ["billing", "ledger"]
    );
    assert_eq!(
        list_services(serde_json::json!(["tag:team=payments", "namespace=api"])).await,
        ["billing"]
    );
    assert_eq!(
        list_services(serde_json::json!(["tag:team=core,payments"])).await,
        ["billing", "ledger", "search"]
    );
    assert_eq!(
        list_services(serde_json::json!(["tag:pinned"])).await,
        ["billing"]
    );
    assert!(
        list_services(serde_json::json!(["tag:owner"]))
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn list_configs_rejects_an_unknown_filter_field() {
    let _db = temp_config_db().await;

    let result = execute_tool(
        "list_configs",
        Some(serde_json::json!({"filters": ["team=payments"]})),
    )
    .await;

    assert_eq!(result.is_error, Some(true));
    assert!(
        result_text(&result).starts_with("Invalid filter 'team=payments': unknown field 'team'"),
        "{}",
        result_text(&result)
    );
}

#[tokio::test]
async fn update_config_replaces_tags_only_when_given() {
    let _db = temp_config_db().await;
    let id = create_tagged("billing", "api", serde_json::json!({"team": "payments"})).await;
    let tags_of = |id: i64| async move {
        let result = execute_tool("get_config", Some(serde_json::json!({"config_id": id}))).await;
        result_json(&result)
            .get("tags")
            .cloned()
            .unwrap_or_default()
    };

    let result = execute_tool(
        "update_config",
        Some(serde_json::json!({"config_id": id, "tags": {"env": "dev", "pinned": ""}})),
    )
    .await;
    result_json(&result);
    assert_eq!(
        tags_of(id).await,
        serde_json::json!({"env": "dev", "pinned": ""})
    );

    let result = execute_tool(
        "update_config",
        Some(serde_json::json!({"config_id": id, "alias": "renamed"})),
    )
    .await;
    result_json(&result);
    assert_eq!(
        tags_of(id).await,
        serde_json::json!({"env": "dev", "pinned": ""})
    );

    let result = execute_tool(
        "update_config",
        Some(serde_json::json!({"config_id": id, "tags": {}})),
    )
    .await;
    result_json(&result);
    assert_eq!(tags_of(id).await, serde_json::Value::Null);
}

#[tokio::test]
async fn invalid_tags_are_rejected_without_saving() {
    let _db = temp_config_db().await;
    let id = create_tagged("billing", "api", serde_json::json!({"team": "payments"})).await;

    let created = execute_tool(
        "create_config",
        Some(serde_json::json!({
            "context": "kind",
            "namespace": "api",
            "service": "ledger",
            "remote_port": 80,
            "tags": {"bad key": "x"},
        })),
    )
    .await;
    assert_eq!(created.is_error, Some(true));
    assert!(
        result_text(&created).contains("tag key 'bad key' may only contain"),
        "{}",
        result_text(&created)
    );

    let updated = execute_tool(
        "update_config",
        Some(serde_json::json!({"config_id": id, "tags": {"team": "a=b"}})),
    )
    .await;
    assert_eq!(updated.is_error, Some(true));
    assert!(
        result_text(&updated).contains("cannot contain ',' or '='"),
        "{}",
        result_text(&updated)
    );

    assert_eq!(
        list_services(serde_json::json!(["tag:team=payments"])).await,
        ["billing"]
    );
    assert_eq!(list_services(serde_json::json!([])).await, ["billing"]);
}

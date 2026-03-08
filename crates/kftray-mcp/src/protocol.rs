//! MCP (Model Context Protocol) types for JSON-RPC communication.
//!
//! This module implements the MCP specification for streamable HTTP transport.
//! See: https://modelcontextprotocol.io/specification

use serde::{
    Deserialize,
    Serialize,
};
use serde_json::Value;

/// JSON-RPC version constant
pub const JSONRPC_VERSION: &str = "2.0";

/// MCP protocol version
pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// Server name for identification
pub const SERVER_NAME: &str = "kftray-mcp";

/// Server version
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

// ============================================================================
// JSON-RPC Base Types
// ============================================================================

/// JSON-RPC Request ID - can be string, number, or null
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum RequestId {
    String(String),
    Number(i64),
}

/// JSON-RPC Request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<RequestId>,
}

/// JSON-RPC Response
///
/// Note: The `id` field MUST be included per JSON-RPC 2.0 spec. When `None`,
/// it serializes as `null` (not omitted). This is required for proper
/// correlation with requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    /// The request ID. Must be included (serializes as `null` if `None`).
    pub id: Option<RequestId>,
}

/// JSON-RPC Error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// JSON-RPC Notification (no id field)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

// ============================================================================
// MCP Error Codes
// ============================================================================

/// Standard JSON-RPC error codes
pub mod error_codes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;
}

// ============================================================================
// MCP Initialize
// ============================================================================

/// Client capabilities sent during initialization
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClientCapabilities {
    #[serde(default)]
    pub roots: Option<RootsCapability>,
    #[serde(default)]
    pub sampling: Option<Value>,
    #[serde(default)]
    pub experimental: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RootsCapability {
    #[serde(default)]
    pub list_changed: bool,
}

/// Client info sent during initialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// Initialize request parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub protocol_version: String,
    pub capabilities: ClientCapabilities,
    pub client_info: ClientInfo,
}

/// Server capabilities returned during initialization
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolsCapability {
    #[serde(default)]
    pub list_changed: bool,
}

/// Server info returned during initialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

/// Initialize response result
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    pub server_info: ServerInfo,
}

// ============================================================================
// MCP Tools
// ============================================================================

/// Tool definition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: ToolInputSchema,
}

/// Tool input schema (JSON Schema)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInputSchema {
    #[serde(rename = "type")]
    pub schema_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
    #[serde(
        rename = "additionalProperties",
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_properties: Option<bool>,
}

/// List tools response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListToolsResult {
    pub tools: Vec<Tool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Call tool request parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolParams {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
}

/// Tool result content types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ToolContent {
    Text {
        text: String,
    },
    Image {
        data: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
    Resource {
        resource: Value,
    },
}

/// Call tool response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallToolResult {
    pub content: Vec<ToolContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

// ============================================================================
// Helper Implementations
// ============================================================================

impl JsonRpcResponse {
    /// Create a successful response
    pub fn success(id: Option<RequestId>, result: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }

    /// Create an error response
    pub fn error(id: Option<RequestId>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
            id,
        }
    }

    /// Create an error response with data
    pub fn error_with_data(
        id: Option<RequestId>, code: i32, message: impl Into<String>, data: Value,
    ) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: Some(data),
            }),
            id,
        }
    }
}

impl CallToolResult {
    /// Create a successful text result
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::Text {
                text: content.into(),
            }],
            is_error: None,
        }
    }

    /// Create a successful JSON result (serialized as text)
    pub fn json<T: Serialize>(value: &T) -> Result<Self, serde_json::Error> {
        let text = serde_json::to_string_pretty(value)?;
        Ok(Self::text(text))
    }

    /// Create an error result
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::Text {
                text: message.into(),
            }],
            is_error: Some(true),
        }
    }
}

impl Tool {
    /// Create a new tool with no required parameters
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: Some(description.into()),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: Some(serde_json::json!({})),
                required: None,
                additional_properties: Some(false),
            },
        }
    }

    /// Create a new tool with input schema
    pub fn with_schema(
        name: impl Into<String>, description: impl Into<String>, properties: Value,
        required: Option<Vec<String>>,
    ) -> Self {
        Self {
            name: name.into(),
            description: Some(description.into()),
            input_schema: ToolInputSchema {
                schema_type: "object".to_string(),
                properties: Some(properties),
                required,
                additional_properties: Some(false),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_rpc_request_parsing() {
        let json = r#"{
            "jsonrpc": "2.0",
            "method": "initialize",
            "params": {},
            "id": 1
        }"#;

        let request: JsonRpcRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.method, "initialize");
        assert_eq!(request.id, Some(RequestId::Number(1)));
    }

    #[test]
    fn test_json_rpc_request_with_string_id() {
        let json = r#"{
            "jsonrpc": "2.0",
            "method": "test",
            "id": "abc-123"
        }"#;

        let request: JsonRpcRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.id, Some(RequestId::String("abc-123".to_string())));
    }

    #[test]
    fn test_json_rpc_request_without_id_is_notification() {
        let json = r#"{
            "jsonrpc": "2.0",
            "method": "notify"
        }"#;

        let request: JsonRpcRequest = serde_json::from_str(json).unwrap();
        assert!(request.id.is_none());
        assert!(request.params.is_none());
    }

    #[test]
    fn test_json_rpc_response_success() {
        let response = JsonRpcResponse::success(
            Some(RequestId::Number(1)),
            serde_json::json!({"status": "ok"}),
        );

        assert!(response.error.is_none());
        assert!(response.result.is_some());
    }

    #[test]
    fn test_json_rpc_response_error() {
        let response = JsonRpcResponse::error(
            Some(RequestId::Number(1)),
            error_codes::METHOD_NOT_FOUND,
            "Method not found",
        );

        assert!(response.result.is_none());
        assert!(response.error.is_some());
        assert_eq!(response.error.unwrap().code, error_codes::METHOD_NOT_FOUND);
    }

    #[test]
    fn test_json_rpc_response_error_with_data() {
        let response = JsonRpcResponse::error_with_data(
            Some(RequestId::Number(1)),
            error_codes::INVALID_PARAMS,
            "Invalid params",
            serde_json::json!({"field": "name", "reason": "required"}),
        );

        assert!(response.error.is_some());
        let error = response.error.unwrap();
        assert_eq!(error.code, error_codes::INVALID_PARAMS);
        assert!(error.data.is_some());
    }

    #[test]
    fn test_json_rpc_response_id_serializes_as_null_when_none() {
        // JSON-RPC 2.0 spec: id MUST be included, serialized as null when None
        let response = JsonRpcResponse::success(None, serde_json::json!({}));

        let json = serde_json::to_string(&response).unwrap();
        assert!(
            json.contains(r#""id":null"#),
            "id should serialize as null, got: {}",
            json
        );
    }

    #[test]
    fn test_json_rpc_response_id_serializes_with_number() {
        let response = JsonRpcResponse::success(Some(RequestId::Number(42)), serde_json::json!({}));

        let json = serde_json::to_string(&response).unwrap();
        assert!(
            json.contains(r#""id":42"#),
            "id should serialize as 42, got: {}",
            json
        );
    }

    #[test]
    fn test_json_rpc_response_id_serializes_with_string() {
        let response = JsonRpcResponse::success(
            Some(RequestId::String("req-1".to_string())),
            serde_json::json!({}),
        );

        let json = serde_json::to_string(&response).unwrap();
        assert!(
            json.contains(r#""id":"req-1""#),
            "id should serialize as string, got: {}",
            json
        );
    }

    #[test]
    fn test_tool_result_text() {
        let result = CallToolResult::text("Hello, World!");
        assert_eq!(result.content.len(), 1);
        assert!(result.is_error.is_none());
    }

    #[test]
    fn test_tool_result_error() {
        let result = CallToolResult::error("Something went wrong");
        assert_eq!(result.is_error, Some(true));
    }

    #[test]
    fn test_tool_result_json() {
        #[derive(serde::Serialize)]
        struct TestData {
            name: String,
            count: i32,
        }

        let data = TestData {
            name: "test".to_string(),
            count: 42,
        };

        let result = CallToolResult::json(&data).unwrap();
        assert_eq!(result.content.len(), 1);
        assert!(result.is_error.is_none());

        if let ToolContent::Text { text } = &result.content[0] {
            assert!(text.contains("\"name\""));
            assert!(text.contains("\"test\""));
            assert!(text.contains("42"));
        } else {
            panic!("Expected text content");
        }
    }

    #[test]
    fn test_tool_new() {
        let tool = Tool::new("my_tool", "A test tool");
        assert_eq!(tool.name, "my_tool");
        assert_eq!(tool.description, Some("A test tool".to_string()));
        assert_eq!(tool.input_schema.schema_type, "object");
        assert!(tool.input_schema.required.is_none());
    }

    #[test]
    fn test_tool_with_schema() {
        let tool = Tool::with_schema(
            "create_item",
            "Create a new item",
            serde_json::json!({
                "name": { "type": "string" },
                "count": { "type": "integer" }
            }),
            Some(vec!["name".to_string()]),
        );

        assert_eq!(tool.name, "create_item");
        assert!(tool.input_schema.properties.is_some());
        assert_eq!(tool.input_schema.required, Some(vec!["name".to_string()]));
    }

    #[test]
    fn test_initialize_params_parsing() {
        let json = r#"{
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "test-client",
                "version": "1.0.0"
            }
        }"#;

        let params: InitializeParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.protocol_version, "2024-11-05");
        assert_eq!(params.client_info.name, "test-client");
    }

    #[test]
    fn test_initialize_result_serialization() {
        let result = InitializeResult {
            protocol_version: MCP_PROTOCOL_VERSION.to_string(),
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability {
                    list_changed: false,
                }),
                resources: None,
                prompts: None,
                logging: None,
                experimental: None,
            },
            server_info: ServerInfo {
                name: SERVER_NAME.to_string(),
                version: SERVER_VERSION.to_string(),
            },
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("protocolVersion"));
        assert!(json.contains("serverInfo"));
        assert!(json.contains("capabilities"));
    }

    #[test]
    fn test_list_tools_result_serialization() {
        let result = ListToolsResult {
            tools: vec![Tool::new("test", "Test tool")],
            next_cursor: None,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("tools"));
        assert!(!json.contains("nextCursor")); // should be skipped when None
    }

    #[test]
    fn test_call_tool_params_parsing() {
        let json = r#"{
            "name": "list_configs",
            "arguments": {"filter": "active"}
        }"#;

        let params: CallToolParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.name, "list_configs");
        assert!(params.arguments.is_some());
    }

    #[test]
    fn test_call_tool_params_without_arguments() {
        let json = r#"{"name": "list_all"}"#;

        let params: CallToolParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.name, "list_all");
        assert!(params.arguments.is_none());
    }

    #[test]
    fn test_tool_content_text_serialization() {
        let content = ToolContent::Text {
            text: "Hello".to_string(),
        };

        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains(r#""type":"text""#));
        assert!(json.contains(r#""text":"Hello""#));
    }

    #[test]
    fn test_error_codes_values() {
        assert_eq!(error_codes::PARSE_ERROR, -32700);
        assert_eq!(error_codes::INVALID_REQUEST, -32600);
        assert_eq!(error_codes::METHOD_NOT_FOUND, -32601);
        assert_eq!(error_codes::INVALID_PARAMS, -32602);
        assert_eq!(error_codes::INTERNAL_ERROR, -32603);
    }

    #[test]
    fn test_constants() {
        assert_eq!(JSONRPC_VERSION, "2.0");
        assert_eq!(MCP_PROTOCOL_VERSION, "2024-11-05");
        assert_eq!(SERVER_NAME, "kftray-mcp");
        assert!(!SERVER_VERSION.is_empty());
    }

    // Wire-format tests to ensure correct MCP key casing

    #[test]
    fn test_roots_capability_serializes_camel_case() {
        let roots = RootsCapability { list_changed: true };
        let value = serde_json::to_value(&roots).unwrap();
        assert_eq!(
            value.get("listChanged").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert!(value.get("list_changed").is_none());
    }

    #[test]
    fn test_tools_capability_serializes_camel_case() {
        let tools = ToolsCapability { list_changed: true };
        let value = serde_json::to_value(&tools).unwrap();
        assert_eq!(
            value.get("listChanged").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert!(value.get("list_changed").is_none());
    }

    #[test]
    fn test_tool_content_image_serializes_mime_type_camel_case() {
        let content = ToolContent::Image {
            data: "abc".to_string(),
            mime_type: "image/png".to_string(),
        };
        let value = serde_json::to_value(&content).unwrap();
        assert_eq!(
            value.get("mimeType").and_then(|v| v.as_str()),
            Some("image/png")
        );
        assert!(value.get("mime_type").is_none());
    }

    #[test]
    fn test_list_tools_result_next_cursor_camel_case() {
        let result = ListToolsResult {
            tools: vec![],
            next_cursor: Some("cursor123".to_string()),
        };
        let value = serde_json::to_value(&result).unwrap();
        assert_eq!(
            value.get("nextCursor").and_then(|v| v.as_str()),
            Some("cursor123")
        );
        assert!(value.get("next_cursor").is_none());
    }

    #[test]
    fn test_params_omitted_when_none() {
        let request = JsonRpcRequest {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: "test".to_string(),
            params: None,
            id: Some(RequestId::Number(1)),
        };
        let json = serde_json::to_string(&request).unwrap();
        assert!(
            !json.contains("params"),
            "params should be omitted when None, got: {}",
            json
        );
    }

    #[test]
    fn test_arguments_omitted_when_none() {
        let params = CallToolParams {
            name: "test".to_string(),
            arguments: None,
        };
        let json = serde_json::to_string(&params).unwrap();
        assert!(
            !json.contains("arguments"),
            "arguments should be omitted when None, got: {}",
            json
        );
    }
}

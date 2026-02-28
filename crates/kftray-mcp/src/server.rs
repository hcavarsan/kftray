//! Streamable HTTP MCP Server implementation.
//!
//! This module implements the MCP Streamable HTTP transport as specified in:
//! https://modelcontextprotocol.io/specification/2024-11-05/basic/transports#streamable-http
//!
//! The server supports:
//! - POST /mcp for JSON-RPC requests (returns JSON or SSE stream)
//! - GET /mcp for SSE event stream (server-to-client notifications)
//! - DELETE /mcp to terminate session

use crate::protocol::{
    CallToolParams, InitializeParams, InitializeResult, JsonRpcRequest,
    JsonRpcResponse, ListToolsResult, RequestId, ServerCapabilities, ServerInfo, ToolsCapability,
    error_codes, MCP_PROTOCOL_VERSION, SERVER_NAME, SERVER_VERSION,
};
use crate::tools;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Session state
#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub initialized: bool,
    pub client_info: Option<String>,
}

/// Server state
pub struct ServerState {
    pub sessions: RwLock<HashMap<String, Session>>,
}

impl ServerState {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    pub async fn create_session(&self) -> String {
        let session_id = Uuid::new_v4().to_string();
        let session = Session {
            id: session_id.clone(),
            initialized: false,
            client_info: None,
        };
        self.sessions.write().await.insert(session_id.clone(), session);
        session_id
    }

    pub async fn get_session(&self, session_id: &str) -> Option<Session> {
        self.sessions.read().await.get(session_id).cloned()
    }

    pub async fn update_session(&self, session: Session) {
        self.sessions.write().await.insert(session.id.clone(), session);
    }

    pub async fn remove_session(&self, session_id: &str) {
        self.sessions.write().await.remove(session_id);
    }
}

impl Default for ServerState {
    fn default() -> Self {
        Self::new()
    }
}

/// Start the MCP HTTP server
pub async fn start_server(addr: SocketAddr) -> anyhow::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    info!("MCP server listening on http://{}", addr);

    let state = Arc::new(ServerState::new());

    loop {
        let (stream, remote_addr) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let state = Arc::clone(&state);

        tokio::spawn(async move {
            let service = service_fn(move |req| {
                let state = Arc::clone(&state);
                async move { handle_request(req, state).await }
            });

            if let Err(err) = http1::Builder::new().serve_connection(io, service).await
                && !err.is_incomplete_message() {
                    error!("Error serving connection from {}: {:?}", remote_addr, err);
                }
        });
    }
}

/// Handle incoming HTTP requests
async fn handle_request(
    req: Request<Incoming>,
    state: Arc<ServerState>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let method = req.method().clone();
    let path = req.uri().path().to_string();

    debug!("Request: {} {}", method, path);

    let response = match (method, path.as_str()) {
        (Method::POST, "/mcp") => handle_mcp_post(req, state).await,
        (Method::GET, "/mcp") => handle_mcp_get(req, state).await,
        (Method::DELETE, "/mcp") => handle_mcp_delete(req, state).await,
        (Method::GET, "/health") => handle_health().await,
        (Method::OPTIONS, _) => handle_cors_preflight().await,
        _ => not_found(),
    };

    // Add CORS headers
    add_cors_headers(response)
}

/// Handle POST /mcp - JSON-RPC requests
async fn handle_mcp_post(
    req: Request<Incoming>,
    state: Arc<ServerState>,
) -> Response<Full<Bytes>> {
    // Get or create session
    let session_id = req
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    let session_id = match session_id {
        Some(id) => {
            if state.get_session(&id).await.is_some() {
                id
            } else {
                state.create_session().await
            }
        }
        None => state.create_session().await,
    };

    // Read body
    let body_bytes = match req.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            return json_rpc_error_response(
                None,
                error_codes::PARSE_ERROR,
                format!("Failed to read body: {e}"),
            );
        }
    };

    // Parse JSON-RPC request
    let rpc_request: JsonRpcRequest = match serde_json::from_slice(&body_bytes) {
        Ok(req) => req,
        Err(e) => {
            return json_rpc_error_response(
                None,
                error_codes::PARSE_ERROR,
                format!("Invalid JSON: {e}"),
            );
        }
    };

    // Handle the request
    let response = handle_json_rpc_request(rpc_request, &session_id, &state).await;

    // Build HTTP response with session header
    let json_body = serde_json::to_vec(&response).unwrap_or_default();

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .header("Mcp-Session-Id", &session_id)
        .body(Full::new(Bytes::from(json_body)))
        .unwrap()
}

/// Handle GET /mcp - SSE event stream
async fn handle_mcp_get(
    req: Request<Incoming>,
    state: Arc<ServerState>,
) -> Response<Full<Bytes>> {
    let session_id = req
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok());

    match session_id {
        Some(id) => {
            if state.get_session(id).await.is_some() {
                // For now, return a simple SSE response that keeps connection open
                // In a full implementation, this would stream events
                let sse_body = format!(
                    "event: connected\ndata: {{\"sessionId\":\"{}\"}}\n\n",
                    id
                );

                Response::builder()
                    .status(StatusCode::OK)
                    .header("Content-Type", "text/event-stream")
                    .header("Cache-Control", "no-cache")
                    .header("Connection", "keep-alive")
                    .header("Mcp-Session-Id", id)
                    .body(Full::new(Bytes::from(sse_body)))
                    .unwrap()
            } else {
                Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Full::new(Bytes::from("Session not found")))
                    .unwrap()
            }
        }
        None => Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Full::new(Bytes::from("Missing Mcp-Session-Id header")))
            .unwrap(),
    }
}

/// Handle DELETE /mcp - terminate session
async fn handle_mcp_delete(
    req: Request<Incoming>,
    state: Arc<ServerState>,
) -> Response<Full<Bytes>> {
    let session_id = req
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok());

    match session_id {
        Some(id) => {
            state.remove_session(id).await;
            info!("Session terminated: {}", id);
            Response::builder()
                .status(StatusCode::OK)
                .body(Full::new(Bytes::from("{\"status\":\"terminated\"}")))
                .unwrap()
        }
        None => Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .body(Full::new(Bytes::from("Missing Mcp-Session-Id header")))
            .unwrap(),
    }
}

/// Handle health check endpoint
async fn handle_health() -> Response<Full<Bytes>> {
    let health = serde_json::json!({
        "status": "healthy",
        "server": SERVER_NAME,
        "version": SERVER_VERSION,
        "protocol_version": MCP_PROTOCOL_VERSION,
    });

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(serde_json::to_vec(&health).unwrap())))
        .unwrap()
}

/// Handle CORS preflight requests
async fn handle_cors_preflight() -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(Full::new(Bytes::new()))
        .unwrap()
}

/// Return 404 Not Found
fn not_found() -> Response<Full<Bytes>> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Full::new(Bytes::from("Not Found")))
        .unwrap()
}

/// Add CORS headers to response
fn add_cors_headers(
    response: Response<Full<Bytes>>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
    let (mut parts, body) = response.into_parts();

    parts.headers.insert(
        "Access-Control-Allow-Origin",
        "*".parse().unwrap(),
    );
    parts.headers.insert(
        "Access-Control-Allow-Methods",
        "GET, POST, DELETE, OPTIONS".parse().unwrap(),
    );
    parts.headers.insert(
        "Access-Control-Allow-Headers",
        "Content-Type, Mcp-Session-Id".parse().unwrap(),
    );
    parts.headers.insert(
        "Access-Control-Expose-Headers",
        "Mcp-Session-Id".parse().unwrap(),
    );

    Ok(Response::from_parts(parts, body))
}

/// Create a JSON-RPC error response
fn json_rpc_error_response(
    id: Option<RequestId>,
    code: i32,
    message: String,
) -> Response<Full<Bytes>> {
    let response = JsonRpcResponse::error(id, code, message);
    let json_body = serde_json::to_vec(&response).unwrap_or_default();

    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Full::new(Bytes::from(json_body)))
        .unwrap()
}

/// Handle a JSON-RPC request
async fn handle_json_rpc_request(
    request: JsonRpcRequest,
    session_id: &str,
    state: &ServerState,
) -> JsonRpcResponse {
    let method = request.method.as_str();
    let id = request.id.clone();

    debug!("Handling method: {}", method);

    match method {
        "initialize" => handle_initialize(request, session_id, state).await,
        "initialized" => {
            // Notification, no response needed but we'll acknowledge
            debug!("Client initialized notification received");
            JsonRpcResponse::success(id, serde_json::json!({}))
        }
        "ping" => JsonRpcResponse::success(id, serde_json::json!({})),
        "tools/list" => handle_list_tools(request).await,
        "tools/call" => handle_call_tool(request).await,
        "notifications/initialized" => {
            // Notification
            JsonRpcResponse::success(id, serde_json::json!({}))
        }
        _ => {
            warn!("Unknown method: {}", method);
            JsonRpcResponse::error(
                id,
                error_codes::METHOD_NOT_FOUND,
                format!("Method not found: {method}"),
            )
        }
    }
}

/// Handle initialize request
async fn handle_initialize(
    request: JsonRpcRequest,
    session_id: &str,
    state: &ServerState,
) -> JsonRpcResponse {
    let params: InitializeParams = match request.params {
        Some(p) => match serde_json::from_value(p) {
            Ok(params) => params,
            Err(e) => {
                return JsonRpcResponse::error(
                    request.id,
                    error_codes::INVALID_PARAMS,
                    format!("Invalid initialize params: {e}"),
                )
            }
        },
        None => {
            return JsonRpcResponse::error(
                request.id,
                error_codes::INVALID_PARAMS,
                "Missing initialize params",
            )
        }
    };

    info!(
        "Client connecting: {} (protocol: {})",
        params.client_info.name, params.protocol_version
    );

    // Update session state
    if let Some(mut session) = state.get_session(session_id).await {
        session.initialized = true;
        session.client_info = Some(params.client_info.name.clone());
        state.update_session(session).await;
    }

    let result = InitializeResult {
        protocol_version: MCP_PROTOCOL_VERSION.to_string(),
        capabilities: ServerCapabilities {
            tools: Some(ToolsCapability { list_changed: false }),
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

    JsonRpcResponse::success(request.id, serde_json::to_value(result).unwrap())
}

/// Handle tools/list request
async fn handle_list_tools(request: JsonRpcRequest) -> JsonRpcResponse {
    let tools = tools::get_all_tools();
    let result = ListToolsResult {
        tools,
        next_cursor: None,
    };

    JsonRpcResponse::success(request.id, serde_json::to_value(result).unwrap())
}

/// Handle tools/call request
async fn handle_call_tool(request: JsonRpcRequest) -> JsonRpcResponse {
    let params: CallToolParams = match request.params {
        Some(p) => match serde_json::from_value(p) {
            Ok(params) => params,
            Err(e) => {
                return JsonRpcResponse::error(
                    request.id,
                    error_codes::INVALID_PARAMS,
                    format!("Invalid call tool params: {e}"),
                )
            }
        },
        None => {
            return JsonRpcResponse::error(
                request.id,
                error_codes::INVALID_PARAMS,
                "Missing tool call params",
            )
        }
    };

    debug!("Calling tool: {} with args: {:?}", params.name, params.arguments);

    let result = tools::execute_tool(&params.name, params.arguments).await;

    JsonRpcResponse::success(request.id, serde_json::to_value(result).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_server_state() {
        let state = ServerState::new();

        // Create session
        let session_id = state.create_session().await;
        assert!(!session_id.is_empty());

        // Get session
        let session = state.get_session(&session_id).await;
        assert!(session.is_some());
        assert!(!session.unwrap().initialized);

        // Update session
        let mut session = state.get_session(&session_id).await.unwrap();
        session.initialized = true;
        state.update_session(session).await;

        let updated = state.get_session(&session_id).await.unwrap();
        assert!(updated.initialized);

        // Remove session
        state.remove_session(&session_id).await;
        assert!(state.get_session(&session_id).await.is_none());
    }

    #[tokio::test]
    async fn test_handle_initialize() {
        let state = ServerState::new();
        let session_id = state.create_session().await;

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "test-client",
                    "version": "1.0.0"
                }
            })),
            id: Some(RequestId::Number(1)),
        };

        let response = handle_initialize(request, &session_id, &state).await;
        assert!(response.error.is_none());
        assert!(response.result.is_some());

        let result: InitializeResult =
            serde_json::from_value(response.result.unwrap()).unwrap();
        assert_eq!(result.server_info.name, SERVER_NAME);
    }

    #[tokio::test]
    async fn test_handle_list_tools() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "tools/list".to_string(),
            params: None,
            id: Some(RequestId::Number(1)),
        };

        let response = handle_list_tools(request).await;
        assert!(response.error.is_none());

        let result: ListToolsResult =
            serde_json::from_value(response.result.unwrap()).unwrap();
        assert!(!result.tools.is_empty());
    }
}

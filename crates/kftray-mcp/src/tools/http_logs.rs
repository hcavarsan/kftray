use kftray_http_logs::config::HTTP_LOG_EXTENSION;
use kftray_http_logs::entries::{
    HttpLogEntry,
    parse_http_logs,
};
use serde::Deserialize;
use serde_json::{
    Value,
    json,
};

use crate::protocol::{
    CallToolResult,
    Tool,
};
use crate::tools::McpTool;

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 200;
const MAX_BODY_BYTES: usize = 16 * 1024;
const MASKED_HEADERS: [&str; 4] = [
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
];

pub struct GetHttpLogsTool;

#[derive(Debug, Deserialize)]
struct GetHttpLogsArgs {
    config_id: i64,
    limit: Option<usize>,
}

pub(crate) fn mask_secret_headers(headers: &[String]) -> Vec<String> {
    headers
        .iter()
        .map(|header| match header.split_once(':') {
            Some((name, _))
                if MASKED_HEADERS.contains(&name.trim().to_ascii_lowercase().as_str()) =>
            {
                format!("{name}: ***")
            }
            _ => header.clone(),
        })
        .collect()
}

pub(crate) fn capped_body(body: &str) -> String {
    if body.len() <= MAX_BODY_BYTES {
        return body.to_owned();
    }
    let mut end = MAX_BODY_BYTES;
    while !body.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n[truncated, showing {end} of {} bytes]",
        &body[..end],
        body.len()
    )
}

fn entry_json(entry: &HttpLogEntry) -> Value {
    json!({
        "trace_id": entry.trace_id,
        "method": entry.method,
        "path": entry.path,
        "status": entry.status_code.as_deref().and_then(|status| status.parse::<u16>().ok()),
        "duration_ms": entry
            .duration_ms
            .as_deref()
            .and_then(|took| took.trim_end_matches(" ms").parse::<i64>().ok()),
        "request_at": entry.request_timestamp,
        "response_at": entry.response_timestamp,
        "request_headers": mask_secret_headers(&entry.request_headers),
        "request_body": capped_body(&entry.request_body),
        "response_headers": mask_secret_headers(&entry.response_headers),
        "response_body": capped_body(&entry.response_body),
    })
}

#[async_trait::async_trait]
impl McpTool for GetHttpLogsTool {
    fn definition(&self) -> Tool {
        Tool::with_schema(
            "get_http_logs",
            "Get the last HTTP requests and responses kftray captured on the forward of a configuration, oldest first. Capture must be on for the config (update_config with http_logs_enabled). Authorization and cookie header values are masked.",
            json!({
                "config_id": {
                    "type": "integer",
                    "description": "The ID of the configuration whose captured requests to return"
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_LIMIT,
                    "description": format!("How many of the most recent requests to return (default {DEFAULT_LIMIT}, at most {MAX_LIMIT})")
                }
            }),
            Some(vec!["config_id".to_string()]),
        )
    }

    async fn execute(&self, arguments: Option<Value>) -> CallToolResult {
        let args: GetHttpLogsArgs = match arguments {
            Some(v) => match serde_json::from_value(v) {
                Ok(a) => a,
                Err(e) => return CallToolResult::error(format!("Invalid arguments: {e}")),
            },
            None => return CallToolResult::error("Missing required argument: config_id"),
        };

        let config = match kftray_commons::config::get_config(args.config_id).await {
            Ok(config) => config,
            Err(e) => return CallToolResult::error(format!("Config not found: {e}")),
        };
        let Some(local_port) = config.local_port else {
            return CallToolResult::error(format!(
                "Config {} has no local port, so kftray has no HTTP log file for it",
                args.config_id
            ));
        };
        let log_dir = match kftray_commons::utils::config_dir::get_log_folder_path() {
            Ok(dir) => dir,
            Err(e) => {
                return CallToolResult::error(format!("Failed to find the HTTP log folder: {e}"));
            }
        };
        let log_file = log_dir.join(format!(
            "{}_{local_port}.{HTTP_LOG_EXTENSION}",
            args.config_id
        ));

        let content = match tokio::fs::read_to_string(&log_file).await {
            Ok(content) => content,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return CallToolResult::error(format!(
                    "No HTTP logs captured for config {} yet ({}). Capture may be off: call \
                     update_config with http_logs_enabled true, start the port forward and send \
                     requests through it.",
                    args.config_id,
                    log_file.display()
                ));
            }
            Err(e) => {
                return CallToolResult::error(format!(
                    "Failed to read HTTP log file {}: {e}",
                    log_file.display()
                ));
            }
        };

        let lines: Vec<String> = content.lines().map(str::to_string).collect();
        let entries = parse_http_logs(&lines);
        let limit = args.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT);
        let recent = &entries[entries.len().saturating_sub(limit)..];

        CallToolResult::json(&json!({
            "config_id": args.config_id,
            "log_file": log_file,
            "total_captured": entries.len(),
            "requests": recent.iter().map(entry_json).collect::<Vec<_>>(),
        }))
        .unwrap_or_else(|e| CallToolResult::error(format!("Failed to serialize HTTP logs: {e}")))
    }
}

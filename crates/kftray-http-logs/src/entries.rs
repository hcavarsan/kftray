use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct HttpLogEntry {
    pub trace_id: String,
    pub request_timestamp: String,
    pub response_timestamp: Option<String>,
    pub method: String,
    pub path: String,
    pub status_code: Option<String>,
    pub duration_ms: Option<String>,
    pub request_headers: Vec<String>,
    pub request_body: String,
    pub response_headers: Vec<String>,
    pub response_body: String,
}

pub fn parse_http_logs(lines: &[String]) -> Vec<HttpLogEntry> {
    let mut entries: Vec<HttpLogEntry> = Vec::new();
    let mut request_indices = HashMap::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();

        if let Some(trace_id) = line.strip_prefix("# Trace ID: ")
            && i + 1 < lines.len()
            && lines[i + 1].trim().starts_with("# Request at: ")
        {
            let request_timestamp = lines[i + 1].trim().replace("# Request at: ", "");

            if i + 2 < lines.len() {
                let request_line = &lines[i + 2];
                let parts: Vec<&str> = request_line.split_whitespace().collect();
                let (method, path) = if parts.len() >= 2 {
                    (parts[0].to_string(), parts[1].to_string())
                } else {
                    ("UNKNOWN".to_string(), "/".to_string())
                };

                let mut headers = Vec::new();
                let mut body = String::new();
                let mut j = i + 3;
                let mut in_body = false;

                while j < lines.len() && lines[j].trim() != "###" {
                    let line = &lines[j];
                    if line.trim().is_empty() && !in_body {
                        in_body = true;
                    } else if line.trim().starts_with("# <empty body>") {
                        body = "<empty body>".to_string();
                    } else if in_body {
                        if !body.is_empty() {
                            body.push('\n');
                        }
                        body.push_str(line);
                    } else if !line.trim().starts_with("#") && line.contains(':') {
                        headers.push(line.trim().to_string());
                    }
                    j += 1;
                }

                request_indices.insert(trace_id, entries.len());
                entries.push(HttpLogEntry {
                    trace_id: trace_id.to_string(),
                    request_timestamp,
                    response_timestamp: None,
                    method,
                    path,
                    status_code: None,
                    duration_ms: None,
                    request_headers: headers,
                    request_body: body,
                    response_headers: Vec::new(),
                    response_body: String::new(),
                });
            }
        } else if line.starts_with("# Response at: ")
            && i > 0
            && let Some(trace_id) = lines[i - 1].trim().strip_prefix("# Trace ID: ")
            && let Some(&entry_index) = request_indices.get(trace_id)
        {
            let response_timestamp = line.replace("# Response at: ", "");
            if i + 1 < lines.len() && lines[i + 1].trim().starts_with("# Took: ") {
                let duration = lines[i + 1].trim().replace("# Took: ", "");

                if i + 2 < lines.len() {
                    let status_line = &lines[i + 2];
                    let parts: Vec<&str> = status_line.split_whitespace().collect();
                    let status_code = if parts.len() >= 2 {
                        parts[1].to_string()
                    } else {
                        "000".to_string()
                    };

                    let mut headers = Vec::new();
                    let mut body = String::new();
                    let mut j = i + 3;
                    let mut in_body = false;

                    while j < lines.len() && lines[j].trim() != "###" {
                        let line = &lines[j];
                        if line.trim().is_empty() && !in_body {
                            in_body = true;
                        } else if in_body {
                            if !body.is_empty() {
                                body.push('\n');
                            }
                            body.push_str(line);
                        } else if !line.trim().starts_with("#") && line.contains(':') {
                            headers.push(line.trim().to_string());
                        }
                        j += 1;
                    }

                    let entry = &mut entries[entry_index];
                    entry.response_timestamp = Some(response_timestamp);
                    entry.duration_ms = Some(duration);
                    entry.status_code = Some(status_code);
                    entry.response_headers = headers;
                    entry.response_body = body;
                }
            }
        }
        i += 1;
    }

    entries
}

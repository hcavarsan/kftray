pub mod file_explorer;
pub mod navigation;
mod popup;

use std::collections::HashSet;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{
    AtomicBool,
    Ordering,
};

use crossterm::event::{
    self,
    Event,
    KeyCode,
    KeyModifiers,
};
use crossterm::terminal::size;
pub use file_explorer::*;
use kftray_commons::models::{
    config_model::Config,
    config_state_model::ConfigState,
};
use kftray_commons::utils::db_mode::DatabaseMode;
pub use popup::*;
use ratatui::widgets::ListState;
use ratatui::widgets::TableState;
use ratatui_explorer::{
    FileExplorer,
    Theme,
};
use tui_logger::TuiWidgetEvent;
use tui_logger::TuiWidgetState;

use crate::core::port_forward::stop_all_port_forward_and_exit;
use crate::logging::LoggerState;
use crate::tui::input::navigation::handle_auto_add_configs;
use crate::tui::input::navigation::handle_context_selection;

#[cfg(not(debug_assertions))]
type UpdateInfo = crate::updater::UpdateInfo;

#[cfg(debug_assertions)]
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub has_update: bool,
}
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

impl HttpLogEntry {
    pub async fn replay(&self, base_url: &str) -> Result<HttpLogEntry, String> {
        let url = if self.path.starts_with("http") {
            self.path.clone()
        } else {
            format!("{}{}", base_url.trim_end_matches('/'), &self.path)
        };

        let method = match self.method.to_uppercase().as_str() {
            "GET" => reqwest::Method::GET,
            "POST" => reqwest::Method::POST,
            "PUT" => reqwest::Method::PUT,
            "DELETE" => reqwest::Method::DELETE,
            "PATCH" => reqwest::Method::PATCH,
            "HEAD" => reqwest::Method::HEAD,
            "OPTIONS" => reqwest::Method::OPTIONS,
            _ => return Err(format!("Unsupported HTTP method: {}", self.method)),
        };

        let client = reqwest::Client::builder()
            .default_headers(reqwest::header::HeaderMap::new())
            .http1_only()
            .build()
            .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

        let mut request_builder = client.request(method.clone(), &url);

        let mut headers = reqwest::header::HeaderMap::new();

        let skip_headers = ["host", "connection", "content-length", "transfer-encoding"];
        let mut added_headers = 0;

        for header_line in &self.request_headers {
            if let Some((name, value)) = header_line.split_once(": ") {
                let name_lower = name.to_lowercase();
                let value_trimmed = value.trim();

                if skip_headers.contains(&name_lower.as_str()) {
                    continue;
                }

                if value.is_empty() {
                    log::debug!("Skipping truly empty header: {}", name);
                    continue;
                }

                log::debug!("Adding header: {} = '{}'", name, value_trimmed);

                if let (Ok(header_name), Ok(header_value)) = (
                    reqwest::header::HeaderName::from_bytes(name.as_bytes()),
                    reqwest::header::HeaderValue::from_str(value_trimmed),
                ) {
                    headers.insert(header_name, header_value);
                    added_headers += 1;
                }
            }
        }

        request_builder = request_builder.headers(headers);

        let actual_body = self.request_body.trim();
        if !actual_body.is_empty() && actual_body != "<empty body>" && actual_body != "(empty)" {
            request_builder = request_builder.body(self.request_body.clone());
        }

        log::debug!(
            "Sending request: {} {} with {} headers",
            method,
            url,
            added_headers
        );

        match request_builder.send().await {
            Ok(response) => {
                let status = response.status();
                let response_headers: Vec<String> = response
                    .headers()
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v.to_str().unwrap_or("")))
                    .collect();

                match response.text().await {
                    Ok(response_body) => {
                        let now = chrono::Utc::now();
                        let uuid_str = uuid::Uuid::new_v4().to_string();
                        let trace_id = format!("replay-{}", &uuid_str[..8]);

                        let replay_entry = HttpLogEntry {
                            trace_id,
                            request_timestamp: now.format("%Y-%m-%d %H:%M:%S").to_string(),
                            response_timestamp: Some(now.format("%Y-%m-%d %H:%M:%S").to_string()),
                            method: self.method.clone(),
                            path: self.path.clone(),
                            status_code: Some(status.as_u16().to_string()),
                            duration_ms: Some("0".to_string()),
                            request_headers: self.request_headers.clone(),
                            request_body: self.request_body.clone(),
                            response_headers,
                            response_body,
                        };

                        Ok(replay_entry)
                    }
                    Err(e) => Err(format!("Failed to read response body: {}", e)),
                }
            }
            Err(e) => Err(format!("Request failed: {}", e)),
        }
    }
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum DeleteButton {
    Confirm,
    Close,
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum UpdateButton {
    Update,
    Cancel,
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum ActiveComponent {
    Menu,
    SearchBar,
    StoppedTable,
    RunningTable,
    Details,
    Logs,
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum ActiveTable {
    Stopped,
    Running,
}

#[derive(PartialEq, Debug)]
pub enum AppState {
    Normal,
    ShowErrorPopup,
    ShowConfirmationPopup,
    ImportFileExplorerOpen,
    ExportFileExplorerOpen,
    ShowInputPrompt,
    ShowHelp,
    ShowAbout,
    ShowDeleteConfirmation,
    ShowContextSelection,
    ShowSettings,
    ShowHttpLogsConfig,
    ShowHttpLogsViewer,
    #[cfg_attr(debug_assertions, allow(dead_code))]
    ShowUpdateConfirmation,
    #[cfg_attr(debug_assertions, allow(dead_code))]
    ShowUpdateProgress,
    #[cfg_attr(debug_assertions, allow(dead_code))]
    ShowRestartNotification,
}

pub struct App {
    pub details_scroll_offset: usize,
    pub details_scroll_max_offset: usize,
    pub selected_rows_stopped: HashSet<usize>,
    pub selected_rows_running: HashSet<usize>,
    pub import_file_explorer: FileExplorer,
    pub export_file_explorer: FileExplorer,
    pub state: AppState,
    pub selected_row_stopped: usize,
    pub selected_row_running: usize,
    pub active_table: ActiveTable,
    pub import_export_message: Option<String>,
    pub input_buffer: String,
    pub selected_file_path: Option<std::path::PathBuf>,
    pub file_content: Option<String>,
    pub stopped_configs: Vec<Config>,
    pub running_configs: Vec<Config>,
    pub error_message: Option<String>,
    pub active_component: ActiveComponent,
    pub selected_menu_item: usize,
    pub delete_confirmation_message: Option<String>,
    pub selected_delete_button: DeleteButton,
    pub visible_rows: usize,
    pub table_state_stopped: TableState,
    pub table_state_running: TableState,
    pub contexts: Vec<String>,
    pub auto_import_alias_as_domain: bool,
    pub auto_import_auto_loopback: bool,
    pub selected_context_index: usize,
    pub context_list_state: ListState,
    pub tui_logger_state: TuiWidgetState,
    pub logger_state: LoggerState,
    pub settings_timeout_input: String,
    pub settings_editing: bool,
    pub settings_network_monitor: bool,
    pub settings_selected_option: usize,
    pub settings_ssl_enabled: bool,
    pub settings_ssl_cert_validity_input: String,
    pub http_logs_enabled: std::collections::HashMap<i64, bool>,
    pub active_pods: std::collections::HashMap<i64, Option<String>>,
    pub http_logs_config_id: Option<i64>,
    pub http_logs_config_editing: bool,
    pub http_logs_config_selected_option: usize,
    pub http_logs_config_max_file_size_input: String,
    pub http_logs_config_retention_days_input: String,
    pub http_logs_config_enabled: bool,
    pub http_logs_config_auto_cleanup: bool,
    pub http_logs_viewer_content: Vec<String>,
    pub http_logs_viewer_scroll: usize,
    pub http_logs_viewer_config_id: Option<i64>,
    pub http_logs_viewer_auto_scroll: bool,
    pub http_logs_viewer_file_path: Option<std::path::PathBuf>,
    pub http_logs_requests: Vec<HttpLogEntry>,
    pub http_logs_list_selected: usize,
    pub http_logs_detail_mode: bool,
    pub http_logs_selected_entry: Option<HttpLogEntry>,
    pub http_logs_replay_result: Option<String>,
    pub http_logs_replay_in_progress: bool,
    pub throbber_state: throbber_widgets_tui::ThrobberState,
    pub configs_being_processed:
        std::collections::HashMap<i64, (Arc<AtomicBool>, std::time::Instant)>,
    pub error_receiver: Option<tokio::sync::mpsc::UnboundedReceiver<String>>,
    pub error_sender: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    pub search_query: String,
    pub search_focused: bool,
    pub filtered_stopped_configs: Vec<Config>,
    pub filtered_running_configs: Vec<Config>,
    pub update_info: Option<UpdateInfo>,
    pub selected_update_button: UpdateButton,
    pub update_progress_message: Option<String>,
}

impl Default for App {
    fn default() -> Self {
        panic!("App::default() should not be used. Use App::new(LoggerState) instead.");
    }
}

impl App {
    pub fn new(logger_state: LoggerState) -> Self {
        let theme = Theme::default().add_default_title();
        let import_file_explorer = FileExplorer::with_theme(theme.clone()).unwrap();
        let export_file_explorer = FileExplorer::with_theme(theme).unwrap();
        let tui_logger_state = TuiWidgetState::new();
        let (error_sender, error_receiver) = tokio::sync::mpsc::unbounded_channel();

        let mut app = Self {
            details_scroll_offset: 0,
            details_scroll_max_offset: 0,
            import_file_explorer,
            export_file_explorer,
            state: AppState::Normal,
            selected_row_stopped: 0,
            selected_row_running: 0,
            active_table: ActiveTable::Stopped,
            selected_rows_stopped: HashSet::new(),
            selected_rows_running: HashSet::new(),
            import_export_message: None,
            input_buffer: String::new(),
            selected_file_path: None,
            file_content: None,
            auto_import_alias_as_domain: false,
            auto_import_auto_loopback: false,
            stopped_configs: Vec::new(),
            running_configs: Vec::new(),
            error_message: None,
            active_component: ActiveComponent::StoppedTable,
            selected_menu_item: 0,
            delete_confirmation_message: None,
            selected_delete_button: DeleteButton::Confirm,
            visible_rows: 0,
            table_state_stopped: TableState::default(),
            table_state_running: TableState::default(),
            contexts: Vec::new(),
            selected_context_index: 0,
            context_list_state: ListState::default(),
            tui_logger_state,
            logger_state,
            settings_timeout_input: String::new(),
            settings_editing: false,
            settings_network_monitor: true,
            settings_selected_option: 0,
            settings_ssl_enabled: false,
            settings_ssl_cert_validity_input: String::new(),
            http_logs_enabled: std::collections::HashMap::new(),
            active_pods: std::collections::HashMap::new(),
            http_logs_config_id: None,
            http_logs_config_editing: false,
            http_logs_config_selected_option: 0,
            http_logs_config_max_file_size_input: String::new(),
            http_logs_config_retention_days_input: String::new(),
            http_logs_config_enabled: false,
            http_logs_config_auto_cleanup: true,
            http_logs_viewer_content: Vec::new(),
            http_logs_viewer_scroll: 0,
            http_logs_viewer_config_id: None,
            http_logs_viewer_auto_scroll: true,
            http_logs_viewer_file_path: None,
            http_logs_requests: Vec::new(),
            http_logs_list_selected: 0,
            http_logs_detail_mode: false,
            http_logs_selected_entry: None,
            http_logs_replay_result: None,
            http_logs_replay_in_progress: false,
            throbber_state: throbber_widgets_tui::ThrobberState::default(),
            configs_being_processed: std::collections::HashMap::new(),
            error_receiver: Some(error_receiver),
            error_sender: Some(error_sender),
            search_query: String::new(),
            search_focused: false,
            filtered_stopped_configs: Vec::new(),
            filtered_running_configs: Vec::new(),
            update_info: None,
            selected_update_button: UpdateButton::Update,
            update_progress_message: None,
        };

        if let Ok((_, height)) = size() {
            app.update_visible_rows(height);
        }

        app
    }

    fn matches_search_query(config: &Config, query_lower: &str) -> bool {
        config
            .alias
            .as_ref()
            .is_some_and(|alias| alias.to_lowercase().contains(query_lower))
            || config
                .service
                .as_ref()
                .is_some_and(|service| service.to_lowercase().contains(query_lower))
            || config.namespace.to_lowercase().contains(query_lower)
            || config
                .context
                .as_ref()
                .is_some_and(|context| context.to_lowercase().contains(query_lower))
            || config
                .workload_type
                .as_ref()
                .is_some_and(|workload| workload.to_lowercase().contains(query_lower))
            || config
                .local_port
                .is_some_and(|port| port.to_string().contains(query_lower))
            || config
                .remote_port
                .is_some_and(|port| port.to_string().contains(query_lower))
    }

    pub fn update_filtered_configs(&mut self) {
        if self.search_query.is_empty() {
            self.filtered_stopped_configs = self.stopped_configs.clone();
            self.filtered_running_configs = self.running_configs.clone();
        } else {
            let query_lower = self.search_query.to_lowercase();

            self.filtered_stopped_configs = self
                .stopped_configs
                .iter()
                .filter(|config| Self::matches_search_query(config, &query_lower))
                .cloned()
                .collect();

            self.filtered_running_configs = self
                .running_configs
                .iter()
                .filter(|config| Self::matches_search_query(config, &query_lower))
                .cloned()
                .collect();
        }

        let stopped_len = self.filtered_stopped_configs.len();
        let running_len = self.filtered_running_configs.len();

        if self.selected_row_stopped >= stopped_len && stopped_len > 0 {
            self.selected_row_stopped = 0;
            self.table_state_stopped.select(Some(0));
        } else if stopped_len == 0 {
            self.table_state_stopped.select(None);
            self.selected_rows_stopped.clear();
        }

        if self.selected_row_running >= running_len && running_len > 0 {
            self.selected_row_running = 0;
            self.table_state_running.select(Some(0));
        } else if running_len == 0 {
            self.table_state_running.select(None);
            self.selected_rows_running.clear();
        }
    }

    pub fn update_http_logs_viewer(&mut self) {
        if let Some(file_path) = &self.http_logs_viewer_file_path
            && let Ok(content) = std::fs::read_to_string(file_path)
        {
            let new_lines: Vec<String> = content.lines().map(|line| line.to_string()).collect();
            let old_len = self.http_logs_viewer_content.len();

            if new_lines.len() != old_len || new_lines != self.http_logs_viewer_content {
                self.http_logs_viewer_content = new_lines;

                self.http_logs_requests = Self::parse_http_logs(&self.http_logs_viewer_content);

                if self.http_logs_viewer_auto_scroll
                    && self.http_logs_viewer_content.len() > old_len
                {
                    self.http_logs_viewer_scroll = if self.http_logs_viewer_content.is_empty() {
                        0
                    } else {
                        self.http_logs_viewer_content.len().saturating_sub(1)
                    };
                }
            }
        }
    }

    fn parse_http_logs(lines: &[String]) -> Vec<HttpLogEntry> {
        let mut entries = Vec::new();
        let mut current_entry: Option<HttpLogEntry> = None;
        let mut i = 0;

        while i < lines.len() {
            let line = lines[i].trim();

            if line.starts_with("# Trace ID: ")
                && i + 1 < lines.len()
                && lines[i + 1].trim().starts_with("# Request at: ")
            {
                if let Some(entry) = current_entry.take() {
                    entries.push(entry);
                }

                let trace_id = line.replace("# Trace ID: ", "");
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

                    current_entry = Some(HttpLogEntry {
                        trace_id,
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
            } else if line.starts_with("# Response at: ") && current_entry.is_some() {
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

                        if let Some(ref mut entry) = current_entry {
                            entry.response_timestamp = Some(response_timestamp);
                            entry.duration_ms = Some(duration);
                            entry.status_code = Some(status_code);
                            entry.response_headers = headers;
                            entry.response_body = body;
                        }
                    }
                }
            }
            i += 1;
        }

        // Add the last entry if it exists
        if let Some(entry) = current_entry {
            entries.push(entry);
        }

        entries
    }

    pub async fn load_http_logs_states(&mut self, configs: &[Config], mode: DatabaseMode) {
        for config in configs {
            if let Some(config_id) = config.id {
                match kftray_commons::utils::http_logs_config::get_http_logs_config_with_mode(
                    config_id, mode,
                )
                .await
                {
                    Ok(http_logs_config) => {
                        self.http_logs_enabled
                            .insert(config_id, http_logs_config.enabled);
                    }
                    Err(_) => {
                        self.http_logs_enabled.insert(config_id, false);
                    }
                }
            }
        }
    }

    pub async fn load_active_pods(&mut self, config_states: &[ConfigState]) {
        use kftray_portforward::port_forward::CHILD_PROCESSES;

        for config_state in config_states {
            if config_state.is_running {
                let handle_key = format!("config:{}:service:", config_state.config_id);
                let processes = CHILD_PROCESSES.lock().await;

                let mut active_pod = None;
                for (key, process) in processes.iter() {
                    if key.starts_with(&handle_key)
                        && let Some(pod_name) = process.get_current_active_pod().await
                    {
                        active_pod = Some(pod_name);
                        break;
                    }
                }

                self.active_pods.insert(config_state.config_id, active_pod);
            } else {
                self.active_pods.insert(config_state.config_id, None);
            }
        }
    }

    pub fn update_visible_rows(&mut self, terminal_height: u16) {
        self.visible_rows = (terminal_height.saturating_sub(19)) as usize;
    }

    pub fn update_configs(&mut self, configs: &[Config], config_states: &[ConfigState]) {
        self.stopped_configs = configs
            .iter()
            .filter(|config| {
                config_states
                    .iter()
                    .find(|state| state.config_id == config.id.unwrap_or_default())
                    .map(|state| !state.is_running)
                    .unwrap_or(true)
            })
            .cloned()
            .collect();

        self.running_configs = configs
            .iter()
            .filter(|config| {
                config_states
                    .iter()
                    .find(|state| state.config_id == config.id.unwrap_or_default())
                    .map(|state| state.is_running)
                    .unwrap_or(false)
            })
            .cloned()
            .collect();

        self.update_filtered_configs();

        let now = std::time::Instant::now();
        self.configs_being_processed
            .retain(|&_config_id, (completion_flag, start_time)| {
                if completion_flag.load(Ordering::Relaxed) {
                    return false;
                }

                if now.duration_since(*start_time) > std::time::Duration::from_secs(30) {
                    return false;
                }

                true
            });

        if let Some(ref mut receiver) = self.error_receiver
            && let Ok(error_msg) = receiver.try_recv()
        {
            self.error_message = Some(error_msg);
            self.state = AppState::ShowErrorPopup;
        }
    }

    pub fn scroll_up(&mut self) {
        match self.active_table {
            ActiveTable::Stopped => {
                let configs = if self.search_query.is_empty() {
                    &self.stopped_configs
                } else {
                    &self.filtered_stopped_configs
                };
                if !configs.is_empty()
                    && let Some(selected) = self.table_state_stopped.selected()
                    && selected > 0
                {
                    self.table_state_stopped.select(Some(selected - 1));
                    self.selected_row_stopped = selected - 1;
                }
            }
            ActiveTable::Running => {
                let configs = if self.search_query.is_empty() {
                    &self.running_configs
                } else {
                    &self.filtered_running_configs
                };
                if !configs.is_empty()
                    && let Some(selected) = self.table_state_running.selected()
                    && selected > 0
                {
                    self.table_state_running.select(Some(selected - 1));
                    self.selected_row_running = selected - 1;
                }
            }
        }
    }

    pub fn scroll_down(&mut self) {
        match self.active_table {
            ActiveTable::Stopped => {
                let configs = if self.search_query.is_empty() {
                    &self.stopped_configs
                } else {
                    &self.filtered_stopped_configs
                };
                if !configs.is_empty() {
                    if let Some(selected) = self.table_state_stopped.selected() {
                        if selected < configs.len() - 1 {
                            self.table_state_stopped.select(Some(selected + 1));
                            self.selected_row_stopped = selected + 1;
                        }
                    } else {
                        self.table_state_stopped.select(Some(0));
                        self.selected_row_stopped = 0;
                    }
                }
            }
            ActiveTable::Running => {
                let configs = if self.search_query.is_empty() {
                    &self.running_configs
                } else {
                    &self.filtered_running_configs
                };
                if !configs.is_empty() {
                    if let Some(selected) = self.table_state_running.selected() {
                        if selected < configs.len() - 1 {
                            self.table_state_running.select(Some(selected + 1));
                            self.selected_row_running = selected + 1;
                        }
                    } else {
                        self.table_state_running.select(Some(0));
                        self.selected_row_running = 0;
                    }
                }
            }
        }
    }
}

pub fn toggle_select_all(app: &mut App) {
    let (selected_rows, configs) = match app.active_table {
        ActiveTable::Stopped => (&mut app.selected_rows_stopped, &app.stopped_configs),
        ActiveTable::Running => (&mut app.selected_rows_running, &app.running_configs),
    };

    if selected_rows.len() == configs.len() {
        selected_rows.clear();
    } else {
        selected_rows.clear();
        for i in 0..configs.len() {
            selected_rows.insert(i);
        }
    }
}

pub async fn handle_input(app: &mut App, mode: DatabaseMode) -> io::Result<bool> {
    if event::poll(std::time::Duration::from_millis(100))? {
        if let Event::Key(key) = event::read()? {
            log::debug!("Key pressed: {key:?}");

            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                stop_all_port_forward_and_exit(app, mode).await;
            }

            match app.state {
                AppState::ShowErrorPopup => {
                    log::debug!("Handling ShowErrorPopup state");
                    handle_error_popup_input(app, key.code)?;
                }
                AppState::ShowConfirmationPopup => {
                    log::debug!("Handling ShowConfirmationPopup state");
                    handle_confirmation_popup_input(app, key.code).await?;
                }
                AppState::ImportFileExplorerOpen => {
                    log::debug!("Handling ImportFileExplorerOpen state");
                    handle_import_file_explorer_input(app, key.code, mode).await?;
                }
                AppState::ExportFileExplorerOpen => {
                    log::debug!("Handling ExportFileExplorerOpen state");
                    handle_export_file_explorer_input(app, key.code, mode).await?;
                }
                AppState::ShowInputPrompt => {
                    log::debug!("Handling ShowInputPrompt state");
                    handle_export_input_prompt(app, key.code, mode).await?;
                }
                AppState::ShowHelp => {
                    log::debug!("Handling ShowHelp state");
                    handle_help_input(app, key.code)?;
                }
                AppState::ShowAbout => {
                    log::debug!("Handling ShowAbout state");
                    handle_about_input(app, key.code)?;
                }
                AppState::ShowDeleteConfirmation => {
                    log::debug!("Handling ShowDeleteConfirmation state");
                    handle_delete_confirmation_input(app, key.code, mode).await?;
                }
                AppState::ShowContextSelection => {
                    log::debug!("Handling ShowContextSelection state");
                    handle_context_selection_input(app, key.code, mode).await?;
                }
                AppState::ShowSettings => {
                    log::debug!("Handling ShowSettings state");
                    handle_settings_input(app, key.code, mode).await?;
                }
                AppState::ShowHttpLogsConfig => {
                    log::debug!("Handling ShowHttpLogsConfig state");
                    handle_http_logs_config_input(app, key.code, mode).await?;
                }
                AppState::ShowHttpLogsViewer => {
                    log::debug!("Handling ShowHttpLogsViewer state");
                    handle_http_logs_viewer_input(app, key.code).await?;
                }
                AppState::ShowUpdateConfirmation => {
                    log::debug!("Handling ShowUpdateConfirmation state");
                    handle_update_confirmation_input(app, key.code, mode).await?;
                }
                AppState::ShowUpdateProgress => {
                    log::debug!("Handling ShowUpdateProgress state");
                }
                AppState::ShowRestartNotification => {
                    log::debug!("Handling ShowRestartNotification state");
                    handle_restart_notification_input(app, key.code)?;
                }
                AppState::Normal => {
                    log::debug!("Handling Normal state");
                    if app.active_component == ActiveComponent::SearchBar {
                        handle_search_input(app, key.code)?;
                    } else {
                        handle_normal_input(app, key.code, mode).await?;
                    }
                }
            }
        } else if let Event::Resize(_, height) = event::read()? {
            log::debug!("Handling Resize event");
            app.update_visible_rows(height);
        }
    }
    Ok(false)
}

pub async fn handle_normal_input(
    app: &mut App, key: KeyCode, mode: DatabaseMode,
) -> io::Result<()> {
    if handle_common_hotkeys(app, key, mode).await? {
        return Ok(());
    }

    match key {
        KeyCode::Tab => {
            app.active_component = match app.active_component {
                ActiveComponent::Menu => ActiveComponent::SearchBar,
                ActiveComponent::SearchBar => ActiveComponent::StoppedTable,
                ActiveComponent::StoppedTable => ActiveComponent::Details,
                ActiveComponent::Details => ActiveComponent::Menu,
                _ => ActiveComponent::Menu,
            };

            app.search_focused = app.active_component == ActiveComponent::SearchBar;
            app.active_table = match app.active_component {
                ActiveComponent::StoppedTable => ActiveTable::Stopped,
                _ => app.active_table,
            };
        }
        KeyCode::PageUp | KeyCode::PageDown => match app.active_component {
            ActiveComponent::Logs => handle_logs_input(app, key).await?,
            ActiveComponent::Details => handle_details_input(app, key, mode).await?,
            _ => {
                if key == KeyCode::PageUp {
                    scroll_page_up(app);
                } else {
                    scroll_page_down(app);
                }
            }
        },
        _ => match app.active_component {
            ActiveComponent::Menu => handle_menu_input(app, key, mode).await?,
            ActiveComponent::SearchBar => handle_search_input(app, key)?,
            ActiveComponent::StoppedTable => handle_stopped_table_input(app, key, mode).await?,
            ActiveComponent::RunningTable => handle_running_table_input(app, key, mode).await?,
            ActiveComponent::Details => handle_details_input(app, key, mode).await?,
            ActiveComponent::Logs => handle_logs_input(app, key).await?,
        },
    }
    Ok(())
}

pub fn scroll_page_up(app: &mut App) {
    match app.active_component {
        ActiveComponent::StoppedTable => {
            let rows_to_scroll = app.visible_rows;
            if app.selected_row_stopped >= rows_to_scroll {
                app.selected_row_stopped -= rows_to_scroll;
            } else {
                app.selected_row_stopped = 0;
            }
            app.table_state_stopped
                .select(Some(app.selected_row_stopped));
        }
        ActiveComponent::RunningTable => {
            let rows_to_scroll = app.visible_rows;
            if app.selected_row_running >= rows_to_scroll {
                app.selected_row_running -= rows_to_scroll;
            } else {
                app.selected_row_running = 0;
            }
            app.table_state_running
                .select(Some(app.selected_row_running));
        }
        ActiveComponent::Details => {
            if app.details_scroll_offset >= app.visible_rows {
                app.details_scroll_offset -= app.visible_rows;
            } else {
                app.details_scroll_offset = 0;
            }
        }
        _ => {}
    }
}

pub fn scroll_page_down(app: &mut App) {
    match app.active_component {
        ActiveComponent::StoppedTable => {
            let configs = if app.search_query.is_empty() {
                &app.stopped_configs
            } else {
                &app.filtered_stopped_configs
            };
            let rows_to_scroll = app.visible_rows;
            if app.selected_row_stopped + rows_to_scroll < configs.len() {
                app.selected_row_stopped += rows_to_scroll;
            } else if !configs.is_empty() {
                app.selected_row_stopped = configs.len() - 1;
            }
            app.table_state_stopped
                .select(Some(app.selected_row_stopped));
        }
        ActiveComponent::RunningTable => {
            let configs = if app.search_query.is_empty() {
                &app.running_configs
            } else {
                &app.filtered_running_configs
            };
            let rows_to_scroll = app.visible_rows;
            if app.selected_row_running + rows_to_scroll < configs.len() {
                app.selected_row_running += rows_to_scroll;
            } else if !configs.is_empty() {
                app.selected_row_running = configs.len() - 1;
            }
            app.table_state_running
                .select(Some(app.selected_row_running));
        }
        ActiveComponent::Details => {
            if app.details_scroll_offset + app.visible_rows < app.details_scroll_max_offset {
                app.details_scroll_offset += app.visible_rows;
            } else {
                app.details_scroll_offset = app.details_scroll_max_offset;
            }
        }
        _ => {}
    }
}

pub fn select_first_row(app: &mut App) {
    match app.active_table {
        ActiveTable::Stopped => {
            let configs = if app.search_query.is_empty() {
                &app.stopped_configs
            } else {
                &app.filtered_stopped_configs
            };
            if !configs.is_empty() {
                app.table_state_stopped.select(Some(0));
                app.selected_row_stopped = 0;
            }
        }
        ActiveTable::Running => {
            let configs = if app.search_query.is_empty() {
                &app.running_configs
            } else {
                &app.filtered_running_configs
            };
            if !configs.is_empty() {
                app.table_state_running.select(Some(0));
                app.selected_row_running = 0;
            }
        }
    }
}

pub fn clear_selection(app: &mut App) {
    match app.active_table {
        ActiveTable::Stopped => {
            app.selected_rows_stopped.clear();
            app.selected_rows_running.clear();
            app.table_state_stopped.select(None);
            app.selected_row_stopped = 0;
            app.table_state_running.select(None);
            app.selected_row_running = 0;
        }
        ActiveTable::Running => {
            app.selected_rows_running.clear();
            app.selected_rows_stopped.clear();
            app.table_state_running.select(None);
            app.selected_row_running = 0;
            app.table_state_stopped.select(None);
            app.selected_row_stopped = 0;
        }
    }
}

pub async fn handle_menu_input(app: &mut App, key: KeyCode, mode: DatabaseMode) -> io::Result<()> {
    if handle_common_hotkeys(app, key, mode).await? {
        return Ok(());
    }

    match key {
        KeyCode::Left => {
            if app.selected_menu_item > 0 {
                app.selected_menu_item -= 1
            }
        }
        KeyCode::Right => {
            if app.selected_menu_item < 6 {
                app.selected_menu_item += 1
            }
        }
        KeyCode::Down => {
            app.active_component = ActiveComponent::SearchBar;
            app.search_focused = true;
        }
        KeyCode::Enter => match app.selected_menu_item {
            0 => app.state = AppState::ShowHelp,
            1 => handle_auto_add_configs(app).await,
            2 => open_import_file_explorer(app),
            3 => open_export_file_explorer(app),
            4 => {
                app.state = AppState::ShowSettings;
                if let Ok(timeout) =
                    kftray_commons::utils::settings::get_disconnect_timeout_with_mode(mode).await
                {
                    app.settings_timeout_input = timeout.unwrap_or(0).to_string();
                }
                if let Ok(network_monitor) =
                    kftray_commons::utils::settings::get_network_monitor_with_mode(mode).await
                {
                    app.settings_network_monitor = network_monitor;
                }
                if let Ok(ssl_enabled) =
                    kftray_commons::utils::settings::get_ssl_enabled_with_mode(mode).await
                {
                    app.settings_ssl_enabled = ssl_enabled;
                }
                if let Ok(ssl_validity) =
                    kftray_commons::utils::settings::get_ssl_cert_validity_days_with_mode(mode)
                        .await
                {
                    app.settings_ssl_cert_validity_input = ssl_validity.to_string();
                }
                app.settings_editing = false;
                app.settings_selected_option = 0;
            }
            5 => {
                #[cfg(not(debug_assertions))]
                if app.update_info.is_none()
                    && let Ok(update_info) = crate::updater::check_for_updates().await
                {
                    app.update_info = Some(update_info);
                }
                app.state = AppState::ShowAbout;
            }
            6 => stop_all_port_forward_and_exit(app, mode).await,
            _ => {}
        },
        _ => {}
    }
    Ok(())
}

pub async fn handle_stopped_table_input(
    app: &mut App, key: KeyCode, mode: DatabaseMode,
) -> io::Result<()> {
    if handle_common_hotkeys(app, key, mode).await? {
        return Ok(());
    }

    match key {
        KeyCode::Right => {
            app.active_component = ActiveComponent::RunningTable;
            app.active_table = ActiveTable::Running;
            clear_selection(app);
            select_first_row(app);
        }
        KeyCode::Up => {
            if app.table_state_stopped.selected() == Some(0) {
                app.active_component = ActiveComponent::SearchBar;
                app.search_focused = true;
            } else {
                app.scroll_up();
            }
        }
        KeyCode::Down => {
            let configs = if app.search_query.is_empty() {
                &app.stopped_configs
            } else {
                &app.filtered_stopped_configs
            };
            if configs.is_empty() || app.table_state_stopped.selected() == Some(configs.len() - 1) {
                app.active_component = ActiveComponent::Details;
                app.table_state_running.select(None);
                app.selected_rows_stopped.clear();
                app.table_state_stopped.select(None);
            } else {
                app.scroll_down();
            }
        }
        KeyCode::Char(' ') => toggle_row_selection(app),
        KeyCode::Char('f') => handle_port_forwarding(app, mode).await?,
        KeyCode::Char('d') => show_delete_confirmation(app),
        KeyCode::Char('a') => toggle_select_all(app),
        KeyCode::Char('l') => handle_http_logs_toggle(app, mode).await?,
        KeyCode::Char('L') => handle_http_logs_config(app, mode).await?,
        KeyCode::Char('o') => handle_open_http_logs(app, mode).await?,
        KeyCode::Char('V') => handle_view_http_logs(app, mode).await?,
        _ => {}
    }
    Ok(())
}

pub async fn handle_running_table_input(
    app: &mut App, key: KeyCode, mode: DatabaseMode,
) -> io::Result<()> {
    if handle_common_hotkeys(app, key, mode).await? {
        return Ok(());
    }

    match key {
        KeyCode::Left => {
            app.active_component = ActiveComponent::StoppedTable;
            app.active_table = ActiveTable::Stopped;
            clear_selection(app);
            select_first_row(app);
        }
        KeyCode::Up => {
            let configs = if app.search_query.is_empty() {
                &app.running_configs
            } else {
                &app.filtered_running_configs
            };
            if configs.is_empty() || app.table_state_running.selected() == Some(0) {
                app.active_component = ActiveComponent::SearchBar;
                app.search_focused = true;
            } else {
                app.scroll_up();
            }
        }
        KeyCode::Down => {
            let configs = if app.search_query.is_empty() {
                &app.running_configs
            } else {
                &app.filtered_running_configs
            };
            if configs.is_empty() || app.table_state_running.selected() == Some(configs.len() - 1) {
                app.active_component = ActiveComponent::Logs;
                app.table_state_running.select(None);
                app.selected_rows_stopped.clear();
                app.table_state_stopped.select(None);
            } else {
                app.scroll_down();
            }
        }
        KeyCode::Char(' ') => toggle_row_selection(app),
        KeyCode::Char('f') => handle_port_forwarding(app, mode).await?,
        KeyCode::Char('d') => show_delete_confirmation(app),
        KeyCode::Char('a') => toggle_select_all(app),
        KeyCode::Char('l') => handle_http_logs_toggle(app, mode).await?,
        KeyCode::Char('L') => handle_http_logs_config(app, mode).await?,
        KeyCode::Char('o') => handle_open_http_logs(app, mode).await?,
        KeyCode::Char('V') => handle_view_http_logs(app, mode).await?,
        _ => {}
    }
    Ok(())
}

pub async fn handle_details_input(
    app: &mut App, key: KeyCode, mode: DatabaseMode,
) -> io::Result<()> {
    if handle_common_hotkeys(app, key, mode).await? {
        return Ok(());
    }

    match key {
        KeyCode::Right => app.active_component = ActiveComponent::Logs,
        KeyCode::Up => {
            app.active_component = ActiveComponent::StoppedTable;
            app.active_table = ActiveTable::Stopped;
            clear_selection(app);
            if !app.stopped_configs.is_empty() {
                app.table_state_stopped.select(Some(0));
                app.selected_row_stopped = 0;
            }
        }
        KeyCode::PageUp => {
            if app.details_scroll_offset >= app.visible_rows {
                app.details_scroll_offset -= app.visible_rows;
            } else {
                app.details_scroll_offset = 0;
            }
        }
        KeyCode::PageDown => {
            if app.details_scroll_offset + app.visible_rows < app.details_scroll_max_offset {
                app.details_scroll_offset += app.visible_rows;
            } else {
                app.details_scroll_offset = app.details_scroll_max_offset;
            }
        }
        _ => {}
    }
    Ok(())
}

pub async fn handle_logs_input(app: &mut App, key: KeyCode) -> io::Result<()> {
    match key {
        KeyCode::Left => app.active_component = ActiveComponent::Details,
        KeyCode::Up => {
            app.active_component = ActiveComponent::RunningTable;
            app.active_table = ActiveTable::Running;
            clear_selection(app);
            select_first_row(app);
        }
        KeyCode::PageUp => app.tui_logger_state.transition(TuiWidgetEvent::PrevPageKey),
        KeyCode::PageDown => app.tui_logger_state.transition(TuiWidgetEvent::NextPageKey),
        _ => {}
    }
    Ok(())
}

pub async fn handle_common_hotkeys(
    app: &mut App, key: KeyCode, mode: DatabaseMode,
) -> io::Result<bool> {
    match key {
        KeyCode::Char('q') => {
            #[cfg(not(debug_assertions))]
            if app.update_info.is_none()
                && let Ok(update_info) = crate::updater::check_for_updates().await
            {
                app.update_info = Some(update_info);
            }
            app.state = AppState::ShowAbout;
            Ok(true)
        }
        KeyCode::Char('i') => {
            open_import_file_explorer(app);
            Ok(true)
        }
        KeyCode::Char('e') => {
            open_export_file_explorer(app);
            Ok(true)
        }
        KeyCode::Char('h') => {
            app.state = AppState::ShowHelp;
            Ok(true)
        }
        KeyCode::Char('s') => {
            app.state = AppState::ShowSettings;
            if let Ok(timeout) =
                kftray_commons::utils::settings::get_disconnect_timeout_with_mode(mode).await
            {
                app.settings_timeout_input = timeout.unwrap_or(0).to_string();
            }
            if let Ok(network_monitor) =
                kftray_commons::utils::settings::get_network_monitor_with_mode(mode).await
            {
                app.settings_network_monitor = network_monitor;
            }
            if let Ok(ssl_enabled) =
                kftray_commons::utils::settings::get_ssl_enabled_with_mode(mode).await
            {
                app.settings_ssl_enabled = ssl_enabled;
            }
            if let Ok(ssl_validity) =
                kftray_commons::utils::settings::get_ssl_cert_validity_days_with_mode(mode).await
            {
                app.settings_ssl_cert_validity_input = ssl_validity.to_string();
            }
            app.settings_editing = false;
            app.settings_selected_option = 0;
            Ok(true)
        }
        KeyCode::Char('L') => {
            handle_http_logs_config(app, mode).await?;
            Ok(true)
        }
        KeyCode::Char('V') => {
            handle_view_http_logs(app, mode).await?;
            Ok(true)
        }
        KeyCode::Char('/') => {
            app.active_component = ActiveComponent::SearchBar;
            app.search_focused = true;
            if !app.search_query.is_empty() {
                app.search_query.clear();
                app.update_filtered_configs();
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}

pub fn handle_search_input(app: &mut App, key: KeyCode) -> io::Result<()> {
    match key {
        KeyCode::Esc => {
            app.active_component = ActiveComponent::StoppedTable;
            app.search_focused = false;
            app.search_query.clear();
            app.update_filtered_configs();
            app.selected_rows_stopped.clear();
            app.selected_rows_running.clear();
        }
        KeyCode::Up => {
            app.active_component = ActiveComponent::Menu;
            app.search_focused = false;
        }
        KeyCode::Enter | KeyCode::Down => {
            app.search_focused = false;
            if !app.filtered_stopped_configs.is_empty() {
                app.active_component = ActiveComponent::StoppedTable;
                app.active_table = ActiveTable::Stopped;
            } else if !app.filtered_running_configs.is_empty() {
                app.active_component = ActiveComponent::RunningTable;
                app.active_table = ActiveTable::Running;
            } else {
                app.active_component = ActiveComponent::StoppedTable;
            }
            select_first_row(app);
        }
        KeyCode::Backspace => {
            app.search_query.pop();
            app.update_filtered_configs();
            app.selected_rows_stopped.clear();
            app.selected_rows_running.clear();
        }
        KeyCode::Char(c) => {
            app.search_query.push(c);
            app.update_filtered_configs();
            app.selected_rows_stopped.clear();
            app.selected_rows_running.clear();
        }
        _ => {}
    }
    Ok(())
}

pub fn toggle_row_selection(app: &mut App) {
    match app.active_table {
        ActiveTable::Running => {
            if let Some(selected) = app.table_state_running.selected() {
                if app.selected_rows_running.contains(&selected) {
                    app.selected_rows_running.retain(|&x| x != selected);
                } else {
                    app.selected_rows_running.insert(selected);
                }
                app.selected_row_running = selected;
            }
        }
        ActiveTable::Stopped => {
            if let Some(selected) = app.table_state_stopped.selected() {
                if app.selected_rows_stopped.contains(&selected) {
                    app.selected_rows_stopped.retain(|&x| x != selected);
                } else {
                    app.selected_rows_stopped.insert(selected);
                }
                app.selected_row_stopped = selected;
            }
        }
    }
}

pub async fn handle_port_forwarding(app: &mut App, mode: DatabaseMode) -> io::Result<()> {
    let (selected_rows, configs, selected_row) = match app.active_table {
        ActiveTable::Stopped => (
            &mut app.selected_rows_stopped,
            if app.search_query.is_empty() {
                &app.stopped_configs
            } else {
                &app.filtered_stopped_configs
            },
            app.selected_row_stopped,
        ),
        ActiveTable::Running => (
            &mut app.selected_rows_running,
            if app.search_query.is_empty() {
                &app.running_configs
            } else {
                &app.filtered_running_configs
            },
            app.selected_row_running,
        ),
    };

    if configs.is_empty() {
        return Ok(());
    }

    if selected_rows.is_empty() {
        selected_rows.insert(selected_row);
    }

    let selected_configs: Vec<Config> = selected_rows
        .iter()
        .filter_map(|&row| configs.get(row).cloned())
        .collect();

    let start_time = std::time::Instant::now();
    for config in &selected_configs {
        if let Some(id) = config.id {
            let completion_flag = Arc::new(AtomicBool::new(false));
            app.configs_being_processed
                .insert(id, (completion_flag.clone(), start_time));
        }
    }

    if app.active_table == ActiveTable::Stopped {
        app.running_configs.extend(selected_configs.clone());
        app.stopped_configs
            .retain(|config| !selected_configs.contains(config));
    } else {
        app.stopped_configs.extend(selected_configs.clone());
        app.running_configs
            .retain(|config| !selected_configs.contains(config));
    }

    let error_sender = app.error_sender.clone();
    let active_table = app.active_table;
    let logger_state_clone = app.logger_state.clone();
    for config in selected_configs.clone() {
        if let Some(id) = config.id {
            let completion_flag = app
                .configs_being_processed
                .get(&id)
                .map(|(flag, _)| flag.clone());
            let sender = error_sender.clone();
            let logger_state_for_task = logger_state_clone.clone();
            if let Some(flag) = completion_flag {
                tokio::spawn(async move {
                    use crate::core::port_forward::{
                        start_port_forwarding,
                        stop_port_forwarding,
                    };
                    use crate::tui::input::{
                        ActiveTable,
                        App,
                    };

                    let mut temp_app = App::new(logger_state_for_task);

                    let is_starting = active_table == ActiveTable::Stopped;

                    if is_starting {
                        start_port_forwarding(&mut temp_app, config, mode).await;
                    } else {
                        stop_port_forwarding(&mut temp_app, config, mode).await;
                    }

                    if let Some(error_msg) = temp_app.error_message
                        && let Some(sender) = sender
                    {
                        let _ = sender.send(error_msg);
                    }

                    flag.store(true, Ordering::Relaxed);
                });
            }
        }
    }

    match app.active_table {
        ActiveTable::Stopped => app.selected_rows_stopped.clear(),
        ActiveTable::Running => app.selected_rows_running.clear(),
    }

    Ok(())
}

pub fn show_delete_confirmation(app: &mut App) {
    if !app.selected_rows_stopped.is_empty() {
        app.state = AppState::ShowDeleteConfirmation;
        app.delete_confirmation_message =
            Some("Are you sure you want to delete the selected configs?".to_string());
    }
}

pub async fn handle_delete_confirmation_input(
    app: &mut App, key: KeyCode, mode: DatabaseMode,
) -> io::Result<()> {
    match key {
        KeyCode::Left | KeyCode::Right => {
            app.selected_delete_button = match app.selected_delete_button {
                DeleteButton::Confirm => DeleteButton::Close,
                DeleteButton::Close => DeleteButton::Confirm,
            };
        }
        KeyCode::Enter => {
            if app.selected_delete_button == DeleteButton::Confirm {
                let ids_to_delete: Vec<i64> = app
                    .selected_rows_stopped
                    .iter()
                    .filter_map(|&row| app.stopped_configs.get(row).and_then(|config| config.id))
                    .collect();

                match kftray_commons::utils::config::delete_configs_with_mode(
                    ids_to_delete.clone(),
                    mode,
                )
                .await
                {
                    Ok(_) => {
                        app.delete_confirmation_message =
                            Some("Configs deleted successfully.".to_string());
                        app.stopped_configs.retain(|config| {
                            !ids_to_delete.contains(&config.id.unwrap_or_default())
                        });
                    }
                    Err(e) => {
                        app.delete_confirmation_message =
                            Some(format!("Failed to delete configs: {e}"));
                    }
                }
            }
            app.selected_rows_stopped.clear();
            app.state = AppState::Normal;
        }
        KeyCode::Esc => app.state = AppState::Normal,
        _ => {}
    }
    Ok(())
}

pub fn open_import_file_explorer(app: &mut App) {
    app.state = AppState::ImportFileExplorerOpen;
    app.selected_file_path = std::env::current_dir().ok();
}

pub fn open_export_file_explorer(app: &mut App) {
    app.state = AppState::ExportFileExplorerOpen;
    app.selected_file_path = std::env::current_dir().ok();
}

pub async fn handle_context_selection_input(
    app: &mut App, key: KeyCode, mode: DatabaseMode,
) -> io::Result<()> {
    if let KeyCode::Enter = key {
        if let Some(selected_context) = app.contexts.get(app.selected_context_index).cloned() {
            handle_context_selection(app, &selected_context, mode).await;
        }
    } else if let KeyCode::Up = key {
        if app.selected_context_index > 0 {
            app.selected_context_index -= 1;
            app.context_list_state
                .select(Some(app.selected_context_index));
        }
    } else if let KeyCode::Esc = key {
        app.state = AppState::Normal;
    } else if let KeyCode::Char('a') = key {
        app.auto_import_alias_as_domain = !app.auto_import_alias_as_domain;
    } else if let KeyCode::Char('d') = key {
        app.auto_import_auto_loopback = !app.auto_import_auto_loopback;
    } else if let KeyCode::Down = key
        && app.selected_context_index < app.contexts.len() - 1
    {
        app.selected_context_index += 1;
        app.context_list_state
            .select(Some(app.selected_context_index));
    }
    Ok(())
}

pub async fn handle_settings_input(
    app: &mut App, key: KeyCode, mode: DatabaseMode,
) -> io::Result<()> {
    match key {
        KeyCode::Esc => {
            app.state = AppState::Normal;
            app.settings_editing = false;
        }
        KeyCode::Up => {
            if app.settings_selected_option > 0 {
                app.settings_selected_option -= 1;
            }
        }
        KeyCode::Down => {
            if app.settings_selected_option < 4 {
                app.settings_selected_option += 1;
            }
        }
        KeyCode::Enter => {
            match app.settings_selected_option {
                0 => {
                    let timeout_enabled = !(app.settings_timeout_input == "0"
                        || app.settings_timeout_input.is_empty());
                    if timeout_enabled {
                        app.settings_timeout_input = "0".to_string();
                    } else {
                        app.settings_timeout_input = "5".to_string();
                    }

                    if let Ok(timeout_value) = app.settings_timeout_input.parse::<u32>()
                        && (kftray_commons::utils::settings::set_disconnect_timeout_with_mode(
                            timeout_value,
                            mode,
                        )
                        .await)
                            .is_err()
                    {
                        app.error_message = Some("Failed to save timeout setting".to_string());
                        app.state = AppState::ShowErrorPopup;
                    }
                }
                1 => {
                    let timeout_enabled = !(app.settings_timeout_input == "0"
                        || app.settings_timeout_input.is_empty());
                    if timeout_enabled {
                        if app.settings_editing {
                            if let Ok(timeout_value) = app.settings_timeout_input.parse::<u32>() {
                                if timeout_value > 0 {
                                    if (kftray_commons::utils::settings::set_disconnect_timeout_with_mode(
                                        timeout_value,
                                        mode,
                                    )
                                    .await)
                                        .is_err()
                                    {
                                        app.error_message =
                                            Some("Failed to save timeout setting".to_string());
                                        app.state = AppState::ShowErrorPopup;
                                    } else {
                                        app.settings_editing = false;
                                    }
                                } else {
                                    app.error_message =
                                        Some("Timeout must be greater than 0".to_string());
                                    app.state = AppState::ShowErrorPopup;
                                }
                            } else {
                                app.error_message = Some(
                                    "Invalid timeout value. Please enter a number.".to_string(),
                                );
                                app.state = AppState::ShowErrorPopup;
                            }
                        } else {
                            // Start editing
                            app.settings_editing = true;
                        }
                    }
                }
                2 => {
                    // Handle network monitor toggle
                    app.settings_network_monitor = !app.settings_network_monitor;
                    match kftray_commons::utils::settings::set_network_monitor_with_mode(
                        app.settings_network_monitor,
                        mode,
                    )
                    .await
                    {
                        Err(e) => {
                            app.error_message =
                                Some(format!("Failed to save network monitor setting: {e}"));
                            app.state = AppState::ShowErrorPopup;
                        }
                        _ => {
                            // Control network monitor at runtime
                            if app.settings_network_monitor {
                                if let Err(e) = kftray_network_monitor::restart().await {
                                    app.error_message =
                                        Some(format!("Failed to start network monitor: {e}"));
                                    app.state = AppState::ShowErrorPopup;
                                }
                            } else if let Err(e) = kftray_network_monitor::stop().await {
                                app.error_message =
                                    Some(format!("Failed to stop network monitor: {e}"));
                                app.state = AppState::ShowErrorPopup;
                            }
                        }
                    }
                }
                3 => {
                    app.settings_ssl_enabled = !app.settings_ssl_enabled;
                    match kftray_commons::utils::settings::set_ssl_enabled_with_mode(
                        app.settings_ssl_enabled,
                        mode,
                    )
                    .await
                    {
                        Err(e) => {
                            app.error_message = Some(format!("Failed to save SSL setting: {e}"));
                            app.state = AppState::ShowErrorPopup;
                        }
                        _ => {
                            if app.settings_ssl_enabled {
                                if let Err(e) = kftray_commons::utils::settings::set_ssl_auto_regenerate_with_mode(true, mode).await {
                                    app.error_message = Some(format!("Failed to enable SSL auto regenerate: {e}"));
                                    app.state = AppState::ShowErrorPopup;
                                    return Ok(());
                                }
                                if let Err(e) = kftray_commons::utils::settings::set_ssl_ca_auto_install_with_mode(true, mode).await {
                                    app.error_message = Some(format!("Failed to enable SSL CA auto install: {e}"));
                                    app.state = AppState::ShowErrorPopup;
                                    return Ok(());
                                }

                                match kftray_commons::utils::settings::get_app_settings_with_mode(
                                    mode,
                                )
                                .await
                                {
                                    Ok(settings) => {
                                        match kftray_portforward::ssl::CertificateManager::new(
                                            &settings,
                                        ) {
                                            Ok(cert_manager) => {
                                                if let Err(e) = cert_manager
                                                    .ensure_ca_installed_and_trusted()
                                                    .await
                                                {
                                                    app.error_message = Some(format!(
                                                        "Failed to install CA certificate: {e}"
                                                    ));
                                                    app.state = AppState::ShowErrorPopup;
                                                    return Ok(());
                                                }

                                                if let Err(e) = cert_manager
                                                    .regenerate_certificate_for_all_configs()
                                                    .await
                                                {
                                                    app.error_message = Some(format!(
                                                        "Failed to generate SSL certificates: {e}"
                                                    ));
                                                    app.state = AppState::ShowErrorPopup;
                                                }
                                            }
                                            Err(e) => {
                                                app.error_message = Some(format!(
                                                    "Failed to create certificate manager: {e}"
                                                ));
                                                app.state = AppState::ShowErrorPopup;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        app.error_message =
                                            Some(format!("Failed to get app settings: {e}"));
                                        app.state = AppState::ShowErrorPopup;
                                    }
                                }
                            }
                        }
                    }
                }
                4 => {
                    let ssl_enabled = app.settings_ssl_enabled;
                    if ssl_enabled {
                        if app.settings_editing {
                            if let Ok(validity_value) =
                                app.settings_ssl_cert_validity_input.parse::<u16>()
                            {
                                if validity_value > 0 && validity_value <= 3650 {
                                    if (kftray_commons::utils::settings::set_ssl_cert_validity_days_with_mode(
                                        validity_value,
                                        mode,
                                    )
                                    .await)
                                        .is_err()
                                    {
                                        app.error_message =
                                            Some("Failed to save SSL validity setting".to_string());
                                        app.state = AppState::ShowErrorPopup;
                                    } else {
                                        app.settings_editing = false;
                                    }
                                } else {
                                    app.error_message = Some(
                                        "Validity must be between 1 and 3650 days".to_string(),
                                    );
                                    app.state = AppState::ShowErrorPopup;
                                }
                            } else {
                                app.error_message = Some(
                                    "Invalid validity value. Please enter a number.".to_string(),
                                );
                                app.state = AppState::ShowErrorPopup;
                            }
                        } else {
                            app.settings_editing = true;
                        }
                    }
                }
                _ => {}
            }
        }
        KeyCode::Char(c) => {
            if app.settings_editing && c.is_ascii_digit() {
                match app.settings_selected_option {
                    1 => app.settings_timeout_input.push(c),
                    4 => app.settings_ssl_cert_validity_input.push(c),
                    _ => {}
                }
            }
        }
        KeyCode::Backspace => {
            if app.settings_editing {
                match app.settings_selected_option {
                    1 => {
                        app.settings_timeout_input.pop();
                    }
                    4 => {
                        app.settings_ssl_cert_validity_input.pop();
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    Ok(())
}

async fn handle_http_logs_toggle(app: &mut App, mode: DatabaseMode) -> io::Result<()> {
    let config_id = match app.active_table {
        ActiveTable::Stopped => {
            if let Some(index) = app.table_state_stopped.selected() {
                if index < app.stopped_configs.len() {
                    app.stopped_configs[index].id
                } else {
                    None
                }
            } else {
                None
            }
        }
        ActiveTable::Running => {
            if let Some(index) = app.table_state_running.selected() {
                if index < app.running_configs.len() {
                    app.running_configs[index].id
                } else {
                    None
                }
            } else {
                None
            }
        }
    };

    if let Some(id) = config_id {
        let current_state = app.http_logs_enabled.get(&id).unwrap_or(&false);
        let new_state = !current_state;

        let mut config =
            match kftray_commons::utils::http_logs_config::get_http_logs_config_with_mode(id, mode)
                .await
            {
                Ok(config) => config,
                Err(_) => kftray_commons::models::http_logs_config_model::HttpLogsConfig::new(id),
            };

        config.enabled = new_state;

        match kftray_commons::utils::http_logs_config::update_http_logs_config_with_mode(
            &config, mode,
        )
        .await
        {
            Ok(()) => {
                app.http_logs_enabled.insert(id, new_state);
                app.import_export_message = Some(format!(
                    "HTTP logs {} for config ID: {}",
                    if new_state { "enabled" } else { "disabled" },
                    id
                ));
            }
            Err(e) => {
                app.error_message = Some(format!("Failed to toggle HTTP logs: {}", e));
                app.state = AppState::ShowErrorPopup;
            }
        }
    } else {
        app.error_message = Some("No configuration selected for HTTP logs toggle".to_string());
        app.state = AppState::ShowErrorPopup;
    }

    Ok(())
}

async fn handle_view_http_logs(app: &mut App, mode: DatabaseMode) -> io::Result<()> {
    let config_info = match app.active_table {
        ActiveTable::Stopped => app
            .stopped_configs
            .get(app.table_state_stopped.selected().unwrap_or(0)),
        ActiveTable::Running => app
            .running_configs
            .get(app.table_state_running.selected().unwrap_or(0)),
    };

    if let Some(config) = config_info {
        if let (Some(config_id), Some(local_port)) = (config.id, config.local_port) {
            let http_logs_enabled =
                match kftray_commons::utils::http_logs_config::get_http_logs_config_with_mode(
                    config_id, mode,
                )
                .await
                {
                    Ok(http_config) => http_config.enabled,
                    Err(_) => *app.http_logs_enabled.get(&config_id).unwrap_or(&false),
                };

            if !http_logs_enabled {
                app.error_message =
                    Some("HTTP logs are not enabled for this configuration".to_string());
                app.state = AppState::ShowErrorPopup;
                return Ok(());
            }

            let log_file_name = format!("{}_{}.http", config_id, local_port);

            match kftray_commons::utils::config_dir::get_log_folder_path() {
                Ok(log_folder_path) => {
                    let log_file_path = log_folder_path.join(&log_file_name);

                    if !log_file_path.exists() {
                        app.error_message = Some(format!(
                            "HTTP log file does not exist: {}",
                            log_file_path.display()
                        ));
                        app.state = AppState::ShowErrorPopup;
                        return Ok(());
                    }

                    match std::fs::read_to_string(&log_file_path) {
                        Ok(content) => {
                            app.http_logs_viewer_content =
                                content.lines().map(|line| line.to_string()).collect();
                            app.http_logs_requests =
                                App::parse_http_logs(&app.http_logs_viewer_content);
                            app.http_logs_viewer_scroll = if app.http_logs_viewer_content.is_empty()
                            {
                                0
                            } else {
                                app.http_logs_viewer_content.len().saturating_sub(1)
                            };
                            app.http_logs_viewer_config_id = Some(config_id);
                            app.http_logs_viewer_auto_scroll = true;
                            app.http_logs_viewer_file_path = Some(log_file_path);
                            app.state = AppState::ShowHttpLogsViewer;
                        }
                        Err(e) => {
                            app.error_message =
                                Some(format!("Failed to read HTTP log file: {}", e));
                            app.state = AppState::ShowErrorPopup;
                        }
                    }
                }
                Err(e) => {
                    app.error_message = Some(format!("Failed to get log folder path: {}", e));
                    app.state = AppState::ShowErrorPopup;
                }
            }
        } else {
            app.error_message = Some("Configuration is missing ID or local port".to_string());
            app.state = AppState::ShowErrorPopup;
        }
    } else {
        app.error_message = Some("No configuration selected for viewing HTTP logs".to_string());
        app.state = AppState::ShowErrorPopup;
    }

    Ok(())
}

async fn handle_http_logs_config(app: &mut App, mode: DatabaseMode) -> io::Result<()> {
    let config_id = match app.active_table {
        ActiveTable::Stopped => {
            if let Some(index) = app.table_state_stopped.selected() {
                if index < app.stopped_configs.len() {
                    app.stopped_configs[index].id
                } else {
                    None
                }
            } else if !app.stopped_configs.is_empty() {
                app.stopped_configs[0].id
            } else {
                None
            }
        }
        ActiveTable::Running => {
            if let Some(index) = app.table_state_running.selected() {
                if index < app.running_configs.len() {
                    app.running_configs[index].id
                } else {
                    None
                }
            } else if !app.running_configs.is_empty() {
                app.running_configs[0].id
            } else {
                None
            }
        }
    };

    let config_id = config_id.or_else(|| {
        if !app.stopped_configs.is_empty() {
            app.stopped_configs[0].id
        } else if !app.running_configs.is_empty() {
            app.running_configs[0].id
        } else {
            None
        }
    });

    if let Some(id) = config_id {
        let config =
            match kftray_commons::utils::http_logs_config::get_http_logs_config_with_mode(id, mode)
                .await
            {
                Ok(config) => config,
                Err(_) => kftray_commons::models::http_logs_config_model::HttpLogsConfig::new(id),
            };

        app.http_logs_config_id = Some(id);
        app.http_logs_config_enabled = config.enabled;
        app.http_logs_config_auto_cleanup = config.auto_cleanup;
        app.http_logs_config_max_file_size_input =
            (config.max_file_size / (1024 * 1024)).to_string();
        app.http_logs_config_retention_days_input = config.retention_days.to_string();
        app.http_logs_config_editing = false;
        app.http_logs_config_selected_option = 0;

        app.state = AppState::ShowHttpLogsConfig;
    } else {
        app.error_message = Some("No configuration available for HTTP logs config".to_string());
        app.state = AppState::ShowErrorPopup;
    }

    Ok(())
}

async fn handle_open_http_logs(app: &mut App, _mode: DatabaseMode) -> io::Result<()> {
    let config_info = match app.active_table {
        ActiveTable::Stopped => {
            if let Some(index) = app.table_state_stopped.selected() {
                if index < app.stopped_configs.len() {
                    Some((
                        app.stopped_configs[index].id,
                        app.stopped_configs[index].local_port,
                    ))
                } else {
                    None
                }
            } else {
                None
            }
        }
        ActiveTable::Running => {
            if let Some(index) = app.table_state_running.selected() {
                if index < app.running_configs.len() {
                    Some((
                        app.running_configs[index].id,
                        app.running_configs[index].local_port,
                    ))
                } else {
                    None
                }
            } else {
                None
            }
        }
    };

    if let Some((config_id, local_port)) = config_info {
        if let (Some(id), Some(port)) = (config_id, local_port) {
            let log_file_name = format!("{}_{}.http", id, port);

            match open_http_log_file(&log_file_name).await {
                Ok(()) => {
                    app.import_export_message =
                        Some(format!("Opened HTTP log file: {}", log_file_name));
                }
                Err(err) => {
                    app.error_message = Some(format!("Failed to open HTTP log file: {}", err));
                    app.state = AppState::ShowErrorPopup;
                }
            }
        } else {
            app.error_message = Some("Selected config missing ID or port".to_string());
            app.state = AppState::ShowErrorPopup;
        }
    } else {
        app.error_message = Some("No configuration selected for opening HTTP logs".to_string());
        app.state = AppState::ShowErrorPopup;
    }

    Ok(())
}

async fn open_http_log_file(log_file_name: &str) -> Result<(), String> {
    use std::env;
    use std::process::Command;

    let log_folder_path = kftray_commons::utils::config_dir::get_log_folder_path()
        .map_err(|e| format!("Failed to get log folder path: {e}"))?;

    let log_file_path = log_folder_path.join(log_file_name);

    if !log_file_path.exists() {
        return Err(format!(
            "HTTP log file does not exist: {}",
            log_file_path.display()
        ));
    }

    let file_path_str = log_file_path.to_str().ok_or("Invalid UTF-8 in file path")?;

    if let Ok(editor) = env::var("EDITOR")
        && Command::new(&editor).arg(file_path_str).spawn().is_ok()
    {
        return Ok(());
    }

    if let Ok(visual) = env::var("VISUAL")
        && Command::new(&visual).arg(file_path_str).spawn().is_ok()
    {
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        if Command::new("open")
            .args(["-t", file_path_str])
            .spawn()
            .is_ok()
        {
            return Ok(());
        }
    }

    #[cfg(target_os = "linux")]
    {
        if Command::new("xdg-open").arg(file_path_str).spawn().is_ok() {
            return Ok(());
        }
    }

    #[cfg(target_os = "windows")]
    {
        if Command::new("notepad").arg(file_path_str).spawn().is_ok() {
            return Ok(());
        }
    }

    Err("Failed to open file with any available editor".to_string())
}

async fn handle_http_logs_config_input(
    app: &mut App, key: KeyCode, mode: DatabaseMode,
) -> io::Result<()> {
    match key {
        KeyCode::Esc => {
            app.state = AppState::Normal;
            app.http_logs_config_editing = false;
        }
        KeyCode::Up => {
            app.http_logs_config_selected_option = match app.http_logs_config_selected_option {
                0 => 0,
                1 => 0,
                2 => 3,
                3 => 3,
                _ => 0,
            };
        }
        KeyCode::Down => {
            app.http_logs_config_selected_option = match app.http_logs_config_selected_option {
                0 => 1,
                1 => 1,
                2 => 2,
                3 => 2,
                _ => 0,
            };
        }
        KeyCode::Left => {
            app.http_logs_config_selected_option = match app.http_logs_config_selected_option {
                0 => 0,
                1 => 1,
                2 => 1,
                3 => 0,
                _ => 0,
            };
        }
        KeyCode::Right => {
            app.http_logs_config_selected_option = match app.http_logs_config_selected_option {
                0 => 3,
                1 => 2,
                2 => 2,
                3 => 3,
                _ => 0,
            };
        }
        KeyCode::Enter => match app.http_logs_config_selected_option {
            0 => {
                app.http_logs_config_enabled = !app.http_logs_config_enabled;
                auto_save_http_logs_config(app, mode).await;
            }
            1 => {
                if app.http_logs_config_editing {
                    app.http_logs_config_editing = false;
                    auto_save_http_logs_config(app, mode).await;
                } else {
                    app.http_logs_config_editing = true;
                }
            }
            2 => {
                if app.http_logs_config_editing {
                    app.http_logs_config_editing = false;
                    auto_save_http_logs_config(app, mode).await;
                } else {
                    app.http_logs_config_editing = true;
                }
            }
            3 => {
                app.http_logs_config_auto_cleanup = !app.http_logs_config_auto_cleanup;
                auto_save_http_logs_config(app, mode).await;
            }
            _ => {}
        },
        KeyCode::Char(c) if app.http_logs_config_editing => {
            match app.http_logs_config_selected_option {
                1 => {
                    if c.is_ascii_digit() {
                        app.http_logs_config_max_file_size_input.push(c);
                    }
                }
                2 => {
                    if c.is_ascii_digit() {
                        app.http_logs_config_retention_days_input.push(c);
                    }
                }
                _ => {}
            }
        }
        KeyCode::Backspace if app.http_logs_config_editing => {
            match app.http_logs_config_selected_option {
                1 => {
                    app.http_logs_config_max_file_size_input.pop();
                }
                2 => {
                    app.http_logs_config_retention_days_input.pop();
                }
                _ => {}
            }
        }
        _ => {}
    }
    Ok(())
}

async fn auto_save_http_logs_config(app: &mut App, mode: DatabaseMode) {
    if let Some(config_id) = app.http_logs_config_id {
        let max_file_size = match app.http_logs_config_max_file_size_input.parse::<u64>() {
            Ok(size) if size > 0 && size <= 1000 => size * 1024 * 1024,
            Ok(_) => {
                app.error_message = Some("Max file size must be between 1 and 1000 MB".to_string());
                app.state = AppState::ShowErrorPopup;
                return;
            }
            Err(_) => {
                app.error_message = Some("Invalid max file size value".to_string());
                app.state = AppState::ShowErrorPopup;
                return;
            }
        };

        let retention_days = match app.http_logs_config_retention_days_input.parse::<u64>() {
            Ok(days) if days > 0 && days <= 365 => days,
            Ok(_) => {
                app.error_message = Some("Retention days must be between 1 and 365".to_string());
                app.state = AppState::ShowErrorPopup;
                return;
            }
            Err(_) => {
                app.error_message = Some("Invalid retention days value".to_string());
                app.state = AppState::ShowErrorPopup;
                return;
            }
        };

        let config = kftray_commons::models::http_logs_config_model::HttpLogsConfig {
            config_id,
            enabled: app.http_logs_config_enabled,
            max_file_size,
            retention_days,
            auto_cleanup: app.http_logs_config_auto_cleanup,
        };

        match kftray_commons::utils::http_logs_config::update_http_logs_config_with_mode(
            &config, mode,
        )
        .await
        {
            Ok(()) => {
                app.http_logs_enabled.insert(config_id, config.enabled);
                app.import_export_message = Some("✅ HTTP logs configuration saved".to_string());
            }
            Err(e) => {
                app.error_message = Some(format!("Failed to save HTTP logs configuration: {}", e));
                app.state = AppState::ShowErrorPopup;
            }
        }
    }
}

async fn handle_http_logs_viewer_input(app: &mut App, key: KeyCode) -> io::Result<()> {
    match key {
        KeyCode::Esc => {
            if app.http_logs_detail_mode {
                app.http_logs_detail_mode = false;
                app.http_logs_selected_entry = None;
                app.http_logs_replay_result = None;
                app.http_logs_replay_in_progress = false;
            } else {
                app.state = AppState::Normal;
                app.http_logs_viewer_content.clear();
                app.http_logs_viewer_scroll = 0;
                app.http_logs_viewer_config_id = None;
                app.http_logs_viewer_auto_scroll = true;
                app.http_logs_viewer_file_path = None;
                app.http_logs_requests.clear();
                app.http_logs_list_selected = 0;
                app.http_logs_detail_mode = false;
                app.http_logs_selected_entry = None;
                app.http_logs_replay_result = None;
                app.http_logs_replay_in_progress = false;
            }
        }
        KeyCode::Up => {
            if app.http_logs_detail_mode {
                if app.http_logs_viewer_scroll > 0 {
                    app.http_logs_viewer_scroll -= 1;
                }
            } else if app.http_logs_list_selected > 0 {
                app.http_logs_list_selected -= 1;
            }
        }
        KeyCode::Down => {
            if app.http_logs_detail_mode {
                app.http_logs_viewer_scroll += 1;
            } else if app.http_logs_list_selected < app.http_logs_requests.len().saturating_sub(1) {
                app.http_logs_list_selected += 1;
            }
        }
        KeyCode::PageUp => {
            if app.http_logs_detail_mode {
                if app.http_logs_viewer_scroll > 10 {
                    app.http_logs_viewer_scroll -= 10;
                } else {
                    app.http_logs_viewer_scroll = 0;
                }
            } else if app.http_logs_list_selected > 10 {
                app.http_logs_list_selected -= 10;
            } else {
                app.http_logs_list_selected = 0;
            }
        }
        KeyCode::PageDown => {
            if app.http_logs_detail_mode {
                app.http_logs_viewer_scroll += 10;
            } else {
                let new_selected = app.http_logs_list_selected + 10;
                if new_selected < app.http_logs_requests.len() {
                    app.http_logs_list_selected = new_selected;
                } else if !app.http_logs_requests.is_empty() {
                    app.http_logs_list_selected = app.http_logs_requests.len() - 1;
                }
            }
        }
        KeyCode::Home => {
            if app.http_logs_detail_mode {
                app.http_logs_viewer_scroll = 0;
            } else {
                app.http_logs_list_selected = 0;
            }
        }
        KeyCode::End => {
            if app.http_logs_detail_mode {
            } else if !app.http_logs_requests.is_empty() {
                app.http_logs_list_selected = app.http_logs_requests.len() - 1;
            }
        }
        KeyCode::Char('a') | KeyCode::Char('A') => {
            if !app.http_logs_detail_mode {
                app.http_logs_viewer_auto_scroll = !app.http_logs_viewer_auto_scroll;
            }
        }
        KeyCode::Char('r') | KeyCode::Char('R') => {
            if app.http_logs_detail_mode
                && app.http_logs_selected_entry.is_some()
                && let Some(entry) = &app.http_logs_selected_entry
            {
                let base_url = if let Some(config_id) = app.http_logs_viewer_config_id {
                    let local_port = app
                        .stopped_configs
                        .iter()
                        .chain(app.running_configs.iter())
                        .find(|c| c.id == Some(config_id))
                        .and_then(|c| c.local_port)
                        .unwrap_or(8080);
                    format!("http://localhost:{}", local_port)
                } else {
                    "http://localhost:8080".to_string()
                };

                let entry_clone = entry.clone();

                match entry_clone.replay(&base_url).await {
                    Ok(replay_entry) => {
                        app.http_logs_selected_entry = Some(replay_entry);
                        app.http_logs_viewer_scroll = 0;
                        app.http_logs_replay_result = None;
                        app.http_logs_replay_in_progress = false;
                    }
                    Err(error) => {
                        app.http_logs_replay_result = Some(error);
                        app.http_logs_replay_in_progress = false;
                    }
                }
            }
        }
        KeyCode::Enter => {
            if !app.http_logs_detail_mode
                && !app.http_logs_requests.is_empty()
                && let Some(entry) = app
                    .http_logs_requests
                    .get(app.http_logs_list_selected)
                    .cloned()
            {
                app.http_logs_selected_entry = Some(entry);
                app.http_logs_detail_mode = true;
                app.http_logs_viewer_scroll = 0;
                app.http_logs_replay_result = None;
                app.http_logs_replay_in_progress = false;
            }
        }
        _ => {}
    }
    Ok(())
}

pub async fn handle_update_confirmation_input(
    app: &mut App, key: KeyCode, _mode: DatabaseMode,
) -> io::Result<()> {
    match key {
        KeyCode::Left | KeyCode::Right => {
            app.selected_update_button = match app.selected_update_button {
                UpdateButton::Update => UpdateButton::Cancel,
                UpdateButton::Cancel => UpdateButton::Update,
            };
        }
        KeyCode::Enter => {
            if app.selected_update_button == UpdateButton::Update {
                #[cfg(debug_assertions)]
                {
                    app.state = AppState::ShowErrorPopup;
                    app.error_message =
                        Some("Updates are disabled in development mode".to_string());
                    app.update_info = None;
                }
                #[cfg(not(debug_assertions))]
                {
                    app.state = AppState::ShowUpdateProgress;
                    app.update_progress_message = Some("Downloading update...".to_string());

                    match crate::updater::perform_update().await {
                        Ok(_) => {
                            app.state = AppState::ShowRestartNotification;
                            app.update_progress_message = None;
                        }
                        Err(e) => {
                            log::error!("Update failed: {}", e);
                            app.state = AppState::ShowErrorPopup;
                            app.error_message = Some(format!("Update failed: {}", e));
                            app.update_info = None;
                            app.update_progress_message = None;
                        }
                    }
                }
            } else {
                app.state = AppState::Normal;
                app.update_info = None;
            }
        }
        KeyCode::Esc => {
            app.state = AppState::Normal;
            app.update_info = None;
        }
        _ => {}
    }
    Ok(())
}

pub fn handle_restart_notification_input(app: &mut App, key: KeyCode) -> io::Result<()> {
    match key {
        KeyCode::Enter | KeyCode::Esc => {
            app.state = AppState::Normal;
            app.update_info = None;
            app.update_progress_message = None;
        }
        _ => {}
    }
    Ok(())
}

pub fn handle_help_input(app: &mut App, key: KeyCode) -> io::Result<()> {
    match key {
        KeyCode::Esc | KeyCode::Enter | KeyCode::Char('h') => {
            app.state = AppState::Normal;
        }
        _ => {}
    }
    Ok(())
}

pub fn handle_about_input(app: &mut App, key: KeyCode) -> io::Result<()> {
    match key {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.state = AppState::Normal;
        }
        KeyCode::Enter => {
            #[cfg(not(debug_assertions))]
            if let Some(update_info) = &app.update_info
                && update_info.has_update
            {
                app.state = AppState::ShowUpdateConfirmation;
            }
            #[cfg(not(debug_assertions))]
            if app.update_info.is_none() || !app.update_info.as_ref().unwrap().has_update {
                app.state = AppState::Normal;
            }
            #[cfg(debug_assertions)]
            {
                app.state = AppState::Normal;
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn handle_error_popup_input(app: &mut App, key: KeyCode) -> io::Result<()> {
    match key {
        KeyCode::Esc | KeyCode::Enter => {
            app.state = AppState::Normal;
            app.error_message = None;
        }
        _ => {}
    }
    Ok(())
}

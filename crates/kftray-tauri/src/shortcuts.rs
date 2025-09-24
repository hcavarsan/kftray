use std::sync::Arc;

use async_trait::async_trait;
use kftray_commons::models::config_model::Config;
use kftray_shortcuts::{
    ActionContext,
    ActionHandler,
    ActionRegistry,
    ShortcutManager,
};
use log::{
    error,
    info,
};
use tauri::{
    AppHandle,
    Emitter,
    Manager,
};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::{
    Mutex,
    OnceCell,
};

use crate::commands::{
    config::get_configs_cmd,
    config_state::get_config_states,
    portforward::{
        deploy_and_forward_pod_cmd,
        start_port_forward_tcp_cmd,
        stop_all_port_forward_cmd,
        stop_port_forward_cmd,
        stop_proxy_forward_cmd,
    },
};

static GLOBAL_MANAGER: OnceCell<Arc<Mutex<ShortcutManager>>> = OnceCell::const_new();

pub async fn setup_shortcut_integration(
    app: tauri::AppHandle,
) -> Result<(), Box<dyn std::error::Error>> {
    let pool = kftray_commons::utils::db::get_db_pool().await?;
    let mut registry = ActionRegistry::new();

    let toggle_action = Arc::new(ToggleWindowAction::new(app.clone()));
    registry.register_handler(toggle_action);

    let config_action = Arc::new(ConfigAction);
    registry.register_handler(config_action);

    let start_all_action = Arc::new(StartAllPortForwardAction::new(app.clone()));
    registry.register_handler(start_all_action);

    let stop_all_action = Arc::new(StopAllPortForwardAction::new(app.clone()));
    registry.register_handler(stop_all_action);

    let start_port_forward_action = Arc::new(StartPortForwardAction::new(app.clone()));
    registry.register_handler(start_port_forward_action);

    let stop_port_forward_action = Arc::new(StopPortForwardAction::new(app.clone()));
    registry.register_handler(stop_port_forward_action);

    let toggle_port_forward_action = Arc::new(TogglePortForwardAction::new(app.clone()));
    registry.register_handler(toggle_port_forward_action);

    let mut manager =
        kftray_shortcuts::create_manager_with_registry(pool.as_ref().clone(), registry)
            .await
            .map_err(|e| format!("Failed to create shortcut manager: {}", e))?;
    manager.initialize().await?;

    manager.start_event_loop().await;

    GLOBAL_MANAGER
        .set(Arc::new(Mutex::new(manager)))
        .map_err(|_| "Failed to initialize global manager")?;

    info!("Shortcut integration setup completed");
    Ok(())
}

pub async fn get_manager() -> Result<Arc<Mutex<ShortcutManager>>, String> {
    GLOBAL_MANAGER
        .get()
        .cloned()
        .ok_or_else(|| "Shortcut manager not initialized".to_string())
}

struct ToggleWindowAction {
    app_handle: AppHandle,
}

impl ToggleWindowAction {
    fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }
}

#[async_trait]
impl ActionHandler for ToggleWindowAction {
    async fn execute(&self, _context: &ActionContext) -> kftray_shortcuts::ShortcutResult<()> {
        info!("Executing toggle window action");
        if let Some(window) = self.app_handle.get_webview_window("main") {
            crate::window::toggle_window_visibility(&window);
        } else {
            error!("Main window not found");
            return Err(kftray_shortcuts::ShortcutError::ActionExecutionFailed(
                "Main window not found".to_string(),
            ));
        }
        Ok(())
    }

    fn action_type(&self) -> &str {
        "toggle_window"
    }

    fn description(&self) -> &str {
        "Toggle main application window visibility"
    }
}

pub struct ConfigAction;

#[async_trait]
impl ActionHandler for ConfigAction {
    async fn execute(&self, context: &ActionContext) -> kftray_shortcuts::ShortcutResult<()> {
        let config_id = context.config_id.ok_or_else(|| {
            kftray_shortcuts::ShortcutError::ActionExecutionFailed(
                "Config ID required for config action".to_string(),
            )
        })?;

        if let Some(action_data) = &context.action_data {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(action_data)
                && let Some(action) = data.get("action").and_then(|v| v.as_str())
            {
                info!(
                    "Executing config action '{}' for config ID: {}",
                    action, config_id
                );
            }
        } else {
            info!(
                "Executing default config action for config ID: {}",
                config_id
            );
        }

        Ok(())
    }

    fn action_type(&self) -> &str {
        "config_action"
    }

    fn description(&self) -> &str {
        "Execute action related to a specific configuration"
    }
}

struct StartAllPortForwardAction {
    app_handle: AppHandle,
}

impl StartAllPortForwardAction {
    fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }
}

#[async_trait]
impl ActionHandler for StartAllPortForwardAction {
    async fn execute(&self, _context: &ActionContext) -> kftray_shortcuts::ShortcutResult<()> {
        info!("Executing start all port forward action");

        let configs = match get_configs_cmd().await {
            Ok(configs) => configs,
            Err(e) => {
                error!("Failed to get configs: {}", e);
                return Err(kftray_shortcuts::ShortcutError::ActionExecutionFailed(
                    format!("Failed to get configs: {}", e),
                ));
            }
        };

        let config_states = match get_config_states().await {
            Ok(states) => states,
            Err(e) => {
                error!("Failed to get config states: {}", e);
                return Err(kftray_shortcuts::ShortcutError::ActionExecutionFailed(
                    format!("Failed to get config states: {}", e),
                ));
            }
        };

        let running_config_ids: Vec<i64> = config_states
            .iter()
            .filter(|state| state.is_running)
            .map(|state| state.config_id)
            .collect();

        let configs_to_start: Vec<Config> = configs
            .into_iter()
            .filter(|config| !running_config_ids.contains(&config.id.unwrap_or(0)))
            .collect();

        if configs_to_start.is_empty() {
            info!("No configs to start - all are already running");
            let _ = self
                .app_handle
                .notification()
                .builder()
                .title("Port Forward")
                .body("All port forwards are already running")
                .show();
            return Ok(());
        }

        let configs_count = configs_to_start.len();

        for config in configs_to_start {
            let result = if config.workload_type.as_deref() == Some("service")
                || config.workload_type.as_deref() == Some("pod")
            {
                if config.protocol == "tcp" {
                    start_port_forward_tcp_cmd(vec![config.clone()], self.app_handle.clone()).await
                } else {
                    deploy_and_forward_pod_cmd(vec![config.clone()], self.app_handle.clone()).await
                }
            } else {
                deploy_and_forward_pod_cmd(vec![config.clone()], self.app_handle.clone()).await
            };

            if let Err(e) = result {
                error!(
                    "Failed to start port forward for config {}: {}",
                    config.id.unwrap_or(0),
                    e
                );
            }
        }

        let _ = self
            .app_handle
            .notification()
            .builder()
            .title("Port Forward")
            .body(format!(
                "Started {} port forward{}",
                configs_count,
                if configs_count == 1 { "" } else { "s" }
            ))
            .show();

        let _ = self.app_handle.emit("port-forward-status-changed", ());

        Ok(())
    }

    fn action_type(&self) -> &str {
        "start_all_port_forward"
    }

    fn description(&self) -> &str {
        "Start all port forwards"
    }
}

struct StopAllPortForwardAction {
    app_handle: AppHandle,
}

impl StopAllPortForwardAction {
    fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }
}

#[async_trait]
impl ActionHandler for StopAllPortForwardAction {
    async fn execute(&self, _context: &ActionContext) -> kftray_shortcuts::ShortcutResult<()> {
        info!("Executing stop all port forward action");

        match stop_all_port_forward_cmd(self.app_handle.clone()).await {
            Ok(_) => {
                info!("Successfully stopped all port forwards");

                let _ = self
                    .app_handle
                    .notification()
                    .builder()
                    .title("Port Forward")
                    .body("Stopped all port forwards")
                    .show();

                let _ = self.app_handle.emit("port-forward-status-changed", ());
                Ok(())
            }
            Err(e) => {
                error!("Failed to stop all port forwards: {}", e);
                Err(kftray_shortcuts::ShortcutError::ActionExecutionFailed(
                    format!("Failed to stop all port forwards: {}", e),
                ))
            }
        }
    }

    fn action_type(&self) -> &str {
        "stop_all_port_forward"
    }

    fn description(&self) -> &str {
        "Stop all port forwards"
    }
}

struct StartPortForwardAction {
    app_handle: AppHandle,
}

impl StartPortForwardAction {
    fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }
}

#[async_trait]
impl ActionHandler for StartPortForwardAction {
    async fn execute(&self, context: &ActionContext) -> kftray_shortcuts::ShortcutResult<()> {
        info!("Executing start port forward action");

        let config_ids = if let Some(action_data) = &context.action_data {
            match serde_json::from_str::<serde_json::Value>(action_data) {
                Ok(data) => {
                    if let Some(ids) = data.get("config_ids").and_then(|v| v.as_array()) {
                        ids.iter().filter_map(|v| v.as_i64()).collect::<Vec<i64>>()
                    } else {
                        return Err(kftray_shortcuts::ShortcutError::ActionExecutionFailed(
                            "No config_ids found in action data".to_string(),
                        ));
                    }
                }
                Err(e) => {
                    return Err(kftray_shortcuts::ShortcutError::ActionExecutionFailed(
                        format!("Failed to parse action data: {}", e),
                    ));
                }
            }
        } else {
            return Err(kftray_shortcuts::ShortcutError::ActionExecutionFailed(
                "No action data provided for start port forward action".to_string(),
            ));
        };

        let all_configs = match get_configs_cmd().await {
            Ok(configs) => configs,
            Err(e) => {
                error!("Failed to get configs: {}", e);
                return Err(kftray_shortcuts::ShortcutError::ActionExecutionFailed(
                    format!("Failed to get configs: {}", e),
                ));
            }
        };

        let config_states = match get_config_states().await {
            Ok(states) => states,
            Err(e) => {
                error!("Failed to get config states: {}", e);
                return Err(kftray_shortcuts::ShortcutError::ActionExecutionFailed(
                    format!("Failed to get config states: {}", e),
                ));
            }
        };

        let running_config_ids: Vec<i64> = config_states
            .iter()
            .filter(|state| state.is_running)
            .map(|state| state.config_id)
            .collect();

        let configs_to_start: Vec<Config> = all_configs
            .into_iter()
            .filter(|config| {
                config_ids.contains(&config.id.unwrap_or(0))
                    && !running_config_ids.contains(&config.id.unwrap_or(0))
            })
            .collect();

        if configs_to_start.is_empty() {
            info!("No configs to start - specified configs are already running or not found");
            let _ = self
                .app_handle
                .notification()
                .builder()
                .title("Port Forward")
                .body("Selected configs are already running or not found")
                .show();
            return Ok(());
        }

        let configs_count = configs_to_start.len();

        for config in configs_to_start {
            let result = if config.workload_type.as_deref() == Some("service")
                || config.workload_type.as_deref() == Some("pod")
            {
                if config.protocol == "tcp" {
                    start_port_forward_tcp_cmd(vec![config.clone()], self.app_handle.clone()).await
                } else {
                    deploy_and_forward_pod_cmd(vec![config.clone()], self.app_handle.clone()).await
                }
            } else {
                deploy_and_forward_pod_cmd(vec![config.clone()], self.app_handle.clone()).await
            };

            if let Err(e) = result {
                error!(
                    "Failed to start port forward for config {}: {}",
                    config.id.unwrap_or(0),
                    e
                );
            }
        }

        let _ = self
            .app_handle
            .notification()
            .builder()
            .title("Port Forward")
            .body(format!(
                "Started {} selected port forward{}",
                configs_count,
                if configs_count == 1 { "" } else { "s" }
            ))
            .show();

        let _ = self.app_handle.emit("port-forward-status-changed", ());

        Ok(())
    }

    fn action_type(&self) -> &str {
        "start_port_forward"
    }

    fn description(&self) -> &str {
        "Start specific port forwards"
    }
}

struct StopPortForwardAction {
    app_handle: AppHandle,
}

impl StopPortForwardAction {
    fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }
}

#[async_trait]
impl ActionHandler for StopPortForwardAction {
    async fn execute(&self, context: &ActionContext) -> kftray_shortcuts::ShortcutResult<()> {
        info!("Executing stop port forward action");

        let config_ids = if let Some(action_data) = &context.action_data {
            match serde_json::from_str::<serde_json::Value>(action_data) {
                Ok(data) => {
                    if let Some(ids) = data.get("config_ids").and_then(|v| v.as_array()) {
                        ids.iter().filter_map(|v| v.as_i64()).collect::<Vec<i64>>()
                    } else {
                        return Err(kftray_shortcuts::ShortcutError::ActionExecutionFailed(
                            "No config_ids found in action data".to_string(),
                        ));
                    }
                }
                Err(e) => {
                    return Err(kftray_shortcuts::ShortcutError::ActionExecutionFailed(
                        format!("Failed to parse action data: {}", e),
                    ));
                }
            }
        } else {
            return Err(kftray_shortcuts::ShortcutError::ActionExecutionFailed(
                "No action data provided for stop port forward action".to_string(),
            ));
        };

        let all_configs = match get_configs_cmd().await {
            Ok(configs) => configs,
            Err(e) => {
                error!("Failed to get configs: {}", e);
                return Err(kftray_shortcuts::ShortcutError::ActionExecutionFailed(
                    format!("Failed to get configs: {}", e),
                ));
            }
        };

        let config_states = match get_config_states().await {
            Ok(states) => states,
            Err(e) => {
                error!("Failed to get config states: {}", e);
                return Err(kftray_shortcuts::ShortcutError::ActionExecutionFailed(
                    format!("Failed to get config states: {}", e),
                ));
            }
        };

        let running_config_ids: Vec<i64> = config_states
            .iter()
            .filter(|state| state.is_running)
            .map(|state| state.config_id)
            .collect();

        let configs_to_stop: Vec<Config> = all_configs
            .into_iter()
            .filter(|config| {
                config_ids.contains(&config.id.unwrap_or(0))
                    && running_config_ids.contains(&config.id.unwrap_or(0))
            })
            .collect();

        if configs_to_stop.is_empty() {
            info!("No configs to stop - specified configs are not running or not found");
            let _ = self
                .app_handle
                .notification()
                .builder()
                .title("Port Forward")
                .body("Selected configs are not running or not found")
                .show();
            return Ok(());
        }

        let configs_count = configs_to_stop.len();

        for config in configs_to_stop {
            let result = if (config.workload_type.as_deref() == Some("service")
                || config.workload_type.as_deref() == Some("pod"))
                && config.protocol == "tcp"
            {
                stop_port_forward_cmd(config.id.unwrap_or(0).to_string(), self.app_handle.clone())
                    .await
            } else if config.workload_type.as_deref() == Some("proxy")
                || ((config.workload_type.as_deref() == Some("service")
                    || config.workload_type.as_deref() == Some("pod"))
                    && config.protocol == "udp")
            {
                stop_proxy_forward_cmd(
                    config.id.unwrap_or(0).to_string(),
                    &config.namespace,
                    config.service.unwrap_or_default(),
                    self.app_handle.clone(),
                )
                .await
            } else {
                Err(format!(
                    "Unsupported workload type: {:?}",
                    config.workload_type
                ))
            };

            if let Err(e) = result {
                error!(
                    "Failed to stop port forward for config {}: {}",
                    config.id.unwrap_or(0),
                    e
                );
            }
        }

        let _ = self
            .app_handle
            .notification()
            .builder()
            .title("Port Forward")
            .body(format!(
                "Stopped {} selected port forward{}",
                configs_count,
                if configs_count == 1 { "" } else { "s" }
            ))
            .show();

        let _ = self.app_handle.emit("port-forward-status-changed", ());

        Ok(())
    }

    fn action_type(&self) -> &str {
        "stop_port_forward"
    }

    fn description(&self) -> &str {
        "Stop specific port forwards"
    }
}

struct TogglePortForwardAction {
    app_handle: AppHandle,
}

impl TogglePortForwardAction {
    fn new(app_handle: AppHandle) -> Self {
        Self { app_handle }
    }
}

#[async_trait]
impl ActionHandler for TogglePortForwardAction {
    async fn execute(&self, context: &ActionContext) -> kftray_shortcuts::ShortcutResult<()> {
        info!("Executing toggle port forward action");

        let config_ids = if let Some(action_data) = &context.action_data {
            match serde_json::from_str::<serde_json::Value>(action_data) {
                Ok(data) => {
                    if let Some(ids) = data.get("config_ids").and_then(|v| v.as_array()) {
                        ids.iter().filter_map(|v| v.as_i64()).collect::<Vec<i64>>()
                    } else {
                        return Err(kftray_shortcuts::ShortcutError::ActionExecutionFailed(
                            "No config_ids found in action data".to_string(),
                        ));
                    }
                }
                Err(e) => {
                    return Err(kftray_shortcuts::ShortcutError::ActionExecutionFailed(
                        format!("Failed to parse action data: {}", e),
                    ));
                }
            }
        } else {
            return Err(kftray_shortcuts::ShortcutError::ActionExecutionFailed(
                "No action data provided for toggle port forward action".to_string(),
            ));
        };

        let all_configs = match get_configs_cmd().await {
            Ok(configs) => configs,
            Err(e) => {
                error!("Failed to get configs: {}", e);
                return Err(kftray_shortcuts::ShortcutError::ActionExecutionFailed(
                    format!("Failed to get configs: {}", e),
                ));
            }
        };

        let config_states = match get_config_states().await {
            Ok(states) => states,
            Err(e) => {
                error!("Failed to get config states: {}", e);
                return Err(kftray_shortcuts::ShortcutError::ActionExecutionFailed(
                    format!("Failed to get config states: {}", e),
                ));
            }
        };

        let running_config_ids: Vec<i64> = config_states
            .iter()
            .filter(|state| state.is_running)
            .map(|state| state.config_id)
            .collect();

        let target_configs: Vec<Config> = all_configs
            .into_iter()
            .filter(|config| config_ids.contains(&config.id.unwrap_or(0)))
            .collect();

        if target_configs.is_empty() {
            info!("No configs found with specified IDs");
            let _ = self
                .app_handle
                .notification()
                .builder()
                .title("Port Forward")
                .body("No configs found with specified IDs")
                .show();
            return Ok(());
        }

        let mut started_count = 0;
        let mut stopped_count = 0;

        for config in target_configs {
            let is_running = running_config_ids.contains(&config.id.unwrap_or(0));

            if is_running {
                let result = if (config.workload_type.as_deref() == Some("service")
                    || config.workload_type.as_deref() == Some("pod"))
                    && config.protocol == "tcp"
                {
                    stop_port_forward_cmd(
                        config.id.unwrap_or(0).to_string(),
                        self.app_handle.clone(),
                    )
                    .await
                } else if config.workload_type.as_deref() == Some("proxy")
                    || ((config.workload_type.as_deref() == Some("service")
                        || config.workload_type.as_deref() == Some("pod"))
                        && config.protocol == "udp")
                {
                    stop_proxy_forward_cmd(
                        config.id.unwrap_or(0).to_string(),
                        &config.namespace,
                        config.service.unwrap_or_default(),
                        self.app_handle.clone(),
                    )
                    .await
                } else {
                    Err(format!(
                        "Unsupported workload type for stopping: {:?}",
                        config.workload_type
                    ))
                };

                if let Err(e) = result {
                    error!(
                        "Failed to stop port forward for config {}: {}",
                        config.id.unwrap_or(0),
                        e
                    );
                } else {
                    stopped_count += 1;
                }
            } else {
                let result = if config.workload_type.as_deref() == Some("service")
                    || config.workload_type.as_deref() == Some("pod")
                {
                    if config.protocol == "tcp" {
                        start_port_forward_tcp_cmd(vec![config.clone()], self.app_handle.clone())
                            .await
                    } else {
                        deploy_and_forward_pod_cmd(vec![config.clone()], self.app_handle.clone())
                            .await
                    }
                } else {
                    deploy_and_forward_pod_cmd(vec![config.clone()], self.app_handle.clone()).await
                };

                if let Err(e) = result {
                    error!(
                        "Failed to start port forward for config {}: {}",
                        config.id.unwrap_or(0),
                        e
                    );
                } else {
                    started_count += 1;
                }
            }
        }

        if started_count > 0 || stopped_count > 0 {
            let message = match (started_count, stopped_count) {
                (0, stopped) => format!(
                    "Stopped {} port forward{}",
                    stopped,
                    if stopped == 1 { "" } else { "s" }
                ),
                (started, 0) => format!(
                    "Started {} port forward{}",
                    started,
                    if started == 1 { "" } else { "s" }
                ),
                (started, stopped) => {
                    format!("Started {}, stopped {} port forwards", started, stopped)
                }
            };
            let _ = self
                .app_handle
                .notification()
                .builder()
                .title("Port Forward")
                .body(message)
                .show();
        }

        let _ = self.app_handle.emit("port-forward-status-changed", ());

        Ok(())
    }

    fn action_type(&self) -> &str {
        "toggle_port_forward"
    }

    fn description(&self) -> &str {
        "Toggle specific port forwards"
    }
}

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
    portforward::stop_all_port_forward_cmd,
};

fn start_error(
    result: Result<Vec<kftray_commons::models::response::CustomResponse>, String>,
) -> Result<(), String> {
    kftray_commons::models::response::batch_failure(&result?)
}

/// What a batch of starts actually did.
struct StartOutcome {
    started: usize,
    failures: Vec<String>,
}

impl StartOutcome {
    /// `, N failed` for a notification, or nothing when every start succeeded.
    fn failed_suffix(&self) -> String {
        if self.failures.is_empty() {
            String::new()
        } else {
            format!(", {} failed", self.failures.len())
        }
    }

    fn into_result(self) -> Result<(), kftray_shortcuts::ShortcutError> {
        self.into_result_as("start")
    }

    /// The result of an action that may also have stopped forwards, so the
    /// wording does not claim every failure was a start.
    fn into_result_as(self, verb: &str) -> Result<(), kftray_shortcuts::ShortcutError> {
        if self.failures.is_empty() {
            Ok(())
        } else {
            Err(kftray_shortcuts::ShortcutError::ActionExecutionFailed(
                format!("Failed to {verb}: {}", self.failures.join("; ")),
            ))
        }
    }
}

/// Derives a stop batch's outcome directly from each response, tied to its
/// own config id. Reconstructing failures by splitting the string
/// `batch_failure` joins with "; " is lossy: a single response's `stderr`
/// can itself contain "; " (multi-part errors from stop/cleanup are a
/// realistic source), which would otherwise be split into bogus extra
/// entries.
fn stop_outcome_from_responses(
    responses: &[kftray_commons::models::response::CustomResponse],
) -> StartOutcome {
    let started = responses.iter().filter(|r| r.status == 0).count();
    let failures: Vec<String> = responses
        .iter()
        .filter(|r| r.failed())
        .map(|r| {
            let message = if r.stderr.trim().is_empty() {
                "failed with no error message".to_owned()
            } else {
                r.stderr.clone()
            };
            match r.id {
                Some(id) => format!("{id}: {message}"),
                None => message,
            }
        })
        .collect();
    StartOutcome { started, failures }
}

/// Starts one configuration through the same workload/protocol dispatch the
/// SSL certificate restart path uses.
async fn start_one(config: &Config) -> Result<(), String> {
    start_error(crate::commands::portforward::dispatch_start(config).await)
}

/// Stops one configuration through the same dispatch the stop and toggle
/// action handlers use, regardless of its workload type.
async fn stop_one(config: &Config) -> Result<(), String> {
    crate::commands::portforward::dispatch_stop(config)
        .await
        .map(|_| ())
}

/// Starts each configuration in turn and counts what actually started: a
/// batch where every start failed used to be announced as a success.
async fn start_each(configs: Vec<Config>) -> StartOutcome {
    let mut outcome = StartOutcome {
        started: 0,
        failures: Vec::new(),
    };
    for config in configs {
        match start_one(&config).await {
            Ok(()) => outcome.started += 1,
            Err(e) => {
                error!(
                    "Failed to start port forward for config {}: {}",
                    config.id.unwrap_or(0),
                    e
                );
                outcome
                    .failures
                    .push(format!("{}: {e}", config.id.unwrap_or(0)));
            }
        }
    }
    outcome
}

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

        let outcome = start_each(configs_to_start).await;

        let _ = self
            .app_handle
            .notification()
            .builder()
            .title("Port Forward")
            .body(format!(
                "Started {} port forward{}{}",
                outcome.started,
                if outcome.started == 1 { "" } else { "s" },
                outcome.failed_suffix()
            ))
            .show();

        let _ = self.app_handle.emit("port-forward-status-changed", ());

        outcome.into_result()
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

        // Partial counts, the same shape `StartOutcome` reports for starts:
        // a row owned by another live process is skipped rather than
        // reported, so what is left over here is this process's own rows
        // that failed to stop, not a signal that the whole batch failed.
        let outcome = match stop_all_port_forward_cmd(self.app_handle.clone())
            .await
            .map_err(|error| error.to_string())
        {
            Ok(responses) => stop_outcome_from_responses(&responses),
            Err(e) => StartOutcome {
                started: 0,
                failures: vec![e],
            },
        };

        if outcome.failures.is_empty() {
            info!("Successfully stopped all port forwards");
        } else {
            error!(
                "Failed to stop every port forward: {}",
                outcome.failures.join("; ")
            );
        }

        let _ = self
            .app_handle
            .notification()
            .builder()
            .title("Port Forward")
            .body(format!(
                "Stopped {} port forward{}{}",
                outcome.started,
                if outcome.started == 1 { "" } else { "s" },
                outcome.failed_suffix()
            ))
            .show();

        let _ = self.app_handle.emit("port-forward-status-changed", ());

        outcome.into_result_as("stop")
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

        let outcome = start_each(configs_to_start).await;

        let _ = self
            .app_handle
            .notification()
            .builder()
            .title("Port Forward")
            .body(format!(
                "Started {} selected port forward{}{}",
                outcome.started,
                if outcome.started == 1 { "" } else { "s" },
                outcome.failed_suffix()
            ))
            .show();

        let _ = self.app_handle.emit("port-forward-status-changed", ());

        outcome.into_result()
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

        let mut outcome = StartOutcome {
            started: 0,
            failures: Vec::new(),
        };

        for config in configs_to_stop {
            let config_id = config.id.unwrap_or(0);
            let result = stop_one(&config).await;

            match result {
                Ok(()) => outcome.started += 1,
                Err(e) => {
                    error!(
                        "Failed to stop port forward for config {}: {}",
                        config_id, e
                    );
                    outcome.failures.push(format!("{config_id}: {e}"));
                }
            }
        }

        let _ = self
            .app_handle
            .notification()
            .builder()
            .title("Port Forward")
            .body(format!(
                "Stopped {} selected port forward{}{}",
                outcome.started,
                if outcome.started == 1 { "" } else { "s" },
                outcome.failed_suffix()
            ))
            .show();

        let _ = self.app_handle.emit("port-forward-status-changed", ());

        outcome.into_result_as("stop")
    }

    fn action_type(&self) -> &str {
        "stop_port_forward"
    }

    fn description(&self) -> &str {
        "Stop specific port forwards"
    }
}

/// Notification text for a toggle batch. Pulled out of `execute` so a bug in
/// which count gets which wording is a plain unit test rather than one that
/// needs a running Tauri action handler.
fn toggle_message(started: usize, stopped: usize, failed_suffix: &str) -> String {
    match (started, stopped) {
        (0, 0) => format!("No port forwards toggled{}", failed_suffix),
        (0, stopped) => format!(
            "Stopped {} port forward{}{}",
            stopped,
            if stopped == 1 { "" } else { "s" },
            failed_suffix
        ),
        (started, 0) => format!(
            "Started {} port forward{}{}",
            started,
            if started == 1 { "" } else { "s" },
            failed_suffix
        ),
        (started, stopped) => format!(
            "Started {}, stopped {} port forwards{}",
            started, stopped, failed_suffix
        ),
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
        let mut failures: Vec<String> = Vec::new();

        for config in target_configs {
            let is_running = running_config_ids.contains(&config.id.unwrap_or(0));

            if is_running {
                let result = stop_one(&config).await;

                if let Err(e) = result {
                    error!(
                        "Failed to stop port forward for config {}: {}",
                        config.id.unwrap_or(0),
                        e
                    );
                    failures.push(format!("{}: {e}", config.id.unwrap_or(0)));
                } else {
                    stopped_count += 1;
                }
            } else {
                match start_one(&config).await {
                    Ok(()) => started_count += 1,
                    Err(e) => {
                        error!(
                            "Failed to start port forward for config {}: {}",
                            config.id.unwrap_or(0),
                            e
                        );
                        failures.push(format!("{}: {e}", config.id.unwrap_or(0)));
                    }
                }
            }
        }

        // A toggle whose starts all failed is a failed toggle: it is reported
        // to the caller and in the notification rather than as nothing.
        if started_count > 0 || stopped_count > 0 || !failures.is_empty() {
            let outcome = StartOutcome {
                started: started_count,
                failures,
            };
            let message = toggle_message(started_count, stopped_count, &outcome.failed_suffix());
            let _ = self
                .app_handle
                .notification()
                .builder()
                .title("Port Forward")
                .body(message)
                .show();

            let _ = self.app_handle.emit("port-forward-status-changed", ());

            return outcome.into_result_as("toggle");
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

#[cfg(test)]
mod tests {
    use kftray_commons::models::response::CustomResponse;

    use super::{
        stop_outcome_from_responses,
        toggle_message,
    };

    fn response(id: i64, status: i32, stderr: &str) -> CustomResponse {
        CustomResponse {
            id: Some(id),
            service: String::new(),
            namespace: String::new(),
            local_port: 0,
            remote_port: 0,
            context: String::new(),
            stdout: String::new(),
            stderr: stderr.to_string(),
            status,
            protocol: String::new(),
        }
    }

    #[test]
    fn stop_outcome_keeps_a_multi_part_stderr_as_one_failure() {
        // Regression: reconstructing failures by splitting the "; "-joined
        // `batch_failure` message split a single stderr that itself
        // contains "; " into several bogus entries, inflating the failure
        // count reported to the user.
        let responses = vec![
            response(1, 0, ""),
            response(2, 1, "stop failed; cleanup failed"),
        ];

        let outcome = stop_outcome_from_responses(&responses);

        assert_eq!(outcome.started, 1);
        assert_eq!(
            outcome.failures,
            vec!["2: stop failed; cleanup failed".to_string()]
        );
    }

    #[test]
    fn stop_outcome_labels_a_missing_stderr() {
        let responses = vec![response(7, 1, "")];

        let outcome = stop_outcome_from_responses(&responses);

        assert_eq!(
            outcome.failures,
            vec!["7: failed with no error message".to_string()]
        );
    }

    #[test]
    fn stopped_only_with_failures_reports_stopped_not_started() {
        // Regression: a guard on this arm used to require `failures` to be
        // empty, so this case fell through to the "Started X, stopped Y"
        // arm and reported "Started 0" for a batch that never started
        // anything.
        let message = toggle_message(0, 3, ", 2 failed");
        assert_eq!(message, "Stopped 3 port forwards, 2 failed");
    }

    #[test]
    fn stopped_only_without_failures() {
        let message = toggle_message(0, 1, "");
        assert_eq!(message, "Stopped 1 port forward");
    }

    #[test]
    fn started_and_stopped_mixed() {
        let message = toggle_message(2, 1, ", 1 failed");
        assert_eq!(message, "Started 2, stopped 1 port forwards, 1 failed");
    }

    #[test]
    fn all_failed_reports_failure_not_stopped() {
        // Regression: (0, stopped) matched (0, 0) too, so a toggle whose
        // every start failed (0 started, 0 stopped) was announced as
        // "Stopped 0 port forwards, N failed" instead of a failure-only
        // message.
        let message = toggle_message(0, 0, ", 3 failed");
        assert_eq!(message, "No port forwards toggled, 3 failed");
    }
}

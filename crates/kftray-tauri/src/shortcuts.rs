use std::sync::Arc;

use async_trait::async_trait;
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
    Manager,
};
use tokio::sync::{
    Mutex,
    OnceCell,
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

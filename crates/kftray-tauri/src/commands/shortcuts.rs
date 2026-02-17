use kftray_shortcuts::ShortcutDefinition;
use log::info;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::shortcuts::get_manager;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateShortcutRequest {
    pub name: String,
    pub shortcut_key: String,
    pub action_type: String,
    pub action_data: Option<String>,
    pub config_id: Option<i64>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutResponse {
    pub id: i64,
    pub name: String,
    pub shortcut_key: String,
    pub action_type: String,
    pub action_data: Option<String>,
    pub config_id: Option<i64>,
    pub enabled: bool,
}

impl From<ShortcutDefinition> for ShortcutResponse {
    fn from(def: ShortcutDefinition) -> Self {
        Self {
            id: def.id.unwrap_or(0),
            name: def.name,
            shortcut_key: def.shortcut_key,
            action_type: def.action_type,
            action_data: def.action_data,
            config_id: def.config_id,
            enabled: def.enabled,
        }
    }
}

impl From<CreateShortcutRequest> for ShortcutDefinition {
    fn from(req: CreateShortcutRequest) -> Self {
        Self {
            id: None,
            name: req.name,
            shortcut_key: req.shortcut_key,
            action_type: req.action_type,
            action_data: req.action_data,
            config_id: req.config_id,
            enabled: req.enabled.unwrap_or(true),
        }
    }
}

#[tauri::command]
pub async fn create_shortcut(
    _app: AppHandle, request: CreateShortcutRequest,
) -> Result<i64, String> {
    info!("Creating shortcut: {}", request.name);
    let manager = get_manager().await?;
    let mut manager = manager.lock().await;
    manager
        .create_shortcut(ShortcutDefinition::from(request))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_shortcuts(_app: AppHandle) -> Result<Vec<ShortcutResponse>, String> {
    let manager = get_manager().await?;
    let manager = manager.lock().await;
    let shortcuts = manager
        .get_all_shortcuts()
        .await
        .map_err(|e| e.to_string())?;
    Ok(shortcuts.into_iter().map(ShortcutResponse::from).collect())
}

#[tauri::command]
pub async fn update_shortcut(
    _app: AppHandle, id: i64, request: CreateShortcutRequest,
) -> Result<(), String> {
    info!("Updating shortcut ID: {}", id);
    let manager = get_manager().await?;
    let mut manager = manager.lock().await;
    let mut shortcut = ShortcutDefinition::from(request);
    shortcut.id = Some(id);
    manager
        .update_shortcut(shortcut)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_shortcut(_app: AppHandle, id: i64) -> Result<(), String> {
    info!("Deleting shortcut ID: {}", id);
    let manager = get_manager().await?;
    let mut manager = manager.lock().await;
    manager.delete_shortcut(id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn validate_shortcut_key(_app: AppHandle, shortcut_key: String) -> Result<bool, String> {
    let manager = get_manager().await?;
    let manager = manager.lock().await;
    manager
        .validate_shortcut(&shortcut_key)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_available_actions() -> Result<Vec<ActionInfo>, String> {
    Ok(vec![
        ActionInfo {
            action_type: "toggle_window".to_string(),
            description: "Toggle main application window visibility".to_string(),
            requires_config: false,
        },
        ActionInfo {
            action_type: "config_action".to_string(),
            description: "Execute action related to a specific configuration".to_string(),
            requires_config: true,
        },
    ])
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionInfo {
    pub action_type: String,
    pub description: String,
    pub requires_config: bool,
}

#[tauri::command]
pub async fn create_config_shortcut(
    _app: AppHandle, config_id: i64, name: String, shortcut_key: String, action: String,
) -> Result<i64, String> {
    info!(
        "Creating config shortcut for config {}: {} -> {}",
        config_id, name, shortcut_key
    );

    let action_data = serde_json::json!({"action": action, "config_id": config_id});
    let shortcut = ShortcutDefinition {
        id: None,
        name,
        shortcut_key,
        action_type: "config_action".to_string(),
        action_data: Some(action_data.to_string()),
        config_id: Some(config_id),
        enabled: true,
    };

    let manager = get_manager().await?;
    let mut manager = manager.lock().await;
    manager
        .create_shortcut(shortcut)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_shortcuts_by_config(
    _app: AppHandle, config_id: i64,
) -> Result<Vec<ShortcutResponse>, String> {
    let manager = get_manager().await?;
    let manager = manager.lock().await;
    let shortcuts = manager
        .get_shortcuts_by_config(config_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(shortcuts.into_iter().map(ShortcutResponse::from).collect())
}

#[tauri::command]
pub async fn test_shortcut_format_v2(shortcut_str: String) -> Result<bool, String> {
    let parser = kftray_shortcuts::ShortcutParser::new();
    Ok(parser.validate_shortcut(&shortcut_str).unwrap_or(false))
}

#[tauri::command]
pub async fn normalize_shortcut_key(shortcut_str: String) -> Result<String, String> {
    let parser = kftray_shortcuts::ShortcutParser::new();
    parser
        .normalize_shortcut(&shortcut_str)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn check_shortcut_conflicts(
    _app: AppHandle, shortcut_key: String, exclude_id: Option<i64>,
) -> Result<Vec<ShortcutResponse>, String> {
    let manager = get_manager().await?;
    let manager = manager.lock().await;
    let shortcuts = manager
        .get_all_shortcuts()
        .await
        .map_err(|e| e.to_string())?;

    let conflicts: Vec<ShortcutResponse> = shortcuts
        .into_iter()
        .filter(|s| {
            s.shortcut_key == shortcut_key
                && (exclude_id.is_none() || s.id != Some(exclude_id.unwrap_or(0)))
        })
        .map(ShortcutResponse::from)
        .collect();

    Ok(conflicts)
}

#[tauri::command]
pub async fn get_platform_status(
    app: AppHandle,
) -> Result<kftray_shortcuts::models::PlatformStatus, String> {
    let manager = get_manager().await?;
    let manager = manager.lock().await;
    let status = manager.get_platform_status();

    // Emit status update event
    let _ = app.emit("platform-status-update", &status);

    Ok(status)
}

#[tauri::command]
pub async fn try_fix_platform_permissions(app: AppHandle) -> Result<String, String> {
    let manager = get_manager().await?;
    let manager = manager.lock().await;
    let result = manager.try_fix_permissions().map_err(|e| e.to_string());

    // Emit status update events
    let _ = app.emit("permission-fix-attempted", &result);

    match &result {
        Ok(message) => {
            let _ = app.emit("permission-fix-success", message);
        }
        Err(error) => {
            let _ = app.emit("permission-fix-error", error);
        }
    }

    result
}

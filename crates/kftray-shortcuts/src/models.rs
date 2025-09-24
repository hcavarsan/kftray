use std::collections::HashMap;

use serde::{
    Deserialize,
    Serialize,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShortcutDefinition {
    pub id: Option<i64>,
    pub name: String,
    pub shortcut_key: String,
    pub action_type: String,
    pub action_data: Option<String>,
    pub config_id: Option<i64>,
    pub enabled: bool,
}

impl ShortcutDefinition {
    pub fn new(name: String, shortcut_key: String, action_type: String) -> Self {
        Self {
            id: None,
            name,
            shortcut_key,
            action_type,
            action_data: None,
            config_id: None,
            enabled: true,
        }
    }

    pub fn with_config_id(mut self, config_id: i64) -> Self {
        self.config_id = Some(config_id);
        self
    }

    pub fn with_action_data<T: Serialize>(mut self, data: &T) -> Result<Self, serde_json::Error> {
        self.action_data = Some(serde_json::to_string(data)?);
        Ok(self)
    }

    pub fn get_action_data<T: for<'de> Deserialize<'de>>(
        &self,
    ) -> Result<Option<T>, serde_json::Error> {
        match &self.action_data {
            Some(data) => Ok(Some(serde_json::from_str(data)?)),
            None => Ok(None),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionContext {
    pub shortcut_id: i64,
    pub config_id: Option<i64>,
    pub action_data: Option<String>,
    pub metadata: HashMap<String, String>,
}

impl ActionContext {
    pub fn new(shortcut_id: i64) -> Self {
        Self {
            shortcut_id,
            config_id: None,
            action_data: None,
            metadata: HashMap::new(),
        }
    }

    pub fn with_config_id(mut self, config_id: i64) -> Self {
        self.config_id = Some(config_id);
        self
    }

    pub fn with_action_data(mut self, data: String) -> Self {
        self.action_data = Some(data);
        self
    }

    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShortcutPlatform {
    Linux,
    Windows,
    MacOS,
    Generic,
}

impl ShortcutPlatform {
    pub fn current() -> Self {
        #[cfg(target_os = "linux")]
        return Self::Linux;

        #[cfg(target_os = "windows")]
        return Self::Windows;

        #[cfg(target_os = "macos")]
        return Self::MacOS;

        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        return Self::Generic;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutRegistration {
    pub id: String,
    pub definition: ShortcutDefinition,
    pub platform: ShortcutPlatform,
    pub registered_at: chrono::DateTime<chrono::Utc>,
}

pub type ShortcutResult<T> = Result<T, ShortcutError>;

#[derive(Debug, thiserror::Error)]
pub enum ShortcutError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Invalid shortcut key: {0}")]
    InvalidShortcut(String),

    #[error("Shortcut already registered: {0}")]
    AlreadyRegistered(String),

    #[error("Shortcut not found: {0}")]
    NotFound(String),

    #[error("Platform not supported")]
    PlatformNotSupported,

    #[error("Platform error: {0}")]
    PlatformError(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Action execution failed: {0}")]
    ActionExecutionFailed(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformStatus {
    pub platform: String,
    pub is_wayland: bool,
    pub has_input_permissions: bool,
    pub needs_permission_fix: bool,
    pub current_implementation: String,
    pub can_fix_permissions: bool,
}

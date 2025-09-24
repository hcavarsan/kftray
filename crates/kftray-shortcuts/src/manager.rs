use std::collections::HashMap;
use std::sync::Arc;

use log::{
    error,
    info,
};
use sqlx::SqlitePool;
use tokio::sync::Mutex;

use crate::actions::{
    ActionRegistry,
    create_default_registry,
};
use crate::models::{
    PlatformStatus,
    ShortcutDefinition,
    ShortcutError,
    ShortcutResult,
};
use crate::parser::ShortcutParser;
use crate::platforms::{
    PlatformManager,
    create_platform_manager,
};
use crate::storage::ShortcutStorage;

pub struct ShortcutManager {
    storage: ShortcutStorage,
    platform_manager: Box<dyn PlatformManager>,
    action_registry: Arc<Mutex<ActionRegistry>>,
    parser: ShortcutParser,
    registered_shortcuts: HashMap<i64, ShortcutDefinition>,
}

impl ShortcutManager {
    pub async fn new(pool: SqlitePool) -> ShortcutResult<Self> {
        let storage = ShortcutStorage::new(pool);
        let action_registry = Arc::new(Mutex::new(create_default_registry()));

        let platform_manager = create_platform_manager(action_registry.clone())
            .await
            .map_err(|e| {
                error!("Failed to create platform manager: {}", e);
                error!("Shortcuts will not be functional on this system");
                e
            })?;

        let parser = ShortcutParser::new();
        let registered_shortcuts = HashMap::new();

        info!(
            "Created shortcut manager with platform: {}",
            platform_manager.platform_name().await
        );

        Ok(Self {
            storage,
            platform_manager,
            action_registry,
            parser,
            registered_shortcuts,
        })
    }

    pub async fn with_custom_registry(
        pool: SqlitePool, registry: ActionRegistry,
    ) -> ShortcutResult<Self> {
        let storage = ShortcutStorage::new(pool);
        let action_registry = Arc::new(Mutex::new(registry));

        let platform_manager = create_platform_manager(action_registry.clone())
            .await
            .map_err(|e| {
                error!("Failed to create platform manager: {}", e);
                error!("Shortcuts will not be functional on this system");
                e
            })?;

        let parser = ShortcutParser::new();
        let registered_shortcuts = HashMap::new();

        info!(
            "Created shortcut manager with platform: {}",
            platform_manager.platform_name().await
        );

        Ok(Self {
            storage,
            platform_manager,
            action_registry,
            parser,
            registered_shortcuts,
        })
    }

    pub async fn initialize(&mut self) -> ShortcutResult<()> {
        info!("Initializing shortcut manager");

        let shortcuts = self.storage.get_all_enabled_shortcuts().await?;

        for shortcut in shortcuts {
            if let Err(e) = self.register_platform_shortcut(&shortcut).await {
                error!(
                    "Failed to register shortcut '{}' during initialization: {}",
                    shortcut.name, e
                );
            }
        }

        info!(
            "Shortcut manager initialized with {} shortcuts",
            self.registered_shortcuts.len()
        );
        Ok(())
    }

    pub async fn start_event_loop(&self) {
        let platform_name = self.platform_manager.platform_name().await;
        info!(
            "Starting shortcut event loop for platform: {}",
            platform_name
        );
        info!("Platform-specific event loops are handled by each platform manager");
    }

    pub async fn create_shortcut(
        &mut self, mut shortcut: ShortcutDefinition,
    ) -> ShortcutResult<i64> {
        if self.storage.shortcut_exists(&shortcut.name).await? {
            return Err(ShortcutError::AlreadyRegistered(shortcut.name));
        }

        self.parser.parse(&shortcut.shortcut_key)?;

        let shortcut_id = self.storage.create_shortcut(&shortcut).await?;
        shortcut.id = Some(shortcut_id);

        if shortcut.enabled
            && let Err(e) = self.register_platform_shortcut(&shortcut).await
        {
            error!("Failed to register platform shortcut after creation: {}", e);
            if let Err(cleanup_err) = self.storage.delete_shortcut(shortcut_id).await {
                error!(
                    "Failed to cleanup shortcut after registration failure: {}",
                    cleanup_err
                );
            }
            return Err(e);
        }

        info!(
            "Created shortcut '{}' with ID: {}",
            shortcut.name, shortcut_id
        );
        Ok(shortcut_id)
    }

    pub async fn get_shortcut(&self, id: i64) -> ShortcutResult<Option<ShortcutDefinition>> {
        self.storage.get_shortcut_by_id(id).await
    }

    pub async fn get_shortcut_by_name(
        &self, name: &str,
    ) -> ShortcutResult<Option<ShortcutDefinition>> {
        self.storage.get_shortcut_by_name(name).await
    }

    pub async fn get_all_shortcuts(&self) -> ShortcutResult<Vec<ShortcutDefinition>> {
        self.storage.get_all_enabled_shortcuts().await
    }

    pub async fn get_shortcuts_by_config(
        &self, config_id: i64,
    ) -> ShortcutResult<Vec<ShortcutDefinition>> {
        self.storage.get_shortcuts_by_config(config_id).await
    }

    pub async fn update_shortcut(&mut self, shortcut: ShortcutDefinition) -> ShortcutResult<()> {
        let shortcut_id = shortcut.id.ok_or_else(|| {
            ShortcutError::Internal("Shortcut ID required for update".to_string())
        })?;

        let existing = self
            .storage
            .get_shortcut_by_id(shortcut_id)
            .await?
            .ok_or_else(|| ShortcutError::NotFound(shortcut_id.to_string()))?;

        self.parser.parse(&shortcut.shortcut_key)?;

        if existing.shortcut_key != shortcut.shortcut_key || existing.enabled != shortcut.enabled {
            if self.registered_shortcuts.contains_key(&shortcut_id) {
                self.platform_manager
                    .unregister_shortcut(shortcut_id)
                    .await?;
                self.registered_shortcuts.remove(&shortcut_id);
            }

            if shortcut.enabled {
                self.register_platform_shortcut(&shortcut).await?;
            }
        }

        self.storage.update_shortcut(&shortcut).await?;
        info!("Updated shortcut ID: {}", shortcut_id);
        Ok(())
    }

    pub async fn delete_shortcut(&mut self, id: i64) -> ShortcutResult<()> {
        if self.registered_shortcuts.contains_key(&id) {
            self.platform_manager.unregister_shortcut(id).await?;
            self.registered_shortcuts.remove(&id);
        }

        self.action_registry
            .lock()
            .await
            .unregister_shortcut_definition(id);
        self.storage.delete_shortcut(id).await?;
        info!("Deleted shortcut ID: {}", id);
        Ok(())
    }

    pub async fn delete_shortcuts_by_config(&mut self, config_id: i64) -> ShortcutResult<u64> {
        let shortcuts = self.storage.get_shortcuts_by_config(config_id).await?;

        for shortcut in shortcuts {
            if let Some(id) = shortcut.id
                && self.registered_shortcuts.contains_key(&id)
            {
                if let Err(e) = self.platform_manager.unregister_shortcut(id).await {
                    error!(
                        "Failed to unregister shortcut {} during config deletion: {}",
                        id, e
                    );
                }
                self.registered_shortcuts.remove(&id);
                self.action_registry
                    .lock()
                    .await
                    .unregister_shortcut_definition(id);
            }
        }

        let deleted_count = self.storage.delete_shortcuts_by_config(config_id).await?;
        info!(
            "Deleted {} shortcuts for config ID: {}",
            deleted_count, config_id
        );
        Ok(deleted_count)
    }

    pub async fn enable_shortcut(&mut self, id: i64) -> ShortcutResult<()> {
        let shortcut = self
            .storage
            .get_shortcut_by_id(id)
            .await?
            .ok_or_else(|| ShortcutError::NotFound(id.to_string()))?;

        if !shortcut.enabled {
            self.storage.enable_shortcut(id).await?;
            let mut enabled_shortcut = shortcut;
            enabled_shortcut.enabled = true;
            self.register_platform_shortcut(&enabled_shortcut).await?;
            info!("Enabled shortcut ID: {}", id);
        }

        Ok(())
    }

    pub async fn disable_shortcut(&mut self, id: i64) -> ShortcutResult<()> {
        if self.registered_shortcuts.contains_key(&id) {
            self.platform_manager.unregister_shortcut(id).await?;
            self.registered_shortcuts.remove(&id);
        }

        self.action_registry
            .lock()
            .await
            .unregister_shortcut_definition(id);
        self.storage.disable_shortcut(id).await?;
        info!("Disabled shortcut ID: {}", id);
        Ok(())
    }

    pub async fn register_action_handler(
        &mut self, handler: Arc<dyn crate::actions::ActionHandler>,
    ) {
        self.action_registry.lock().await.register_handler(handler);
    }

    pub async fn list_actions(&self) -> Vec<(String, String)> {
        let registry = self.action_registry.lock().await;
        registry
            .list_actions()
            .into_iter()
            .map(|(action_type, description)| (action_type.to_string(), description.to_string()))
            .collect()
    }

    pub fn validate_shortcut(&self, shortcut_str: &str) -> ShortcutResult<bool> {
        self.parser.validate_shortcut(shortcut_str)
    }

    pub fn normalize_shortcut(&self, shortcut_str: &str) -> ShortcutResult<String> {
        self.parser.normalize_shortcut(shortcut_str)
    }

    async fn register_platform_shortcut(
        &mut self, shortcut: &ShortcutDefinition,
    ) -> ShortcutResult<()> {
        let shortcut_id = shortcut.id.ok_or_else(|| {
            ShortcutError::Internal("Shortcut ID required for platform registration".to_string())
        })?;

        self.parser.parse(&shortcut.shortcut_key)?;

        self.platform_manager.register_shortcut(shortcut).await?;
        self.registered_shortcuts
            .insert(shortcut_id, shortcut.clone());
        self.action_registry
            .lock()
            .await
            .register_shortcut_definition(shortcut.clone());
        Ok(())
    }

    pub fn get_platform_status(&self) -> PlatformStatus {
        #[cfg(target_os = "linux")]
        {
            use crate::platforms::linux::LinuxPlatform;

            let is_wayland = std::env::var("WAYLAND_DISPLAY").is_ok()
                || std::env::var("XDG_SESSION_TYPE").map_or(false, |t| t == "wayland");
            let needs_fix = LinuxPlatform::needs_permission_fix();
            let has_permissions = !needs_fix || !is_wayland;

            let current_implementation = if is_wayland && has_permissions {
                "evdev (Wayland native)".to_string()
            } else if is_wayland && !has_permissions {
                "global-hotkey (X11 compatibility - limited functionality)".to_string()
            } else {
                "global-hotkey (X11)".to_string()
            };

            PlatformStatus {
                platform: "linux".to_string(),
                is_wayland,
                has_input_permissions: has_permissions,
                needs_permission_fix: needs_fix,
                current_implementation,
                can_fix_permissions: is_wayland,
            }
        }

        #[cfg(target_os = "macos")]
        {
            PlatformStatus {
                platform: "macos".to_string(),
                is_wayland: false,
                has_input_permissions: true,
                needs_permission_fix: false,
                current_implementation: "global-hotkey (native)".to_string(),
                can_fix_permissions: false,
            }
        }

        #[cfg(target_os = "windows")]
        {
            PlatformStatus {
                platform: "windows".to_string(),
                is_wayland: false,
                has_input_permissions: true,
                needs_permission_fix: false,
                current_implementation: "global-hotkey (native)".to_string(),
                can_fix_permissions: false,
            }
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            PlatformStatus {
                platform: "unknown".to_string(),
                is_wayland: false,
                has_input_permissions: false,
                needs_permission_fix: false,
                current_implementation: "unsupported".to_string(),
                can_fix_permissions: false,
            }
        }
    }

    pub fn try_fix_permissions(&self) -> ShortcutResult<String> {
        #[cfg(target_os = "linux")]
        {
            use crate::platforms::linux::LinuxPlatform;
            LinuxPlatform::try_fix_permissions().map_err(|e| ShortcutError::PlatformError(e))
        }

        #[cfg(not(target_os = "linux"))]
        Err(ShortcutError::PlatformError(
            "Permission fix only available on Linux".to_string(),
        ))
    }

    pub async fn shutdown(&mut self) -> ShortcutResult<()> {
        info!("Shutting down shortcut manager");
        for &shortcut_id in self.registered_shortcuts.keys().collect::<Vec<_>>().iter() {
            if let Err(e) = self
                .platform_manager
                .unregister_shortcut(*shortcut_id)
                .await
            {
                error!(
                    "Failed to unregister shortcut {} during shutdown: {}",
                    shortcut_id, e
                );
            }
        }
        self.registered_shortcuts.clear();
        info!("Shortcut manager shut down");
        Ok(())
    }
}

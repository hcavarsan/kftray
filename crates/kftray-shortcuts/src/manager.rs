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

        let rebind =
            existing.shortcut_key != shortcut.shortcut_key || existing.enabled != shortcut.enabled;
        let was_registered = self.registered_shortcuts.contains_key(&shortcut_id);

        if rebind {
            if was_registered {
                self.platform_manager
                    .unregister_shortcut(shortcut_id)
                    .await?;
                self.registered_shortcuts.remove(&shortcut_id);
                self.action_registry
                    .lock()
                    .await
                    .unregister_shortcut_definition(shortcut_id);
            }

            if shortcut.enabled
                && let Err(e) = self.register_platform_shortcut(&shortcut).await
            {
                if was_registered
                    && let Err(restore_err) = self.register_platform_shortcut(&existing).await
                {
                    error!(
                        "Failed to restore shortcut ID {} after a failed update: {}",
                        shortcut_id, restore_err
                    );
                }
                return Err(e);
            }
        }

        if let Err(e) = self.storage.update_shortcut(&shortcut).await {
            if rebind {
                if self.registered_shortcuts.contains_key(&shortcut_id) {
                    if let Err(cleanup_err) =
                        self.platform_manager.unregister_shortcut(shortcut_id).await
                    {
                        error!(
                            "Failed to unregister shortcut ID {} after a failed storage update: {}",
                            shortcut_id, cleanup_err
                        );
                        // Keep tracking the binding that the platform could not remove.
                        return Err(e);
                    }
                    self.registered_shortcuts.remove(&shortcut_id);
                    self.action_registry
                        .lock()
                        .await
                        .unregister_shortcut_definition(shortcut_id);
                }

                if was_registered
                    && let Err(restore_err) = self.register_platform_shortcut(&existing).await
                {
                    error!(
                        "Failed to restore shortcut ID {} after a failed storage update: {}",
                        shortcut_id, restore_err
                    );
                }
            }
            return Err(e);
        }

        if !rebind && self.registered_shortcuts.contains_key(&shortcut_id) {
            self.registered_shortcuts
                .insert(shortcut_id, shortcut.clone());
            self.action_registry
                .lock()
                .await
                .register_shortcut_definition(shortcut);
        }

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

    pub async fn reload_registered_shortcuts(&mut self) -> ShortcutResult<()> {
        let ids: Vec<i64> = self.registered_shortcuts.keys().copied().collect();
        for id in ids {
            match self.storage.get_shortcut_by_id(id).await {
                Ok(Some(shortcut)) if shortcut.enabled => {
                    self.registered_shortcuts.insert(id, shortcut.clone());
                    self.action_registry
                        .lock()
                        .await
                        .register_shortcut_definition(shortcut);
                }
                lookup => {
                    if let Err(e) = lookup {
                        error!("Failed to reload shortcut {}: {}", id, e);
                    }
                    if let Err(e) = self.platform_manager.unregister_shortcut(id).await {
                        error!("Failed to unregister removed shortcut {}: {}", id, e);
                    }
                    self.registered_shortcuts.remove(&id);
                    self.action_registry
                        .lock()
                        .await
                        .unregister_shortcut_definition(id);
                }
            }
        }
        Ok(())
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
                || std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland");
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
            LinuxPlatform::try_fix_permissions().map_err(ShortcutError::PlatformError)
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

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use super::*;
    use crate::actions::ActionHandler;
    use crate::models::ActionContext;

    struct NoopPlatform;

    #[async_trait]
    impl PlatformManager for NoopPlatform {
        async fn register_shortcut(&mut self, _: &ShortcutDefinition) -> ShortcutResult<()> {
            Ok(())
        }

        async fn unregister_shortcut(&mut self, _: i64) -> ShortcutResult<()> {
            Ok(())
        }

        async fn is_available(&self) -> bool {
            true
        }

        async fn platform_name(&self) -> &str {
            "noop"
        }
    }

    type Executed = Arc<Mutex<Vec<(String, Option<String>, Option<i64>)>>>;

    struct Recorder {
        action_type: &'static str,
        executed: Executed,
    }

    #[async_trait]
    impl ActionHandler for Recorder {
        async fn execute(&self, context: &ActionContext) -> ShortcutResult<()> {
            self.executed.lock().await.push((
                self.action_type.to_string(),
                context.action_data.clone(),
                context.config_id,
            ));
            Ok(())
        }

        fn action_type(&self) -> &str {
            self.action_type
        }

        fn description(&self) -> &str {
            self.action_type
        }
    }

    async fn test_manager(executed: &Executed) -> ShortcutManager {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE shortcuts (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                shortcut_key TEXT NOT NULL,
                action_type TEXT NOT NULL,
                action_data TEXT,
                config_id INTEGER,
                enabled BOOLEAN NOT NULL DEFAULT true,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .execute(&pool)
        .await
        .unwrap();

        let mut registry = ActionRegistry::new();
        for action_type in ["start", "stop"] {
            registry.register_handler(Arc::new(Recorder {
                action_type,
                executed: executed.clone(),
            }));
        }

        ShortcutManager {
            storage: ShortcutStorage::new(pool),
            platform_manager: Box::new(NoopPlatform),
            action_registry: Arc::new(Mutex::new(registry)),
            parser: ShortcutParser::new(),
            registered_shortcuts: HashMap::new(),
        }
    }

    async fn press(manager: &ShortcutManager, id: i64) {
        manager
            .action_registry
            .lock()
            .await
            .execute_by_id(id, &ActionContext::new(id))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn update_shortcut_without_key_change_runs_the_new_action() {
        let executed = Executed::default();
        let mut manager = test_manager(&executed).await;

        let mut shortcut =
            ShortcutDefinition::new("s".into(), "Ctrl+Shift+F1".into(), "start".into());
        shortcut.action_data = Some(r#"{"config_ids":[1]}"#.into());
        let id = manager.create_shortcut(shortcut.clone()).await.unwrap();
        shortcut.id = Some(id);

        shortcut.action_data = Some(r#"{"config_ids":[2]}"#.into());
        shortcut.config_id = Some(2);
        manager.update_shortcut(shortcut.clone()).await.unwrap();
        press(&manager, id).await;

        shortcut.action_type = "stop".into();
        manager.update_shortcut(shortcut.clone()).await.unwrap();
        press(&manager, id).await;

        assert_eq!(
            *executed.lock().await,
            vec![
                (
                    "start".to_string(),
                    Some(r#"{"config_ids":[2]}"#.to_string()),
                    Some(2)
                ),
                (
                    "stop".to_string(),
                    Some(r#"{"config_ids":[2]}"#.to_string()),
                    Some(2)
                ),
            ]
        );
        assert_eq!(manager.registered_shortcuts.get(&id), Some(&shortcut));
    }

    struct RejectKey(&'static str);

    #[derive(Default)]
    struct PlatformRecording {
        calls: Vec<(i64, Option<String>)>,
        registered: HashMap<i64, ShortcutDefinition>,
        fail_at: Option<usize>,
    }

    struct RecordingPlatform(Arc<Mutex<PlatformRecording>>);

    #[async_trait]
    impl PlatformManager for RecordingPlatform {
        async fn register_shortcut(&mut self, shortcut: &ShortcutDefinition) -> ShortcutResult<()> {
            let id = shortcut.id.unwrap();
            let mut recording = self.0.lock().await;
            recording
                .calls
                .push((id, Some(shortcut.shortcut_key.clone())));
            if recording.fail_at == Some(recording.calls.len()) {
                return Err(ShortcutError::PlatformError("Registration failed".into()));
            }
            recording.registered.insert(id, shortcut.clone());
            Ok(())
        }

        async fn unregister_shortcut(&mut self, id: i64) -> ShortcutResult<()> {
            let mut recording = self.0.lock().await;
            recording.calls.push((id, None));
            if recording.fail_at == Some(recording.calls.len()) {
                return Err(ShortcutError::PlatformError("Unregistration failed".into()));
            }
            recording.registered.remove(&id);
            Ok(())
        }

        async fn is_available(&self) -> bool {
            true
        }

        async fn platform_name(&self) -> &str {
            "recording"
        }
    }

    #[async_trait]
    impl PlatformManager for RejectKey {
        async fn register_shortcut(&mut self, shortcut: &ShortcutDefinition) -> ShortcutResult<()> {
            if shortcut.shortcut_key == self.0 {
                return Err(ShortcutError::PlatformError("Unknown key: pause".into()));
            }
            Ok(())
        }

        async fn unregister_shortcut(&mut self, _: i64) -> ShortcutResult<()> {
            Ok(())
        }

        async fn is_available(&self) -> bool {
            true
        }

        async fn platform_name(&self) -> &str {
            "reject"
        }
    }

    #[tokio::test]
    async fn failed_rebind_keeps_old_shortcut() {
        let executed = Executed::default();
        let mut manager = test_manager(&executed).await;
        manager.platform_manager = Box::new(RejectKey("Ctrl+Shift+Pause"));

        let mut shortcut =
            ShortcutDefinition::new("s".into(), "Ctrl+Shift+F1".into(), "start".into());
        let id = manager.create_shortcut(shortcut.clone()).await.unwrap();
        shortcut.id = Some(id);
        let old = shortcut.clone();

        shortcut.shortcut_key = "Ctrl+Shift+Pause".into();
        assert!(manager.update_shortcut(shortcut).await.is_err());

        assert_eq!(manager.registered_shortcuts.get(&id), Some(&old));
        press(&manager, id).await;
        assert_eq!(executed.lock().await.len(), 1);
    }

    #[tokio::test]
    async fn failed_storage_update_keeps_old_shortcut() {
        for scenario in [
            "key",
            "disable",
            "enable",
            "metadata",
            "unregister",
            "restore",
        ] {
            let executed = Executed::default();
            let mut manager = test_manager(&executed).await;
            let recording = Arc::new(Mutex::new(PlatformRecording::default()));
            manager.platform_manager = Box::new(RecordingPlatform(recording.clone()));

            let mut shortcut =
                ShortcutDefinition::new("s".into(), "Ctrl+Shift+F1".into(), "start".into());
            shortcut.action_data = Some(r#"{"config_ids":[1]}"#.into());
            shortcut.config_id = Some(1);
            shortcut.enabled = scenario != "enable";
            let id = manager.create_shortcut(shortcut).await.unwrap();
            let old = manager.get_shortcut(id).await.unwrap().unwrap();
            let other_id = manager
                .create_shortcut(ShortcutDefinition::new(
                    "t".into(),
                    "Ctrl+Shift+F2".into(),
                    "start".into(),
                ))
                .await
                .unwrap();
            let other = manager.get_shortcut(other_id).await.unwrap().unwrap();

            let mut edited = old.clone();
            edited.name = other.name.clone();
            edited.action_type = "stop".into();
            edited.action_data = Some(r#"{"config_ids":[2]}"#.into());
            edited.config_id = Some(2);
            if scenario != "metadata" {
                edited.shortcut_key = other.shortcut_key.clone();
            }
            edited.enabled = scenario != "disable";
            {
                let mut recording = recording.lock().await;
                recording.calls.clear();
                recording.fail_at = match scenario {
                    "unregister" => Some(3),
                    "restore" => Some(4),
                    _ => None,
                };
            }

            assert!(matches!(
                manager.update_shortcut(edited.clone()).await,
                Err(ShortcutError::Database(_))
            ));
            assert_eq!(manager.get_shortcut(id).await.unwrap(), Some(old.clone()));
            assert_eq!(
                manager.get_shortcut(other_id).await.unwrap(),
                Some(other.clone())
            );
            assert_eq!(manager.registered_shortcuts.get(&other_id), Some(&other));

            let expected = match scenario {
                "enable" | "restore" => None,
                "unregister" => Some(&edited),
                _ => Some(&old),
            };
            assert_eq!(
                manager.registered_shortcuts.get(&id),
                expected,
                "{scenario}"
            );
            if let Some(expected) = expected {
                press(&manager, id).await;
                assert_eq!(
                    *executed.lock().await,
                    vec![(
                        expected.action_type.clone(),
                        expected.action_data.clone(),
                        expected.config_id,
                    )],
                    "{scenario}"
                );
            } else {
                assert!(
                    manager
                        .action_registry
                        .lock()
                        .await
                        .execute_by_id(id, &ActionContext::new(id))
                        .await
                        .is_err()
                );
            }

            let recording = recording.lock().await;
            assert_eq!(recording.registered.get(&id), expected, "{scenario}");
            assert_eq!(recording.registered.get(&other_id), Some(&other));
            let unregister = (id, None);
            let register_old = (id, Some(old.shortcut_key.clone()));
            let register_new = (id, Some(edited.shortcut_key.clone()));
            let calls = match scenario {
                "metadata" => vec![],
                "disable" => vec![unregister, register_old],
                "enable" => vec![register_new, unregister],
                "unregister" => vec![unregister.clone(), register_new, unregister],
                _ => vec![unregister.clone(), register_new, unregister, register_old],
            };
            assert_eq!(recording.calls, calls, "{scenario}");
        }
    }

    #[tokio::test]
    async fn successful_rebind_updates_platform_storage_and_action() {
        let executed = Executed::default();
        let mut manager = test_manager(&executed).await;
        let recording = Arc::new(Mutex::new(PlatformRecording::default()));
        manager.platform_manager = Box::new(RecordingPlatform(recording.clone()));
        let id = manager
            .create_shortcut(ShortcutDefinition::new(
                "s".into(),
                "Ctrl+Shift+F1".into(),
                "start".into(),
            ))
            .await
            .unwrap();
        let mut shortcut = manager.get_shortcut(id).await.unwrap().unwrap();
        shortcut.shortcut_key = "Ctrl+Shift+F2".into();
        shortcut.action_type = "stop".into();
        shortcut.action_data = Some(r#"{"config_ids":[2]}"#.into());
        shortcut.config_id = Some(2);

        for enabled in [true, false, true] {
            shortcut.enabled = enabled;
            manager.update_shortcut(shortcut.clone()).await.unwrap();
            assert_eq!(
                manager.get_shortcut(id).await.unwrap(),
                Some(shortcut.clone())
            );
            let expected = enabled.then_some(&shortcut);
            assert_eq!(manager.registered_shortcuts.get(&id), expected);
            assert_eq!(recording.lock().await.registered.get(&id), expected);
            if enabled {
                press(&manager, id).await;
                assert_eq!(
                    executed.lock().await.last(),
                    Some(&("stop".to_string(), shortcut.action_data.clone(), Some(2),))
                );
            } else {
                assert!(
                    manager
                        .action_registry
                        .lock()
                        .await
                        .execute_by_id(id, &ActionContext::new(id))
                        .await
                        .is_err()
                );
            }
        }
    }

    #[tokio::test]
    async fn reload_follows_targets_changed_or_removed_in_storage() {
        let executed = Executed::default();
        let mut manager = test_manager(&executed).await;

        let mut shortcut =
            ShortcutDefinition::new("s".into(), "Ctrl+Shift+F1".into(), "start".into());
        shortcut.action_data = Some(r#"{"config_ids":[1,2]}"#.into());
        let kept = manager.create_shortcut(shortcut.clone()).await.unwrap();
        shortcut.name = "t".into();
        shortcut.shortcut_key = "Ctrl+Shift+F2".into();
        shortcut.action_data = Some(r#"{"config_ids":[2]}"#.into());
        let removed = manager.create_shortcut(shortcut).await.unwrap();

        let mut changed = manager.get_shortcut(kept).await.unwrap().unwrap();
        changed.action_data = Some(r#"{"config_ids":[1]}"#.into());
        manager.storage.update_shortcut(&changed).await.unwrap();
        manager.storage.delete_shortcut(removed).await.unwrap();

        manager.reload_registered_shortcuts().await.unwrap();
        press(&manager, kept).await;

        assert_eq!(
            *executed.lock().await,
            vec![(
                "start".to_string(),
                Some(r#"{"config_ids":[1]}"#.to_string()),
                None
            )]
        );
        assert!(!manager.registered_shortcuts.contains_key(&removed));
        assert!(
            manager
                .action_registry
                .lock()
                .await
                .execute_by_id(removed, &ActionContext::new(removed))
                .await
                .is_err()
        );
    }
}

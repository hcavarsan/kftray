use std::collections::HashMap;
use std::sync::{
    Arc,
    Mutex,
};

use log::{
    error,
    info,
    warn,
};
use tauri::{
    AppHandle,
    Emitter,
    Manager,
};

use crate::shortcut::{
    GlobalShortcutManager,
    parse_shortcut_string,
};

pub struct ShortcutManagerState {
    manager: Arc<Mutex<GlobalShortcutManager>>,
    registered_shortcuts: Arc<Mutex<HashMap<String, String>>>,
}

impl Default for ShortcutManagerState {
    fn default() -> Self {
        Self::new()
    }
}

impl ShortcutManagerState {
    pub fn new() -> Self {
        Self {
            manager: Arc::new(Mutex::new(GlobalShortcutManager::new())),
            registered_shortcuts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn register_shortcut<F>(
        &self, shortcut_id: String, shortcut_str: String, callback: F,
    ) -> Result<(), String>
    where
        F: Fn() + Send + Sync + 'static,
    {
        if parse_shortcut_string(&shortcut_str).is_none() {
            return Err(format!("Invalid shortcut format: {}", shortcut_str));
        }

        #[cfg(target_os = "linux")]
        {
            use std::process::Command;
            if let Ok(output) = Command::new("groups").output() {
                let groups = String::from_utf8_lossy(&output.stdout);
                if !groups.contains("input") {
                    return Err(
                        "Linux: User not in 'input' group. Run: sudo usermod -a -G input $USER"
                            .to_string(),
                    );
                }
            }
        }

        let mut manager = self
            .manager
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;

        match manager.register_shortcut(&shortcut_str, callback) {
            Ok(_) => {
                let mut shortcuts = self
                    .registered_shortcuts
                    .lock()
                    .map_err(|e| format!("Lock error: {}", e))?;
                shortcuts.insert(shortcut_id.clone(), shortcut_str.clone());
                info!(
                    "Registered global shortcut '{}': {}",
                    shortcut_id, shortcut_str
                );
                Ok(())
            }
            Err(e) => {
                error!("Failed to register shortcut '{}': {}", shortcut_str, e);
                #[cfg(target_os = "linux")]
                {
                    if e.to_string().contains("Permission denied") || e.to_string().contains("grab")
                    {
                        return Err("Permission denied. Add user to 'input' group: sudo usermod -a -G input $USER".to_string());
                    }
                }
                Err(format!("Failed to register shortcut: {}", e))
            }
        }
    }

    pub fn unregister_shortcut(&self, shortcut_id: &str) -> Result<(), String> {
        let mut shortcuts = self
            .registered_shortcuts
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;

        if let Some(shortcut_str) = shortcuts.remove(shortcut_id) {
            let mut manager = self
                .manager
                .lock()
                .map_err(|e| format!("Lock error: {}", e))?;

            match manager.unregister_shortcut(&shortcut_str) {
                Ok(_) => {
                    info!(
                        "Unregistered global shortcut '{}': {}",
                        shortcut_id, shortcut_str
                    );
                    Ok(())
                }
                Err(e) => {
                    error!("Failed to unregister shortcut '{}': {}", shortcut_str, e);
                    shortcuts.insert(shortcut_id.to_string(), shortcut_str);
                    Err(format!("Failed to unregister shortcut: {}", e))
                }
            }
        } else {
            warn!("Shortcut '{}' not found", shortcut_id);
            Err(format!("Shortcut '{}' not found", shortcut_id))
        }
    }

    pub fn get_registered_shortcuts(&self) -> Result<HashMap<String, String>, String> {
        let shortcuts = self
            .registered_shortcuts
            .lock()
            .map_err(|e| format!("Lock error: {}", e))?;
        Ok(shortcuts.clone())
    }
}

#[tauri::command]
pub async fn register_global_shortcut(
    app: AppHandle, shortcut_id: String, shortcut_str: String, action: String,
) -> Result<(), String> {
    let state = app.state::<ShortcutManagerState>();

    let app_handle = app.clone();
    let action_clone = action.clone();
    let shortcut_id_clone = shortcut_id.clone();

    state.register_shortcut(shortcut_id.clone(), shortcut_str, move || {
        let _ = app_handle.emit(
            &format!("shortcut-triggered:{}", shortcut_id_clone),
            &action_clone,
        );
        info!(
            "Global shortcut triggered: {} -> {}",
            shortcut_id_clone, action_clone
        );
    })
}

#[tauri::command]
pub async fn unregister_global_shortcut(app: AppHandle, shortcut_id: String) -> Result<(), String> {
    let state = app.state::<ShortcutManagerState>();
    state.unregister_shortcut(&shortcut_id)
}

#[tauri::command]
pub async fn get_registered_shortcuts(app: AppHandle) -> Result<HashMap<String, String>, String> {
    let state = app.state::<ShortcutManagerState>();
    state.get_registered_shortcuts()
}

#[tauri::command]
pub async fn test_shortcut_format(shortcut_str: String) -> Result<bool, String> {
    Ok(parse_shortcut_string(&shortcut_str).is_some())
}

#[tauri::command]
pub async fn check_linux_permissions() -> Result<(bool, bool), String> {
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;

        let is_linux = true;
        let has_permissions = match Command::new("groups").output() {
            Ok(output) => {
                let groups = String::from_utf8_lossy(&output.stdout);
                info!("User groups: {}", groups.trim());
                groups.split_whitespace().any(|group| group == "input")
            }
            Err(_) => false,
        };
        Ok((is_linux, has_permissions))
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok((false, true)) // Not Linux, permissions not relevant
    }
}

#[tauri::command]
pub async fn try_fix_linux_permissions() -> Result<bool, String> {
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;

        // Try pkexec first (fail fast)
        let current_user = std::env::var("USER").unwrap_or_else(|_| "unknown".to_string());

        info!(
            "Attempting to add user '{}' to input group via pkexec",
            current_user
        );

        let result = Command::new("pkexec")
            .args(&["/usr/sbin/usermod", "-a", "-G", "input", &current_user])
            .output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    info!("Successfully added user to input group via pkexec");

                    // Verify the change worked by checking /etc/group
                    match std::fs::read_to_string("/etc/group") {
                        Ok(group_file) => {
                            if let Some(input_line) =
                                group_file.lines().find(|line| line.starts_with("input:"))
                            {
                                info!("Input group line: {}", input_line);
                                if input_line.contains(&current_user) {
                                    info!("User successfully added to input group in /etc/group");
                                } else {
                                    warn!("User not found in input group line after usermod");
                                }
                            }
                        }
                        Err(e) => warn!("Could not read /etc/group to verify: {}", e),
                    }

                    Ok(true)
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    warn!("pkexec usermod failed - stderr: {}", stderr);
                    Ok(false)
                }
            }
            Err(e) => {
                info!("pkexec not available or failed: {}", e);
                Ok(false)
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(true)
    }
}

pub fn setup_shortcut_manager(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    app.manage(ShortcutManagerState::new());

    #[cfg(target_os = "linux")]
    {
        use std::process::Command;

        let output = Command::new("groups").output();
        match output {
            Ok(output) => {
                let groups = String::from_utf8_lossy(&output.stdout);
                warn!("Setup check - User groups: {}", groups.trim());
                if !groups.split_whitespace().any(|group| group == "input") {
                    warn!("User is not in 'input' group. Global shortcuts may not work on Linux.");
                    warn!("Run: sudo usermod -a -G input $USER && logout/login");
                }
            }
            Err(_) => {
                warn!("Could not check user groups on Linux");
            }
        }
    }

    info!("Global shortcut manager initialized");
    Ok(())
}

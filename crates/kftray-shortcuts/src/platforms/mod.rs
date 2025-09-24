#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "linux")]
pub mod linux;

use std::sync::Arc;

use async_trait::async_trait;

use crate::actions::ActionRegistry;
use crate::models::ShortcutDefinition;

pub type ShortcutResult<T> = Result<T, crate::models::ShortcutError>;

#[async_trait]
pub trait PlatformManager: Send + Sync {
    async fn register_shortcut(&mut self, shortcut: &ShortcutDefinition) -> ShortcutResult<()>;
    async fn unregister_shortcut(&mut self, shortcut_id: i64) -> ShortcutResult<()>;
    async fn is_available(&self) -> bool;
    async fn platform_name(&self) -> &str;
}

pub async fn create_platform_manager(
    action_registry: Arc<tokio::sync::Mutex<ActionRegistry>>,
) -> ShortcutResult<Box<dyn PlatformManager>> {
    #[cfg(target_os = "windows")]
    {
        match windows::WindowsPlatform::new(action_registry.clone()) {
            Ok(manager) => {
                if manager.is_available().await {
                    return Ok(Box::new(manager));
                }
            }
            Err(e) => {
                log::error!("Failed to initialize Windows platform: {}", e);
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        match macos::MacOSPlatform::new(action_registry.clone()) {
            Ok(manager) => {
                if manager.is_available().await {
                    return Ok(Box::new(manager));
                }
            }
            Err(e) => {
                log::error!("Failed to initialize macOS platform: {}", e);
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        match linux::LinuxPlatform::new(action_registry.clone()).await {
            Ok(manager) => {
                if manager.is_available().await {
                    return Ok(Box::new(manager));
                }
            }
            Err(e) => {
                log::error!("Failed to initialize Linux platform: {}", e);
            }
        }
    }

    Err(crate::models::ShortcutError::PlatformError(
        "No compatible shortcut platform available for this system".to_string(),
    ))
}

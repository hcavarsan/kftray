use std::collections::HashMap;
use std::sync::Arc;

use log::info;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use tokio::sync::RwLock;

use crate::models::settings_model::AppSettings;
use crate::utils::db::get_db_pool;
use crate::utils::db_mode::{DatabaseManager, DatabaseMode};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Setting {
    pub key: String,
    pub value: String,
    pub updated_at: Option<String>,
}

pub struct SettingsManager {
    cache: Arc<RwLock<HashMap<String, String>>>,
}

impl SettingsManager {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn load_settings(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let pool = get_db_pool().await?;
        let settings = load_all_settings(&pool).await?;

        let mut cache = self.cache.write().await;
        cache.clear();
        for setting in settings {
            cache.insert(setting.key, setting.value);
        }
        info!("Loaded {} settings from database", cache.len());
        Ok(())
    }

    pub async fn get_setting(&self, key: &str) -> Option<String> {
        let cache = self.cache.read().await;
        cache.get(key).cloned()
    }

    pub async fn set_setting(
        &self, key: &str, value: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let pool = get_db_pool().await?;
        upsert_setting(&pool, key, value).await?;

        let mut cache = self.cache.write().await;
        cache.insert(key.to_string(), value.to_string());
        info!("Set setting: {key} = {value}");
        Ok(())
    }

    pub async fn get_disconnect_timeout(&self) -> Option<u32> {
        if let Some(value) = self.get_setting("disconnect_timeout_minutes").await {
            value.parse::<u32>().ok()
        } else {
            Some(0) // Default to 0 (no timeout)
        }
    }

    pub async fn set_disconnect_timeout(
        &self, minutes: u32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.set_setting("disconnect_timeout_minutes", &minutes.to_string())
            .await
    }

    pub async fn get_network_monitor(&self) -> bool {
        if let Some(value) = self.get_setting("network_monitor").await {
            value.parse::<bool>().unwrap_or(true)
        } else {
            true // Default to true
        }
    }

    pub async fn set_network_monitor(
        &self, enabled: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.set_setting("network_monitor", &enabled.to_string())
            .await
    }

    pub async fn get_http_logs_default_enabled(&self) -> bool {
        if let Some(value) = self.get_setting("http_logs_default_enabled").await {
            value.parse::<bool>().unwrap_or(false)
        } else {
            false // Default to false
        }
    }

    pub async fn set_http_logs_default_enabled(
        &self, enabled: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.set_setting("http_logs_default_enabled", &enabled.to_string())
            .await
    }

    pub async fn get_http_logs_max_file_size(&self) -> u64 {
        if let Some(value) = self.get_setting("http_logs_max_file_size").await {
            value.parse::<u64>().unwrap_or(10 * 1024 * 1024)
        } else {
            10 * 1024 * 1024 // Default 10MB
        }
    }

    pub async fn set_http_logs_max_file_size(
        &self, size: u64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.set_setting("http_logs_max_file_size", &size.to_string())
            .await
    }

    pub async fn get_http_logs_retention_days(&self) -> u64 {
        if let Some(value) = self.get_setting("http_logs_retention_days").await {
            value.parse::<u64>().unwrap_or(7)
        } else {
            7 // Default 7 days
        }
    }

    pub async fn set_http_logs_retention_days(
        &self, days: u64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.set_setting("http_logs_retention_days", &days.to_string())
            .await
    }

    pub async fn get_auto_update_enabled(&self) -> bool {
        if let Some(value) = self.get_setting("auto_update_enabled").await {
            value.parse::<bool>().unwrap_or(true)
        } else {
            true
        }
    }

    pub async fn set_auto_update_enabled(
        &self, enabled: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.set_setting("auto_update_enabled", &enabled.to_string())
            .await
    }

    pub async fn get_last_update_check(&self) -> Option<i64> {
        if let Some(value) = self.get_setting("last_update_check").await {
            value.parse::<i64>().ok()
        } else {
            None
        }
    }

    pub async fn set_last_update_check(
        &self, timestamp: i64,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.set_setting("last_update_check", &timestamp.to_string())
            .await
    }

    pub async fn get_all_settings(&self) -> HashMap<String, String> {
        let cache = self.cache.read().await;
        cache.clone()
    }

    pub async fn get_app_settings(&self) -> AppSettings {
        let settings = self.get_all_settings().await;
        AppSettings::from_settings_manager(&settings)
    }

    pub async fn set_app_settings(
        &self, app_settings: &AppSettings,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let settings_map = app_settings.to_settings_map();

        for (key, value) in settings_map.iter() {
            self.set_setting(key, value).await?;
        }

        Ok(())
    }

    pub async fn get_ssl_enabled(&self) -> bool {
        if let Some(value) = self.get_setting("ssl_enabled").await {
            value.parse::<bool>().unwrap_or(false)
        } else {
            false
        }
    }

    pub async fn set_ssl_enabled(
        &self, enabled: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.set_setting("ssl_enabled", &enabled.to_string()).await
    }

    pub async fn get_ssl_cert_validity_days(&self) -> u16 {
        if let Some(value) = self.get_setting("ssl_cert_validity_days").await {
            value.parse::<u16>().unwrap_or(365)
        } else {
            365
        }
    }

    pub async fn set_ssl_cert_validity_days(
        &self, days: u16,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.set_setting("ssl_cert_validity_days", &days.to_string())
            .await
    }

    pub async fn get_ssl_auto_regenerate(&self) -> bool {
        if let Some(value) = self.get_setting("ssl_auto_regenerate").await {
            value.parse::<bool>().unwrap_or(true)
        } else {
            true
        }
    }

    pub async fn set_ssl_auto_regenerate(
        &self, enabled: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.set_setting("ssl_auto_regenerate", &enabled.to_string())
            .await
    }

    pub async fn get_ssl_ca_auto_install(&self) -> bool {
        if let Some(value) = self.get_setting("ssl_ca_auto_install").await {
            value.parse::<bool>().unwrap_or(false)
        } else {
            false
        }
    }

    pub async fn set_ssl_ca_auto_install(
        &self, enabled: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.set_setting("ssl_ca_auto_install", &enabled.to_string())
            .await
    }

    pub async fn get_global_shortcut(&self) -> String {
        if let Some(value) = self.get_setting("global_shortcut").await {
            value
        } else {
            "Ctrl+Shift+F1".to_string()
        }
    }

    pub async fn set_global_shortcut(
        &self, shortcut: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.set_setting("global_shortcut", shortcut).await
    }
}

impl Default for SettingsManager {
    fn default() -> Self {
        Self::new()
    }
}

async fn load_all_settings(pool: &SqlitePool) -> Result<Vec<Setting>, sqlx::Error> {
    let mut conn = pool.acquire().await?;
    let rows = sqlx::query("SELECT key, value, updated_at FROM settings ORDER BY key")
        .fetch_all(&mut *conn)
        .await?;

    let settings = rows
        .into_iter()
        .map(|row| Setting {
            key: row.get("key"),
            value: row.get("value"),
            updated_at: row.get("updated_at"),
        })
        .collect();

    Ok(settings)
}

pub async fn upsert_setting(pool: &SqlitePool, key: &str, value: &str) -> Result<(), sqlx::Error> {
    let mut conn = pool.acquire().await?;
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at)
         VALUES (?, ?, CURRENT_TIMESTAMP)
         ON CONFLICT(key) DO UPDATE SET
         value = excluded.value,
         updated_at = CURRENT_TIMESTAMP",
    )
    .bind(key)
    .bind(value)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn get_setting(
    key: &str,
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let pool = get_db_pool().await?;
    get_setting_with_pool(&pool, key).await
}

pub async fn get_setting_with_pool(
    pool: &SqlitePool, key: &str,
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let mut conn = pool.acquire().await?;
    let result = sqlx::query("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(&mut *conn)
        .await?;

    Ok(result.map(|row| row.get("value")))
}

pub async fn set_setting(
    key: &str, value: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let pool = get_db_pool().await?;
    upsert_setting(&pool, key, value).await?;
    Ok(())
}

pub async fn get_disconnect_timeout()
-> Result<Option<u32>, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(value) = get_setting("disconnect_timeout_minutes").await? {
        Ok(value.parse::<u32>().ok())
    } else {
        Ok(Some(0)) // Default to 0 (no timeout)
    }
}

pub async fn set_disconnect_timeout(
    minutes: u32,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    set_setting("disconnect_timeout_minutes", &minutes.to_string()).await
}

pub async fn get_network_monitor() -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(value) = get_setting("network_monitor").await? {
        Ok(value.parse::<bool>().unwrap_or(true))
    } else {
        Ok(true) // Default to true
    }
}

pub async fn set_network_monitor(
    enabled: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    set_setting("network_monitor", &enabled.to_string()).await
}

pub async fn get_setting_with_mode(
    key: &str, mode: DatabaseMode,
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let context = DatabaseManager::get_context(mode).await?;
    let mut conn = context.pool.acquire().await?;
    let result = sqlx::query("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(result.map(|row| row.get("value")))
}

pub async fn set_setting_with_mode(
    key: &str, value: &str, mode: DatabaseMode,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let context = DatabaseManager::get_context(mode).await?;
    upsert_setting(&context.pool, key, value).await?;
    Ok(())
}

pub async fn get_disconnect_timeout_with_mode(
    mode: DatabaseMode,
) -> Result<Option<u32>, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(value) = get_setting_with_mode("disconnect_timeout_minutes", mode).await? {
        Ok(value.parse::<u32>().ok())
    } else {
        Ok(Some(0))
    }
}

pub async fn set_disconnect_timeout_with_mode(
    minutes: u32, mode: DatabaseMode,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    set_setting_with_mode("disconnect_timeout_minutes", &minutes.to_string(), mode).await
}

pub async fn get_network_monitor_with_mode(
    mode: DatabaseMode,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(value) = get_setting_with_mode("network_monitor", mode).await? {
        Ok(value.parse::<bool>().unwrap_or(true))
    } else {
        Ok(true)
    }
}

pub async fn set_network_monitor_with_mode(
    enabled: bool, mode: DatabaseMode,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    set_setting_with_mode("network_monitor", &enabled.to_string(), mode).await
}

pub async fn get_auto_update_enabled() -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(value) = get_setting("auto_update_enabled").await? {
        Ok(value.parse::<bool>().unwrap_or(true))
    } else {
        Ok(true)
    }
}

pub async fn set_auto_update_enabled(
    enabled: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    set_setting("auto_update_enabled", &enabled.to_string()).await
}

pub async fn get_last_update_check() -> Result<Option<i64>, Box<dyn std::error::Error + Send + Sync>>
{
    if let Some(value) = get_setting("last_update_check").await? {
        Ok(value.parse::<i64>().ok())
    } else {
        Ok(None)
    }
}

pub async fn set_last_update_check(
    timestamp: i64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    set_setting("last_update_check", &timestamp.to_string()).await
}

pub async fn get_auto_update_enabled_with_mode(
    mode: DatabaseMode,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(value) = get_setting_with_mode("auto_update_enabled", mode).await? {
        Ok(value.parse::<bool>().unwrap_or(true))
    } else {
        Ok(true)
    }
}

pub async fn set_auto_update_enabled_with_mode(
    enabled: bool, mode: DatabaseMode,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    set_setting_with_mode("auto_update_enabled", &enabled.to_string(), mode).await
}

pub async fn get_app_settings() -> Result<AppSettings, Box<dyn std::error::Error + Send + Sync>> {
    let pool = get_db_pool().await?;
    let settings = load_all_settings(&pool).await?;

    let settings_map: HashMap<String, String> =
        settings.into_iter().map(|s| (s.key, s.value)).collect();

    Ok(AppSettings::from_settings_manager(&settings_map))
}

pub async fn set_app_settings(
    app_settings: &AppSettings,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let pool = get_db_pool().await?;
    let settings_map = app_settings.to_settings_map();

    for (key, value) in settings_map.iter() {
        upsert_setting(&pool, key, value).await?;
    }

    Ok(())
}

pub async fn get_ssl_enabled() -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(value) = get_setting("ssl_enabled").await? {
        Ok(value.parse::<bool>().unwrap_or(false))
    } else {
        Ok(false)
    }
}

pub async fn set_ssl_enabled(
    enabled: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    set_setting("ssl_enabled", &enabled.to_string()).await
}

pub async fn get_ssl_cert_validity_days() -> Result<u16, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(value) = get_setting("ssl_cert_validity_days").await? {
        Ok(value.parse::<u16>().unwrap_or(365))
    } else {
        Ok(365)
    }
}

pub async fn set_ssl_cert_validity_days(
    days: u16,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    set_setting("ssl_cert_validity_days", &days.to_string()).await
}

pub async fn get_ssl_auto_regenerate() -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(value) = get_setting("ssl_auto_regenerate").await? {
        Ok(value.parse::<bool>().unwrap_or(true))
    } else {
        Ok(true)
    }
}

pub async fn set_ssl_auto_regenerate(
    enabled: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    set_setting("ssl_auto_regenerate", &enabled.to_string()).await
}

pub async fn get_ssl_ca_auto_install() -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(value) = get_setting("ssl_ca_auto_install").await? {
        Ok(value.parse::<bool>().unwrap_or(false))
    } else {
        Ok(false)
    }
}

pub async fn set_ssl_ca_auto_install(
    enabled: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    set_setting("ssl_ca_auto_install", &enabled.to_string()).await
}

pub async fn get_global_shortcut() -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(value) = get_setting("global_shortcut").await? {
        Ok(value)
    } else {
        Ok("Ctrl+Shift+F1".to_string())
    }
}

pub async fn set_global_shortcut(
    shortcut: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    set_setting("global_shortcut", shortcut).await
}

pub async fn get_ssl_enabled_with_mode(
    mode: DatabaseMode,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(value) = get_setting_with_mode("ssl_enabled", mode).await? {
        Ok(value.parse::<bool>().unwrap_or(false))
    } else {
        Ok(false)
    }
}

pub async fn set_ssl_enabled_with_mode(
    enabled: bool, mode: DatabaseMode,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    set_setting_with_mode("ssl_enabled", &enabled.to_string(), mode).await
}

pub async fn get_ssl_cert_validity_days_with_mode(
    mode: DatabaseMode,
) -> Result<u16, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(value) = get_setting_with_mode("ssl_cert_validity_days", mode).await? {
        Ok(value.parse::<u16>().unwrap_or(365))
    } else {
        Ok(365)
    }
}

pub async fn set_ssl_cert_validity_days_with_mode(
    days: u16, mode: DatabaseMode,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    set_setting_with_mode("ssl_cert_validity_days", &days.to_string(), mode).await
}

pub async fn get_ssl_auto_regenerate_with_mode(
    mode: DatabaseMode,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(value) = get_setting_with_mode("ssl_auto_regenerate", mode).await? {
        Ok(value.parse::<bool>().unwrap_or(true))
    } else {
        Ok(true)
    }
}

pub async fn set_ssl_auto_regenerate_with_mode(
    enabled: bool, mode: DatabaseMode,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    set_setting_with_mode("ssl_auto_regenerate", &enabled.to_string(), mode).await
}

pub async fn get_ssl_ca_auto_install_with_mode(
    mode: DatabaseMode,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(value) = get_setting_with_mode("ssl_ca_auto_install", mode).await? {
        Ok(value.parse::<bool>().unwrap_or(false))
    } else {
        Ok(false)
    }
}

pub async fn set_ssl_ca_auto_install_with_mode(
    enabled: bool, mode: DatabaseMode,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    set_setting_with_mode("ssl_ca_auto_install", &enabled.to_string(), mode).await
}

/// Returns whether automatic .env file synchronization is enabled.
/// Defaults to `true` if not previously set.
pub async fn get_env_auto_sync_enabled() -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(value) = get_setting("env_auto_sync_enabled").await? {
        Ok(value.parse::<bool>().unwrap_or(true))
    } else {
        Ok(true) // Default to enabled
    }
}

/// Sets whether automatic .env file synchronization is enabled.
pub async fn set_env_auto_sync_enabled(
    enabled: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    set_setting("env_auto_sync_enabled", &enabled.to_string()).await
}

/// Returns the file path for automatic .env synchronization.
/// Returns `None` if no path is configured or the path is empty.
pub async fn get_env_auto_sync_path()
-> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let value = get_setting("env_auto_sync_path").await?;
    Ok(value.filter(|s| !s.is_empty()))
}

/// Sets the file path for automatic .env synchronization.
pub async fn set_env_auto_sync_path(
    path: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    set_setting("env_auto_sync_path", path).await
}

pub async fn get_app_settings_with_mode(
    mode: DatabaseMode,
) -> Result<AppSettings, Box<dyn std::error::Error + Send + Sync>> {
    let context = DatabaseManager::get_context(mode).await?;
    let settings = load_all_settings(&context.pool).await?;

    let settings_map: HashMap<String, String> =
        settings.into_iter().map(|s| (s.key, s.value)).collect();

    Ok(AppSettings::from_settings_manager(&settings_map))
}

#[cfg(test)]
mod tests {
    use sqlx::SqlitePool;

    use super::*;
    use crate::utils::db::create_db_table;

    async fn create_test_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        create_db_table(&pool).await.unwrap();
        pool
    }

    #[tokio::test]
    async fn test_upsert_setting() {
        let pool = create_test_pool().await;

        upsert_setting(&pool, "test_key", "test_value")
            .await
            .unwrap();

        let result: String = sqlx::query("SELECT value FROM settings WHERE key = 'test_key'")
            .fetch_one(&pool)
            .await
            .unwrap()
            .get("value");
        assert_eq!(result, "test_value");

        upsert_setting(&pool, "test_key", "updated_value")
            .await
            .unwrap();

        let result: String = sqlx::query("SELECT value FROM settings WHERE key = 'test_key'")
            .fetch_one(&pool)
            .await
            .unwrap()
            .get("value");
        assert_eq!(result, "updated_value");
    }

    #[tokio::test]
    async fn test_load_all_settings() {
        let pool = create_test_pool().await;

        upsert_setting(&pool, "key1", "value1").await.unwrap();
        upsert_setting(&pool, "key2", "value2").await.unwrap();

        let settings = load_all_settings(&pool).await.unwrap();

        assert_eq!(settings.len(), 2);
        assert_eq!(settings[0].key, "key1");
        assert_eq!(settings[0].value, "value1");
        assert_eq!(settings[1].key, "key2");
        assert_eq!(settings[1].value, "value2");
    }

    #[tokio::test]
    async fn test_settings_manager() {
        let pool = create_test_pool().await;

        upsert_setting(&pool, "test_timeout", "30").await.unwrap();
        let settings = load_all_settings(&pool).await.unwrap();
        assert_eq!(settings.len(), 1);
        assert_eq!(settings[0].key, "test_timeout");
        assert_eq!(settings[0].value, "30");

        upsert_setting(&pool, "test_timeout", "60").await.unwrap();
        let settings = load_all_settings(&pool).await.unwrap();
        assert_eq!(settings.len(), 1);
        assert_eq!(settings[0].value, "60");

        let _manager = SettingsManager::new();
    }

    #[tokio::test]
    async fn test_settings_with_mode_memory() {
        let _lock = crate::test_utils::MEMORY_MODE_TEST_MUTEX.lock().await;
        set_setting_with_mode("memory_test", "test_value", DatabaseMode::Memory)
            .await
            .unwrap();

        let value = get_setting_with_mode("memory_test", DatabaseMode::Memory)
            .await
            .unwrap();
        assert_eq!(value, Some("test_value".to_string()));

        set_disconnect_timeout_with_mode(120, DatabaseMode::Memory)
            .await
            .unwrap();
        let timeout = get_disconnect_timeout_with_mode(DatabaseMode::Memory)
            .await
            .unwrap();
        assert_eq!(timeout, Some(120));

        set_network_monitor_with_mode(false, DatabaseMode::Memory)
            .await
            .unwrap();
        let monitor = get_network_monitor_with_mode(DatabaseMode::Memory)
            .await
            .unwrap();
        assert!(!monitor);
    }

    #[tokio::test]
    async fn test_settings_isolation_between_modes() {
        let _lock = crate::test_utils::MEMORY_MODE_TEST_MUTEX.lock().await;
        let memory_context = DatabaseManager::get_context(DatabaseMode::Memory)
            .await
            .unwrap();
        let memory_key = "isolation_test_memory";

        upsert_setting(&memory_context.pool, memory_key, "memory_value")
            .await
            .unwrap();

        let memory_result = get_setting_with_pool(&memory_context.pool, memory_key)
            .await
            .unwrap();
        assert_eq!(memory_result, Some("memory_value".to_string()));

        let different_key = "isolation_test_different";
        let different_result = get_setting_with_pool(&memory_context.pool, different_key)
            .await
            .unwrap();
        assert!(
            different_result.is_none(),
            "Different key should not have data"
        );

        let same_result = get_setting_with_pool(&memory_context.pool, memory_key)
            .await
            .unwrap();
        assert_eq!(same_result, Some("memory_value".to_string()));
    }
}

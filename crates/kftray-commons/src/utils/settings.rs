use std::collections::HashMap;
use std::sync::Arc;

use log::info;
use serde::{
    Deserialize,
    Serialize,
};
use sqlx::{
    Row,
    SqlitePool,
};
use tokio::sync::RwLock;

use crate::utils::db::get_db_pool;

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

    pub async fn get_all_settings(&self) -> HashMap<String, String> {
        let cache = self.cache.read().await;
        cache.clone()
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

async fn upsert_setting(pool: &SqlitePool, key: &str, value: &str) -> Result<(), sqlx::Error> {
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

pub async fn get_disconnect_timeout(
) -> Result<Option<u32>, Box<dyn std::error::Error + Send + Sync>> {
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
}

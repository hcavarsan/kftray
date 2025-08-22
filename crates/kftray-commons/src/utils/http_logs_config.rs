use log::error;
use sqlx::{
    Row,
    SqlitePool,
};

use crate::db::get_db_pool;
use crate::models::http_logs_config_model::HttpLogsConfig;
use crate::utils::db_mode::{
    DatabaseManager,
    DatabaseMode,
};

pub(crate) async fn get_http_logs_config_with_pool(
    config_id: i64, pool: &SqlitePool,
) -> Result<HttpLogsConfig, String> {
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    let row = sqlx::query(
        "SELECT config_id, enabled, max_file_size, retention_days, auto_cleanup 
         FROM http_logs_config WHERE config_id = ?1",
    )
    .bind(config_id)
    .fetch_optional(&mut *conn)
    .await
    .map_err(|e| {
        error!("Failed to fetch http_logs_config: {e}");
        e.to_string()
    })?;

    match row {
        Some(row) => {
            let enabled: bool = row.try_get("enabled").map_err(|e| {
                error!("Failed to get enabled: {e}");
                e.to_string()
            })?;
            let max_file_size: i64 = row.try_get("max_file_size").map_err(|e| {
                error!("Failed to get max_file_size: {e}");
                e.to_string()
            })?;
            let retention_days: i64 = row.try_get("retention_days").map_err(|e| {
                error!("Failed to get retention_days: {e}");
                e.to_string()
            })?;
            let auto_cleanup: bool = row.try_get("auto_cleanup").map_err(|e| {
                error!("Failed to get auto_cleanup: {e}");
                e.to_string()
            })?;

            Ok(HttpLogsConfig {
                config_id,
                enabled,
                max_file_size: max_file_size as u64,
                retention_days: retention_days as u64,
                auto_cleanup,
            })
        }
        None => Ok(HttpLogsConfig::new(config_id)),
    }
}

pub async fn get_http_logs_config(config_id: i64) -> Result<HttpLogsConfig, String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    get_http_logs_config_with_pool(config_id, &pool).await
}

pub(crate) async fn update_http_logs_config_with_pool(
    config: &HttpLogsConfig, pool: &SqlitePool,
) -> Result<(), String> {
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;

    sqlx::query(
        "INSERT INTO http_logs_config (config_id, enabled, max_file_size, retention_days, auto_cleanup, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, CURRENT_TIMESTAMP)
         ON CONFLICT(config_id) DO UPDATE SET
             enabled=excluded.enabled,
             max_file_size=excluded.max_file_size,
             retention_days=excluded.retention_days,
             auto_cleanup=excluded.auto_cleanup,
             updated_at=CURRENT_TIMESTAMP",
    )
    .bind(config.config_id)
    .bind(config.enabled)
    .bind(config.max_file_size as i64)
    .bind(config.retention_days as i64)
    .bind(config.auto_cleanup)
    .execute(&mut *conn)
    .await
    .map_err(|e| {
        error!("Failed to update http_logs_config: {e}");
        e.to_string()
    })?;

    Ok(())
}

pub async fn update_http_logs_config(config: &HttpLogsConfig) -> Result<(), String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    update_http_logs_config_with_pool(config, &pool).await
}

pub(crate) async fn delete_http_logs_config_with_pool(
    config_id: i64, pool: &SqlitePool,
) -> Result<(), String> {
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM http_logs_config WHERE config_id = ?1")
        .bind(config_id)
        .execute(&mut *conn)
        .await
        .map_err(|e| {
            error!("Failed to delete http_logs_config: {e}");
            e.to_string()
        })?;

    Ok(())
}

pub async fn delete_http_logs_config(config_id: i64) -> Result<(), String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    delete_http_logs_config_with_pool(config_id, &pool).await
}

pub(crate) async fn read_all_http_logs_configs_with_pool(
    pool: &SqlitePool,
) -> Result<Vec<HttpLogsConfig>, String> {
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    let rows = sqlx::query(
        "SELECT config_id, enabled, max_file_size, retention_days, auto_cleanup 
         FROM http_logs_config ORDER BY config_id",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(|e| {
        error!("Failed to fetch all http_logs_configs: {e}");
        e.to_string()
    })?;

    let configs = rows
        .into_iter()
        .map(|row| {
            let config_id: i64 = row.try_get("config_id").map_err(|e| {
                error!("Failed to get config_id: {e}");
                e.to_string()
            })?;
            let enabled: bool = row.try_get("enabled").map_err(|e| {
                error!("Failed to get enabled: {e}");
                e.to_string()
            })?;
            let max_file_size: i64 = row.try_get("max_file_size").map_err(|e| {
                error!("Failed to get max_file_size: {e}");
                e.to_string()
            })?;
            let retention_days: i64 = row.try_get("retention_days").map_err(|e| {
                error!("Failed to get retention_days: {e}");
                e.to_string()
            })?;
            let auto_cleanup: bool = row.try_get("auto_cleanup").map_err(|e| {
                error!("Failed to get auto_cleanup: {e}");
                e.to_string()
            })?;

            Ok(HttpLogsConfig {
                config_id,
                enabled,
                max_file_size: max_file_size as u64,
                retention_days: retention_days as u64,
                auto_cleanup,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    Ok(configs)
}

pub async fn read_all_http_logs_configs() -> Result<Vec<HttpLogsConfig>, String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    read_all_http_logs_configs_with_pool(&pool).await
}

pub async fn get_http_logs_config_with_mode(
    config_id: i64, mode: DatabaseMode,
) -> Result<HttpLogsConfig, String> {
    let context = DatabaseManager::get_context(mode).await?;
    get_http_logs_config_with_pool(config_id, &context.pool).await
}

pub async fn update_http_logs_config_with_mode(
    config: &HttpLogsConfig, mode: DatabaseMode,
) -> Result<(), String> {
    let context = DatabaseManager::get_context(mode).await?;
    update_http_logs_config_with_pool(config, &context.pool).await
}

pub async fn delete_http_logs_config_with_mode(
    config_id: i64, mode: DatabaseMode,
) -> Result<(), String> {
    let context = DatabaseManager::get_context(mode).await?;
    delete_http_logs_config_with_pool(config_id, &context.pool).await
}

pub async fn read_all_http_logs_configs_with_mode(
    mode: DatabaseMode,
) -> Result<Vec<HttpLogsConfig>, String> {
    let context = DatabaseManager::get_context(mode).await?;
    read_all_http_logs_configs_with_pool(&context.pool).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_db_table;
    use crate::utils::migration::migrate_configs;

    async fn setup_test_db() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("Failed to connect to in-memory database");
        create_db_table(&pool)
            .await
            .expect("Failed to create tables");
        migrate_configs(Some(&pool))
            .await
            .expect("Failed to run migrations");
        pool
    }

    #[tokio::test]
    async fn test_http_logs_config_operations() {
        let pool = setup_test_db().await;

        // Create a test config first
        use crate::config::insert_config_with_pool;
        use crate::models::config_model::Config;

        let test_config = Config {
            service: Some("test-service".to_string()),
            ..Config::default()
        };

        insert_config_with_pool(test_config, &pool).await.unwrap();

        // Get the inserted config's ID
        let configs = crate::config::read_configs_with_pool(&pool).await.unwrap();
        let config_id = configs[0].id.unwrap();

        let initial_config = get_http_logs_config_with_pool(config_id, &pool)
            .await
            .unwrap();
        assert_eq!(initial_config.config_id, config_id);
        assert!(!initial_config.enabled);

        let updated_config = HttpLogsConfig {
            config_id,
            enabled: true,
            max_file_size: 5 * 1024 * 1024,
            retention_days: 14,
            auto_cleanup: false,
        };

        update_http_logs_config_with_pool(&updated_config, &pool)
            .await
            .unwrap();

        let retrieved_config = get_http_logs_config_with_pool(config_id, &pool)
            .await
            .unwrap();
        assert_eq!(retrieved_config.config_id, config_id);
        assert!(retrieved_config.enabled);
        assert_eq!(retrieved_config.max_file_size, 5 * 1024 * 1024);
        assert_eq!(retrieved_config.retention_days, 14);
        assert!(!retrieved_config.auto_cleanup);

        delete_http_logs_config_with_pool(config_id, &pool)
            .await
            .unwrap();

        let deleted_config = get_http_logs_config_with_pool(config_id, &pool)
            .await
            .unwrap();
        assert_eq!(deleted_config.config_id, config_id);
        assert!(!deleted_config.enabled);
    }

    #[tokio::test]
    async fn test_read_all_http_logs_configs() {
        let pool = setup_test_db().await;

        // Create test configs first
        use crate::config::insert_config_with_pool;
        use crate::models::config_model::Config;

        let test_config1 = Config {
            service: Some("test-service-1".to_string()),
            ..Config::default()
        };
        let test_config2 = Config {
            service: Some("test-service-2".to_string()),
            ..Config::default()
        };

        insert_config_with_pool(test_config1, &pool).await.unwrap();
        insert_config_with_pool(test_config2, &pool).await.unwrap();

        // Get the inserted config IDs
        let configs = crate::config::read_configs_with_pool(&pool).await.unwrap();
        let config_id1 = configs[0].id.unwrap();
        let config_id2 = configs[1].id.unwrap();

        let config1 = HttpLogsConfig {
            config_id: config_id1,
            enabled: true,
            max_file_size: 10 * 1024 * 1024,
            retention_days: 7,
            auto_cleanup: true,
        };
        let config2 = HttpLogsConfig {
            config_id: config_id2,
            enabled: false,
            max_file_size: 20 * 1024 * 1024,
            retention_days: 14,
            auto_cleanup: false,
        };

        update_http_logs_config_with_pool(&config1, &pool)
            .await
            .unwrap();
        update_http_logs_config_with_pool(&config2, &pool)
            .await
            .unwrap();

        let all_configs = read_all_http_logs_configs_with_pool(&pool).await.unwrap();
        assert_eq!(all_configs.len(), 2);

        let found_config1 = all_configs
            .iter()
            .find(|c| c.config_id == config_id1)
            .unwrap();
        assert!(found_config1.enabled);
        assert_eq!(found_config1.max_file_size, 10 * 1024 * 1024);

        let found_config2 = all_configs
            .iter()
            .find(|c| c.config_id == config_id2)
            .unwrap();
        assert!(!found_config2.enabled);
        assert_eq!(found_config2.max_file_size, 20 * 1024 * 1024);
    }
}

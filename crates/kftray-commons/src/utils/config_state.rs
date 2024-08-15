use log::error;
use sqlx::Row;

use crate::db::get_db_pool;
use crate::models::config_state_model::ConfigState;

pub async fn update_config_state(config_state: &ConfigState) -> Result<(), String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;

    sqlx::query("UPDATE config_state SET is_running = ?1 WHERE config_id = ?2")
        .bind(config_state.is_running)
        .bind(config_state.config_id)
        .execute(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

pub async fn read_config_states() -> Result<Vec<ConfigState>, sqlx::Error> {
    let pool = get_db_pool().await.map_err(|e| {
        error!("Failed to get database pool: {}", e);
        sqlx::Error::Configuration(e.into())
    })?;

    let mut conn = pool.acquire().await.map_err(|e| {
        error!("Failed to acquire database connection: {}", e);
        e
    })?;

    let rows = sqlx::query("SELECT id, config_id, is_running FROM config_state")
        .fetch_all(&mut *conn)
        .await
        .map_err(|e| {
            error!("Failed to fetch config states: {}", e);
            e
        })?;

    let config_states = rows
        .into_iter()
        .map(|row| {
            let id: Option<i64> = row.try_get("id").ok();
            let config_id: i64 = row.try_get("config_id").map_err(|e| {
                error!("Failed to get config_id: {}", e);
                e
            })?;
            let is_running: bool = row.try_get("is_running").map_err(|e| {
                error!("Failed to get is_running: {}", e);
                e
            })?;
            Ok(ConfigState {
                id,
                config_id,
                is_running,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;

    Ok(config_states)
}

pub async fn get_configs_state() -> Result<Vec<ConfigState>, String> {
    read_config_states().await.map_err(|e| {
        error!("Failed to get config states: {}", e);
        e.to_string()
    })
}

use sqlx::Row;

use crate::db::get_db_pool;
use crate::models::config_state::ConfigState;

// Function to insert a config state into the database
#[tauri::command]
pub async fn insert_config_state(config_state: ConfigState) -> Result<(), String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS config_state (
            id INTEGER PRIMARY KEY,
            config_id INTEGER NOT NULL,
            is_running BOOLEAN NOT NULL DEFAULT false,
            FOREIGN KEY(config_id) REFERENCES configs(id) ON DELETE CASCADE
        )",
    )
    .execute(&mut *conn)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query("INSERT INTO config_state (config_id, is_running) VALUES (?1, ?2)")
        .bind(config_state.config_id)
        .bind(config_state.is_running)
        .execute(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

// Function to read config states from the database
async fn read_config_states() -> Result<Vec<ConfigState>, sqlx::Error> {
    let pool = get_db_pool()
        .await
        .map_err(|e| sqlx::Error::Configuration(e.into()))?;
    let mut conn = pool.acquire().await?;

    let rows = sqlx::query("SELECT id, config_id, is_running FROM config_state")
        .fetch_all(&mut *conn)
        .await?;

    let config_states = rows
        .into_iter()
        .map(|row| ConfigState {
            id: row.try_get("id").ok(),
            config_id: row.try_get("config_id").unwrap(),
            is_running: row.try_get("is_running").unwrap(),
        })
        .collect();

    Ok(config_states)
}

// Function to get all config states from the database
#[tauri::command]
pub async fn get_config_states() -> Result<Vec<ConfigState>, String> {
    let config_states = read_config_states().await.map_err(|e| e.to_string())?;
    Ok(config_states)
}

// Function to get a config state by config_id from the database
#[tauri::command]
pub async fn get_config_state_by_config_id(config_id: i64) -> Result<ConfigState, String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;

    let row =
        sqlx::query("SELECT id, config_id, is_running FROM config_state WHERE config_id = ?1")
            .bind(config_id)
            .fetch_optional(&mut *conn)
            .await
            .map_err(|e| e.to_string())?;

    match row {
        Some(row) => Ok(ConfigState {
            id: row.try_get("id").ok(),
            config_id: row.try_get("config_id").unwrap(),
            is_running: row.try_get("is_running").unwrap(),
        }),
        None => Err(format!(
            "No config state found with config_id: {}",
            config_id
        )),
    }
}

// Function to update a config state in the database
#[tauri::command]
pub async fn update_config_state(config_state: ConfigState) -> Result<(), String> {
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

// Function to delete a config state from the database
#[tauri::command]
pub async fn delete_config_state(config_id: i64) -> Result<(), String> {
    let pool = get_db_pool().await.map_err(|e| e.to_string())?;
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM config_state WHERE config_id = ?1")
        .bind(config_id)
        .execute(&mut *conn)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

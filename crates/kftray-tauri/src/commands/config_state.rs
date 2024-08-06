use kftray_commons::models::config_state_model::ConfigState;
use kftray_commons::utils::db::get_db_pool;
use sqlx::Row;

// Function to read config states from the database
pub async fn read_config_states() -> Result<Vec<ConfigState>, sqlx::Error> {
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

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

use kftray_commons::{
    db::Database,
    error::Result as CommonsResult,
    models::state::ConfigState,
    utils::{
        get_db_path,
        state::StateManager,
    },
};
use log::info;

async fn get_state_manager() -> CommonsResult<(Database, StateManager)> {
    let db_path = get_db_path().await?;
    let database = Database::new(db_path).await?;
    let state_manager = StateManager::new(database.clone()).await?;
    Ok((database, state_manager))
}

#[tauri::command]
pub async fn get_config_states() -> Result<Vec<ConfigState>, String> {
    info!("Getting all config states");
    let (_, state_manager) = get_state_manager()
        .await
        .map_err(|e| format!("Failed to initialize state manager: {}", e))?;

    state_manager
        .get_all_states()
        .await
        .map_err(|e| e.to_string())
}

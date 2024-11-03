use kftray_commons::{
    config::Config,
    db::Database,
    error::Result as CommonsResult,
    utils::get_db_path,
};
use log::{
    error,
    info,
};
use serde_json;

async fn get_database() -> CommonsResult<Database> {
    let db_path = get_db_path().await?;
    Database::new(db_path).await
}

#[tauri::command]
pub async fn delete_config_cmd(id: i64) -> Result<(), String> {
    info!("Deleting config with id: {}", id);
    let db = get_database()
        .await
        .map_err(|e| format!("Failed to initialize database: {}", e))?;

    db.delete_config(id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_configs_cmd(ids: Vec<i64>) -> Result<(), String> {
    info!("Deleting configs with ids: {:?}", ids);
    let db = get_database()
        .await
        .map_err(|e| format!("Failed to initialize database: {}", e))?;

    db.delete_configs(&ids).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_all_configs_cmd() -> Result<(), String> {
    info!("Deleting all configs");
    let db = get_database()
        .await
        .map_err(|e| format!("Failed to initialize database: {}", e))?;

    db.clear_all_configs().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn insert_config_cmd(mut config: Config) -> Result<(), String> {
    let db = get_database()
        .await
        .map_err(|e| format!("Failed to initialize database: {}", e))?;

    config = config.prepare_for_save();
    db.save_config(&config).await.map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn get_configs_cmd() -> Result<Vec<Config>, String> {
    info!("Getting all configs");
    let db = get_database()
        .await
        .map_err(|e| format!("Failed to initialize database: {}", e))?;

    db.get_all_configs().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_config_cmd(id: i64) -> Result<Config, String> {
    info!("Getting config with id: {}", id);
    let db = get_database()
        .await
        .map_err(|e| format!("Failed to initialize database: {}", e))?;

    db.get_config(id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_config_cmd(mut config: Config) -> Result<(), String> {
    let db = get_database()
        .await
        .map_err(|e| format!("Failed to initialize database: {}", e))?;

    config = config.prepare_for_save();
    db.update_config(&config).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn export_configs_cmd() -> Result<String, String> {
    let db = get_database()
        .await
        .map_err(|e| format!("Failed to initialize database: {}", e))?;

    let configs = db.get_all_configs().await.map_err(|e| e.to_string())?;

    serde_json::to_string(&configs).map_err(|e| format!("Failed to serialize configs: {}", e))
}

#[tauri::command]
pub async fn import_configs_cmd(json: String) -> Result<(), String> {
    let db = get_database()
        .await
        .map_err(|e| format!("Failed to initialize database: {}", e))?;

    let configs: Vec<Config> =
        serde_json::from_str(&json).map_err(|e| format!("Failed to parse configs: {}", e))?;

    for mut config in configs {
        config = config.prepare_for_save();
        if let Err(e) = db.save_config(&config).await {
            error!("Failed to import config: {}", e);
            return Err(format!("Failed to import config: {}", e));
        }
    }

    Ok(())
}

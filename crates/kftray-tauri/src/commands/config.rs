use kftray_commons::config::{
    delete_all_configs,
    delete_config,
    delete_configs,
    export_configs,
    get_config,
    get_configs,
    import_configs,
    insert_config,
    update_config,
};
use kftray_commons::models::config_model::Config;
use log::{
    error,
    info,
};

#[tauri::command]
pub async fn delete_config_cmd(id: i64) -> Result<(), String> {
    info!("Deleting config with id: {}", id);
    delete_config(id).await
}

#[tauri::command]
pub async fn delete_configs_cmd(ids: Vec<i64>) -> Result<(), String> {
    info!("Deleting configs with ids: {:?}", ids);
    delete_configs(ids).await
}

#[tauri::command]
pub async fn delete_all_configs_cmd() -> Result<(), String> {
    info!("Deleting all configs");
    delete_all_configs().await
}

#[tauri::command]
pub async fn insert_config_cmd(config: Config) -> Result<(), String> {
    insert_config(config).await
}

#[tauri::command]
pub async fn get_configs_cmd() -> Result<Vec<Config>, String> {
    info!("get_configs called");
    let configs = get_configs().await?;
    Ok(configs)
}

#[tauri::command]
pub async fn get_config_cmd(id: i64) -> Result<Config, String> {
    info!("get_config called with id: {}", id);
    get_config(id).await
}

#[tauri::command]
pub async fn update_config_cmd(config: Config) -> Result<(), String> {
    update_config(config).await
}

#[tauri::command]
pub async fn export_configs_cmd() -> Result<String, String> {
    export_configs().await
}

#[tauri::command]
pub async fn import_configs_cmd(json: String) -> Result<(), String> {
    if let Err(e) = import_configs(json).await {
        error!("Error migrating configs: {}. Please check if the configurations are valid and compatible with the current system/version.", e);
        return Err(format!("Error migrating configs: {}", e));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_configs_cmd_format() {
        let _ = get_configs_cmd().await;
    }

    #[tokio::test]
    async fn test_get_config_cmd_format() {
        let id = 123;
        let _ = get_config_cmd(id).await;
    }

    #[tokio::test]
    async fn test_delete_config_cmd_format() {
        let id = 123;
        let _ = delete_config_cmd(id).await;
    }
}

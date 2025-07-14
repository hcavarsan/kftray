use kftray_commons::utils::config::{
    export_configs_with_mode,
    import_configs_with_mode,
};
use kftray_commons::utils::db_mode::DatabaseMode;

pub async fn import_configs_from_file(file_path: &str, mode: DatabaseMode) -> Result<(), String> {
    log::debug!("Starting import of configs from file: {file_path}");
    let json = std::fs::read_to_string(file_path).map_err(|e| {
        let err_msg = format!("Failed to read file {file_path}: {e}");
        log::error!("{err_msg}");
        err_msg
    })?;
    log::debug!("File content read successfully. Size: {} bytes", json.len());

    import_configs_with_mode(json, mode).await.map_err(|e| {
        let err_msg = format!("Failed to import configs: {e}");
        log::error!("{err_msg}");
        err_msg
    })?;
    log::debug!("Successfully imported configs from file: {file_path}");
    Ok(())
}

pub async fn export_configs_to_file(file_path: &str, mode: DatabaseMode) -> Result<(), String> {
    log::debug!("Starting export of configs to file: {file_path}");
    let json = export_configs_with_mode(mode).await.map_err(|e| {
        let err_msg = format!("Failed to export configs: {e}");
        log::error!("{err_msg}");
        err_msg
    })?;
    log::debug!("Configs exported successfully: {json}");

    std::fs::write(file_path, json).map_err(|e| {
        let err_msg = format!("Failed to write to file {file_path}: {e}");
        log::error!("{err_msg}");
        err_msg
    })?;
    log::debug!("Successfully exported configs to file: {file_path}");
    Ok(())
}

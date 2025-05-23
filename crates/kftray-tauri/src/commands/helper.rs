use kftray_helper::HelperClient;
use log::info;

#[tauri::command]
pub async fn install_helper() -> Result<bool, String> {
    info!("Installing helper sidecar");
    let app_id = "com.kftray.app".to_string();
    let client = HelperClient::new(app_id).map_err(|e| e.to_string())?;

    client
        .ensure_helper_installed()
        .map_err(|e| e.to_string())?;
    Ok(true)
}

#[tauri::command]
pub async fn remove_helper() -> Result<bool, String> {
    info!("Removing helper sidecar");
    let app_id = "com.kftray.app".to_string();
    let client = HelperClient::new(app_id).map_err(|e| e.to_string())?;

    client
        .ensure_helper_uninstalled()
        .map_err(|e| e.to_string())?;
    Ok(true)
}

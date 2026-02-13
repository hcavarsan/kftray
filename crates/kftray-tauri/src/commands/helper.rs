use kftray_helper::HelperClient;
use log::{info, warn};

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

#[tauri::command]
pub async fn allocate_local_address_cmd(service_name: String) -> Result<String, String> {
    info!("Allocating local address for service: {service_name}");
    let app_id = "com.kftray.app".to_string();
    let client = HelperClient::new(app_id).map_err(|e| e.to_string())?;

    match client.allocate_local_address(service_name) {
        Ok(address) => {
            info!("Successfully allocated address: {address}");
            Ok(address)
        }
        Err(e) => {
            warn!("Failed to allocate address: {e}");
            Err(e.to_string())
        }
    }
}

#[tauri::command]
pub async fn release_local_address_cmd(address: String) -> Result<(), String> {
    info!("Releasing local address: {address}");
    let app_id = "com.kftray.app".to_string();
    let client = HelperClient::new(app_id).map_err(|e| e.to_string())?;

    client.release_local_address(address).map_err(|e| {
        warn!("Failed to release address: {e}");
        e.to_string()
    })
}

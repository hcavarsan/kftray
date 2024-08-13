use kftray_commons::config_state::get_configs_state;
use kftray_commons::models::config_state_model::ConfigState;

#[tauri::command]
pub async fn get_config_states() -> Result<Vec<ConfigState>, String> {
    log::info!("get_configs state called");
    let configs = get_configs_state().await?;
    log::info!("{:?}", configs);
    Ok(configs)
}

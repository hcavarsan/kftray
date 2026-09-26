use kftray_commons::config::get_configs;
use kftray_commons::utils::config_view::{
    ConfigGroup,
    ConfigView,
    Facet,
    facets,
    get_config_view_with_mode,
    set_config_view_with_mode,
};
use kftray_commons::utils::db_mode::DatabaseMode;
use serde::Serialize;

#[derive(Serialize)]
pub struct ConfigViewResult {
    pub view: ConfigView,
    pub groups: Vec<ConfigGroup>,
    pub facets: Vec<Facet>,
}

#[tauri::command]
pub async fn query_config_view_cmd(view: Option<ConfigView>) -> Result<ConfigViewResult, String> {
    let view = match view {
        Some(view) => view,
        None => get_config_view_with_mode(DatabaseMode::File).await,
    };
    let configs = get_configs().await?;
    Ok(ConfigViewResult {
        groups: view.group(&configs),
        facets: facets(&configs),
        view,
    })
}

#[tauri::command]
pub async fn set_config_view_cmd(view: ConfigView) -> Result<(), String> {
    set_config_view_with_mode(&view, DatabaseMode::File).await
}

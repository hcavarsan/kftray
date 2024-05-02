use std::sync::atomic::Ordering;

use base64::{engine::general_purpose, Engine as _};
use reqwest::header::{AUTHORIZATION, USER_AGENT};
use tauri::State;

use crate::{
    config::{import_configs, migrate_configs},
    models::{config::Config, dialog::SaveDialogState},
    remote_config::{build_github_api_url, clear_existing_configs},
};

//  command to save the dialog state when is open
#[tauri::command]
pub fn open_save_dialog(state: State<SaveDialogState>) {
    state.is_open.store(true, Ordering::SeqCst);
}

// command to save the dialog state when is closed
#[tauri::command]
pub fn close_save_dialog(state: State<SaveDialogState>) {
    state.is_open.store(false, Ordering::SeqCst);
}

// command to import configs from github
#[tauri::command]
pub async fn import_configs_from_github(
    repo_url: String,
    config_path: String,
    is_private: bool,
    flush: bool,
    token: Option<String>,
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let url = build_github_api_url(&repo_url, &config_path);
    let mut request_builder = client.get(url);

    if is_private {
        let token = token.ok_or("Token is required for private repositories")?;
        request_builder = request_builder.header(AUTHORIZATION, format!("token {}", token));
    }

    let response = request_builder
        .header(USER_AGENT, "request")
        .send()
        .await
        .map_err(|e| format!("Failed to send request: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Request failed: {}", e))?;

    let json_content = response.text().await.map_err(|e| e.to_string())?;

    let json_obj: serde_json::Value = serde_json::from_str(&json_content)
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let base64_content = json_obj["content"]
        .as_str()
        .ok_or("Failed to extract content from response")?
        .trim();
    println!("base64_content: {}", base64_content);

    let base64_content_cleaned = base64_content.replace(['\n', '\r'], "");

    let decoded_content = general_purpose::STANDARD
        .decode(&base64_content_cleaned)
        .map_err(|e| format!("Failed to decode base64 content: {}", e))?;

    let decoded_str = String::from_utf8(decoded_content)
        .map_err(|e| format!("Failed to convert decoded content to string: {}", e))?;

    println!("decoded_str: {}", decoded_str);
    let configs: Vec<Config> = serde_json::from_str(&decoded_str)
        .map_err(|e| format!("Failed to parse configs: {}", e))?;

    if flush {
        clear_existing_configs().map_err(|e| e.to_string())?;
    }
    for config in configs {
        let config_json = serde_json::to_string(&config)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        import_configs(config_json).await?;
    }

    if let Err(e) = migrate_configs() {
        eprintln!("Error migrating configs: {}. Please check if the configurations are valid and compatible with the current system/version.", e);
    }

    Ok(())
}

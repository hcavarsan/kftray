use base64::{
    engine::general_purpose,
    Engine as _,
};
use keyring::{
    Entry,
    Error as KeyringError,
};
use kftray_commons::{
    models::config_model::Config,
    utils::config::import_configs,
    utils::github::{
        build_github_api_url,
        clear_existing_configs,
    },
    utils::migration::migrate_configs,
};
use log::{
    error,
    info,
};
use reqwest::header::{
    AUTHORIZATION,
    USER_AGENT,
};
use serde_json::Value;
use tauri::{
    Error as TauriError,
    InvokeError,
};

#[derive(Debug)]
pub enum CustomError {
    Keyring(KeyringError),
    Tauri(TauriError),
}

impl From<KeyringError> for CustomError {
    fn from(error: KeyringError) -> Self {
        CustomError::Keyring(error)
    }
}

impl From<TauriError> for CustomError {
    fn from(error: TauriError) -> Self {
        CustomError::Tauri(error)
    }
}

impl From<CustomError> for InvokeError {
    fn from(error: CustomError) -> Self {
        match error {
            CustomError::Keyring(err) => InvokeError::from(err.to_string()),
            CustomError::Tauri(err) => InvokeError::from(err.to_string()),
        }
    }
}

#[tauri::command]
pub fn store_key(
    service: &str, name: &str, password: &str,
) -> std::result::Result<(), CustomError> {
    let entry = Entry::new(service, name).map_err(CustomError::from)?;

    entry.set_password(password).map_err(CustomError::from)?;

    Ok(())
}

#[tauri::command]
pub fn get_key(service: &str, name: &str) -> std::result::Result<String, CustomError> {
    let entry = Entry::new(service, name).map_err(CustomError::from)?;

    let password = entry.get_password().map_err(CustomError::from)?;

    Ok(password)
}

#[tauri::command]
pub fn delete_key(service: &str, name: &str) -> std::result::Result<(), CustomError> {
    let entry = Entry::new(service, name).map_err(CustomError::from)?;

    entry.delete_credential().map_err(CustomError::from)?;

    Ok(())
}

#[tauri::command]
pub async fn import_configs_from_github(
    repo_url: String, config_path: String, is_private: bool, flush: bool, token: Option<String>,
) -> Result<(), String> {
    let client = reqwest::Client::new();

    let url = build_github_api_url(&repo_url, &config_path)
        .map_err(|e| format!("Failed to build GitHub API URL: {}", e))?;

    let mut request_builder = client.get(&url);

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

    let json_obj: Value = serde_json::from_str(&json_content)
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let base64_content = json_obj["content"]
        .as_str()
        .ok_or("Failed to extract content from response")?
        .trim();

    info!("base64_content: {}", base64_content);

    let base64_content_cleaned = base64_content.replace(['\n', '\r'], "");

    let decoded_content = general_purpose::STANDARD
        .decode(&base64_content_cleaned)
        .map_err(|e| format!("Failed to decode base64 content: {}", e))?;

    let decoded_str = String::from_utf8(decoded_content)
        .map_err(|e| format!("Failed to convert decoded content to string: {}", e))?;

    info!("decoded_str: {}", decoded_str);

    let configs: Vec<Config> = serde_json::from_str(&decoded_str)
        .map_err(|e| format!("Failed to parse configs: {}", e))?;

    if flush {
        clear_existing_configs().await.map_err(|e| e.to_string())?;
    }

    for config in configs {
        let config_json = serde_json::to_string(&config)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;

        import_configs(config_json).await?;
    }

    if let Err(e) = migrate_configs().await {
        error!("Error migrating configs: {}. Please check if the configurations are valid and compatible with the current system/version.", e);
    }

    Ok(())
}

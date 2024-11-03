use keyring::{
    Entry,
    Error as KeyringError,
};
use kftray_commons::utils::get_db_path;
use kftray_commons::{
    core::GithubClient,
    db::Database,
    error::Error as CommonsError,
};
use tauri::{
    Error as TauriError,
    InvokeError,
};

#[derive(Debug)]
pub enum CustomError {
    Keyring(KeyringError),
    Tauri(TauriError),
    Commons(CommonsError),
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

impl From<CommonsError> for CustomError {
    fn from(error: CommonsError) -> Self {
        CustomError::Commons(error)
    }
}

impl From<CustomError> for InvokeError {
    fn from(error: CustomError) -> Self {
        match error {
            CustomError::Keyring(err) => InvokeError::from(err.to_string()),
            CustomError::Tauri(err) => InvokeError::from(err.to_string()),
            CustomError::Commons(err) => InvokeError::from(err.to_string()),
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
    let db = Database::new(get_db_path().await.unwrap()).await.unwrap();

    let github_client = GithubClient::new(db);

    if flush {
        github_client
            .db
            .clear_all_configs()
            .await
            .map_err(|e| format!("Failed to clear existing configs: {}", e))?;
    }

    github_client
        .sync_configs(&repo_url, &config_path, is_private, token)
        .await
        .map_err(|e| format!("Failed to sync configs: {}", e))?;

    Ok(())
}

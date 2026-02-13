use keyring::{Entry, Error as KeyringError};
use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_commons::utils::github::{GitHubConfig, GitHubRepository};
use tauri::{Error as TauriError, ipc::InvokeError};

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
            CustomError::Keyring(err) => InvokeError::from(format!("Keyring error: {err:?}")),
            CustomError::Tauri(err) => InvokeError::from(format!("Tauri error: {err:?}")),
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

// Removed credentials module - now handled in commons

// Removed functions - now handled in commons

#[tauri::command]
pub async fn import_configs_from_github(
    repo_url: String, config_path: String, use_system_credentials: bool, flush: bool,
    github_token: Option<String>,
) -> Result<(), String> {
    let config = GitHubConfig {
        repo_url,
        config_path,
        use_system_credentials,
        github_token,
        flush_existing: flush,
    };

    GitHubRepository::import_configs(config, DatabaseMode::File).await
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockEntry {
        service: String,
        name: String,
    }

    impl MockEntry {
        fn new(service: &str, name: &str) -> Result<Self, KeyringError> {
            Ok(Self {
                service: service.to_string(),
                name: name.to_string(),
            })
        }

        fn set_password(&self, _password: &str) -> Result<(), KeyringError> {
            Ok(())
        }

        fn get_password(&self) -> Result<String, KeyringError> {
            Ok("test_password".to_string())
        }

        fn delete_credential(&self) -> Result<(), KeyringError> {
            Ok(())
        }
    }

    #[test]
    fn test_keyring_operations() {
        let entry = MockEntry::new("test_service", "test_name").unwrap();
        assert_eq!(entry.service, "test_service");
        assert_eq!(entry.name, "test_name");

        assert!(entry.set_password("test_password").is_ok());
        assert_eq!(entry.get_password().unwrap(), "test_password");
        assert!(entry.delete_credential().is_ok());
    }

    #[test]
    fn test_store_get_delete_key() {
        let result = store_key("test_service", "test_name", "test_password");
        assert!(result.is_ok());

        let password = get_key("test_service", "test_name");
        assert!(password.is_ok());

        let result = delete_key("test_service", "test_name");
        assert!(result.is_ok());
    }

    #[test]
    fn test_custom_error_conversion() {
        let keyring_error = KeyringError::NoEntry;
        let custom_error = CustomError::from(keyring_error);
        match custom_error {
            CustomError::Keyring(_) => (),
            _ => panic!("Wrong error variant for KeyringError conversion"),
        }

        let tauri_error = TauriError::InvalidWebviewUrl("test");
        let custom_error = CustomError::from(tauri_error);
        match custom_error {
            CustomError::Tauri(_) => (),
            _ => panic!("Wrong error variant for TauriError conversion"),
        }

        let custom_error = CustomError::Keyring(KeyringError::NoEntry);
        let invoke_error = InvokeError::from(custom_error);
        let error_str = format!("{invoke_error:?}");
        assert!(
            error_str.contains("Keyring error") && error_str.contains("NoEntry"),
            "InvokeError should contain both the error type and variant: {error_str}"
        );
    }

    #[tokio::test]
    async fn test_import_configs_from_github() {
        let result = import_configs_from_github(
            "https://github.com/nonexistent/repo".to_string(),
            "config.json".to_string(),
            false,
            false,
            None,
        )
        .await;

        assert!(result.is_err(), "Should fail with invalid repository");
        let err = result.unwrap_err();
        assert!(
            err.contains("Failed to clone repository") || err.contains("failed to resolve address"),
            "Error should indicate clone or network failure: {err}"
        );
    }
}

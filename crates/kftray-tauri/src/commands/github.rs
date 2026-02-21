use keyring::{
    Entry,
    Error as KeyringError,
};
use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_commons::utils::github::{
    GitHubConfig,
    GitHubRepository,
};
use tauri::{
    Error as TauriError,
    ipc::InvokeError,
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
    use std::any::Any;
    use std::collections::BTreeMap;
    use std::sync::{
        Arc,
        Mutex,
    };

    use keyring::credential::{
        CredentialApi,
        CredentialBuilderApi,
        CredentialPersistence,
    };

    use super::*;

    // -- Shared in-memory credential store (modeled after GitButler) ----------
    //
    // The built-in `keyring::mock` stores passwords per-Entry *instance*, so
    // separate `Entry::new()` calls (as in store_key / get_key / delete_key)
    // never share state.  This store keeps a single BTreeMap behind an
    // Arc<Mutex<>> so every Entry with the same service+user reads/writes the
    // same slot.

    type SharedStore = Arc<Mutex<BTreeMap<String, String>>>;

    struct MockEntry {
        handle: String,
        store: SharedStore,
    }

    impl CredentialApi for MockEntry {
        fn set_password(&self, password: &str) -> keyring::Result<()> {
            self.store
                .lock()
                .unwrap()
                .insert(self.handle.clone(), password.into());
            Ok(())
        }

        fn set_secret(&self, _secret: &[u8]) -> keyring::Result<()> {
            unreachable!("unused in tests")
        }

        fn get_password(&self) -> keyring::Result<String> {
            match self.store.lock().unwrap().get(&self.handle) {
                Some(secret) => Ok(secret.clone()),
                None => Err(keyring::Error::NoEntry),
            }
        }

        fn get_secret(&self) -> keyring::Result<Vec<u8>> {
            unreachable!("unused in tests")
        }

        fn delete_credential(&self) -> keyring::Result<()> {
            self.store.lock().unwrap().remove(&self.handle);
            Ok(())
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    struct MockCredentialBuilder {
        store: SharedStore,
    }

    impl CredentialBuilderApi for MockCredentialBuilder {
        fn build(
            &self, _target: Option<&str>, service: &str, user: &str,
        ) -> keyring::Result<Box<keyring::Credential>> {
            let credential = MockEntry {
                handle: format!("{service}:{user}"),
                store: self.store.clone(),
            };
            Ok(Box::new(credential))
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn persistence(&self) -> CredentialPersistence {
            CredentialPersistence::ProcessOnly
        }
    }

    /// Replace the global keyring backend with a shared in-memory store.
    fn use_mock_keyring() {
        let store = SharedStore::default();
        keyring::set_default_credential_builder(Box::new(MockCredentialBuilder { store }));
    }

    // -- Tests ----------------------------------------------------------------

    #[test]
    fn test_keyring_operations() {
        use_mock_keyring();

        let entry = Entry::new("test_service", "test_name").unwrap();
        assert!(entry.set_password("test_password").is_ok());
        assert_eq!(entry.get_password().unwrap(), "test_password");
        assert!(entry.delete_credential().is_ok());
        // After delete, get_password should return NoEntry
        assert!(entry.get_password().is_err());
    }

    #[test]
    fn test_store_get_delete_key() {
        use_mock_keyring();

        let result = store_key("test_service", "test_name", "test_password");
        assert!(result.is_ok());

        let password = get_key("test_service", "test_name");
        assert!(password.is_ok());
        assert_eq!(password.unwrap(), "test_password");

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

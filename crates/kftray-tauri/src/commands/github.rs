use std::path::{
    Component,
    Path,
    PathBuf,
};

use keyring_core::{
    Entry,
    Error as KeyringError,
};
use kftray_commons::utils::config_dir::get_config_dir;
use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_commons::utils::github::{
    GitHubConfig,
    GitHubRepository,
};
use kftray_telemetry::{
    Operation,
    Phase,
    phase,
};
use tauri::{
    Error as TauriError,
    ipc::InvokeError,
};

use crate::commands::measured::{
    MeasuredError,
    measured,
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

fn lexically_normalized(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match normalized.components().next_back() {
                Some(Component::Normal(_)) => {
                    normalized.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => normalized.push(component),
            },
            _ => normalized.push(component),
        }
    }

    normalized
}

fn scoped_account(name: &str, config_dir: Option<&Path>) -> String {
    match config_dir {
        Some(dir) => {
            let normalized = dir.canonicalize().unwrap_or_else(|_| {
                lexically_normalized(
                    &std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf()),
                )
            });

            format!("{name}@{}", normalized.display())
        }
        None => name.to_owned(),
    }
}

fn scoped_entry(service: &str, name: &str) -> Result<Entry, CustomError> {
    let config_dir = std::env::var("KFTRAY_CONFIG")
        .is_ok()
        .then(get_config_dir)
        .and_then(Result::ok);
    let account = scoped_account(name, config_dir.as_deref());

    Entry::new(service, &account).map_err(CustomError::from)
}

#[tauri::command]
pub fn store_key(
    service: &str, name: &str, password: &str,
) -> std::result::Result<(), CustomError> {
    scoped_entry(service, name)?
        .set_password(password)
        .map_err(CustomError::from)
}

#[tauri::command]
pub fn get_key(service: &str, name: &str) -> std::result::Result<Option<String>, CustomError> {
    match scoped_entry(service, name)?.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(KeyringError::NoEntry) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[tauri::command]
pub fn delete_key(service: &str, name: &str) -> std::result::Result<(), CustomError> {
    scoped_entry(service, name)?
        .delete_credential()
        .map_err(CustomError::from)
}

async fn import_config_content(config_content: &str, flush: bool) -> Result<(), String> {
    if !flush {
        return GitHubRepository::process_config_content(config_content, false, DatabaseMode::File)
            .await;
    }

    let ids = saved_config_ids().await?;
    flush_and_import(&ids, config_content).await
}

async fn saved_config_ids() -> Result<Vec<i64>, String> {
    Ok(kftray_commons::utils::config::get_configs()
        .await?
        .into_iter()
        .filter_map(|config| config.id)
        .collect())
}

async fn flush_and_import(ids: &[i64], config_content: &str) -> Result<(), String> {
    kftray_portforward::kube::delete_configs_if_idle(ids, DatabaseMode::File, || async move {
        let unguarded: Vec<String> = saved_config_ids()
            .await?
            .into_iter()
            .filter(|id| !ids.contains(id))
            .map(|id| id.to_string())
            .collect();
        if !unguarded.is_empty() {
            return Err(format!(
                "Configurations were saved during the sync: {}. Sync again.",
                unguarded.join(", ")
            ));
        }
        GitHubRepository::process_config_content(config_content, true, DatabaseMode::File).await
    })
    .await
}

#[tauri::command]
pub async fn import_configs_from_github(
    repo_url: String, config_paths: Vec<String>, use_system_credentials: bool, flush: bool,
    github_token: Option<String>,
) -> Result<(), MeasuredError> {
    let config = GitHubConfig {
        repo_url,
        config_paths,
        use_system_credentials,
        github_token,
        flush_existing: flush,
    };

    measured(Operation::GitSync, async {
        let guard = phase(Phase::GitFetch);
        let config_content = GitHubRepository::fetch_config_content(&config)?;
        drop(guard);

        let _guard = phase(Phase::GitImport);
        import_config_content(&config_content, config.flush_existing).await
    })
    .await
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Mutex,
        MutexGuard,
    };

    use super::*;

    static KEYRING_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn use_mock_keyring() -> MutexGuard<'static, ()> {
        let guard = KEYRING_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        keyring_core::set_default_store(keyring_core::mock::Store::new().unwrap());
        guard
    }

    #[test]
    fn test_keyring_operations() {
        let _guard = use_mock_keyring();

        let entry = Entry::new("test_service", "test_name").unwrap();
        assert!(entry.set_password("test_password").is_ok());
        assert_eq!(entry.get_password().unwrap(), "test_password");
        assert!(entry.delete_credential().is_ok());
        assert!(matches!(entry.get_password(), Err(KeyringError::NoEntry)));
    }

    #[test]
    fn test_store_get_delete_key() {
        let _guard = use_mock_keyring();

        let result = store_key("test_service", "test_name", "test_password");
        assert!(result.is_ok());

        let password = get_key("test_service", "test_name");
        assert_eq!(password.unwrap().as_deref(), Some("test_password"));

        let result = delete_key("test_service", "test_name");
        assert!(result.is_ok());
    }

    #[test]
    fn get_key_returns_none_for_missing_entry() {
        let _guard = use_mock_keyring();

        assert_eq!(get_key("test_service", "missing_name").unwrap(), None);
    }

    #[test]
    fn keychain_account_is_scoped_only_for_custom_config_dirs() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("kftray-a");

        assert_eq!(scoped_account("github_config", None), "github_config");
        assert_eq!(
            scoped_account("github_config", Some(&missing)),
            format!("github_config@{}", missing.display())
        );
        assert_ne!(
            scoped_account("github_config", Some(&missing)),
            scoped_account("github_config", Some(&temp.path().join("kftray-b")))
        );
    }

    #[test]
    fn keychain_account_is_absolute_for_relative_missing_config_dirs() {
        let cwd = std::env::current_dir().unwrap();
        let relative = scoped_account("github_config", Some(Path::new("nonexistent-rel/kftray")));

        assert_eq!(
            relative,
            scoped_account(
                "github_config",
                Some(&cwd.join("nonexistent-rel").join("kftray"))
            )
        );
        assert!(relative.starts_with(&format!("github_config@{}", cwd.display())));
        assert_ne!(
            relative,
            scoped_account("github_config", Some(Path::new("nonexistent-other/kftray")))
        );
    }

    #[test]
    fn keychain_account_is_equal_for_equivalent_missing_config_dir_spellings() {
        let expected = scoped_account("github_config", Some(Path::new("/nonexistent/x/kftray")));

        for spelling in [
            "/nonexistent/x/kftray/",
            "/nonexistent/x/kftray//",
            "/nonexistent/x/./kftray",
            "/nonexistent/../nonexistent/x/kftray",
            "/nonexistent/x/../x/kftray",
        ] {
            assert_eq!(
                scoped_account("github_config", Some(Path::new(spelling))),
                expected,
                "{spelling}"
            );
        }
    }

    #[test]
    fn keychain_account_is_equal_for_equivalent_existing_config_dir_spellings() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("kftray");
        std::fs::create_dir(&dir).unwrap();
        let other = temp.path().join("other");
        std::fs::create_dir(&other).unwrap();

        let expected = scoped_account("github_config", Some(&dir));

        assert_eq!(
            scoped_account("github_config", Some(&dir.join(""))),
            expected
        );
        assert_eq!(
            scoped_account("github_config", Some(&other.join("..").join("kftray"))),
            expected
        );
        assert_ne!(scoped_account("github_config", Some(&other)), expected);
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
            vec!["config.json".to_string()],
            false,
            false,
            None,
        )
        .await;

        assert!(result.is_err(), "Should fail with invalid repository");
        let err = result.unwrap_err().message;
        assert!(
            err.contains("Failed to clone repository") || err.contains("failed to resolve address"),
            "Error should indicate clone or network failure: {err}"
        );
    }

    async fn use_file_db() -> (
        kftray_commons::test_utils::TestDb,
        tokio::sync::MutexGuard<'static, ()>,
        kftray_commons::test_utils::EnvVarGuard,
        tempfile::TempDir,
    ) {
        let db = kftray_commons::test_utils::test_db().await;
        let process_lock = kftray_portforward::port_forward::PROCESS_TEST_MUTEX
            .lock()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let env = kftray_commons::test_utils::EnvVarGuard::set(
            "KFTRAY_CONFIG",
            dir.path().to_str().unwrap(),
        );
        kftray_commons::utils::db::init().await.unwrap();
        kftray_commons::utils::migration::migrate_configs(None)
            .await
            .unwrap();
        (db, process_lock, env, dir)
    }

    fn service_config(name: &str) -> kftray_commons::models::config_model::Config {
        kftray_commons::models::config_model::Config {
            alias: Some(name.to_string()),
            service: Some(name.to_string()),
            namespace: "default".to_string(),
            protocol: "tcp".to_string(),
            workload_type: Some("service".to_string()),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn a_flush_sync_refuses_to_replace_configs_while_a_forward_is_running() {
        let _file_db = use_file_db().await;
        kftray_commons::utils::config::insert_config(service_config("api"))
            .await
            .unwrap();
        let web = kftray_commons::utils::config::insert_config(service_config("web"))
            .await
            .unwrap();
        kftray_commons::utils::config_state::update_config_state(
            &kftray_commons::models::config_state_model::ConfigState::new(web, true),
        )
        .await
        .unwrap();
        let handle: tokio::task::JoinHandle<anyhow::Result<()>> = tokio::spawn(async { Ok(()) });
        kftray_portforward::port_forward::CHILD_PROCESSES.insert(
            web,
            kftray_portforward::port_forward::PortForwardProcess::new(handle, web.to_string()),
        );
        let repo_file = kftray_commons::utils::config::export_configs()
            .await
            .unwrap();

        let result = import_config_content(&repo_file, true).await;
        kftray_portforward::port_forward::CHILD_PROCESSES.remove(&web);

        assert_eq!(
            result,
            Err(format!(
                "Stop these configurations before deleting them: {web}"
            ))
        );
        let states = kftray_commons::utils::config_state::get_configs_state()
            .await
            .unwrap();
        assert!(
            states
                .iter()
                .any(|state| state.config_id == web && state.is_running),
            "{states:?}"
        );
    }

    #[tokio::test]
    async fn a_flush_sync_with_no_running_forward_replaces_the_saved_configs() {
        let _file_db = use_file_db().await;
        kftray_commons::utils::config::insert_config(service_config("api"))
            .await
            .unwrap();
        let repo_file = kftray_commons::utils::config::export_configs()
            .await
            .unwrap();
        kftray_commons::utils::config::insert_config(service_config("web"))
            .await
            .unwrap();

        import_config_content(&repo_file, true).await.unwrap();

        let aliases: Vec<Option<String>> = kftray_commons::utils::config::get_configs()
            .await
            .unwrap()
            .into_iter()
            .map(|config| config.alias)
            .collect();
        assert_eq!(aliases, vec![Some("api".to_string())]);
    }

    #[tokio::test]
    async fn a_flush_sync_keeps_a_config_saved_and_started_after_it_read_the_saved_ids() {
        let _file_db = use_file_db().await;
        kftray_commons::utils::config::insert_config(service_config("api"))
            .await
            .unwrap();
        let repo_file = kftray_commons::utils::config::export_configs()
            .await
            .unwrap();
        let ids_read_by_the_sync = saved_config_ids().await.unwrap();
        let web = kftray_commons::utils::config::insert_config(service_config("web"))
            .await
            .unwrap();
        kftray_commons::utils::config_state::update_config_state(
            &kftray_commons::models::config_state_model::ConfigState::new(web, true),
        )
        .await
        .unwrap();
        let handle: tokio::task::JoinHandle<anyhow::Result<()>> = tokio::spawn(async { Ok(()) });
        kftray_portforward::port_forward::CHILD_PROCESSES.insert(
            web,
            kftray_portforward::port_forward::PortForwardProcess::new(handle, web.to_string()),
        );

        let result = flush_and_import(&ids_read_by_the_sync, &repo_file).await;
        kftray_portforward::port_forward::CHILD_PROCESSES.remove(&web);

        assert_eq!(
            result,
            Err(format!(
                "Configurations were saved during the sync: {web}. Sync again."
            ))
        );
        let aliases: Vec<Option<String>> = kftray_commons::utils::config::get_configs()
            .await
            .unwrap()
            .into_iter()
            .map(|config| config.alias)
            .collect();
        assert_eq!(
            aliases,
            vec![Some("api".to_string()), Some("web".to_string())]
        );
        let states = kftray_commons::utils::config_state::get_configs_state()
            .await
            .unwrap();
        assert!(
            states
                .iter()
                .any(|state| state.config_id == web && state.is_running),
            "{states:?}"
        );
    }
}

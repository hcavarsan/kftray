use std::path::Path;
use std::path::PathBuf;

use git2::{
    CertificateCheckStatus,
    Cred,
    FetchOptions,
    RemoteCallbacks,
    Repository,
};
use keyring::{
    Entry,
    Error as KeyringError,
};
use kftray_commons::{
    models::config_model::Config,
    utils::config::import_configs,
    utils::github::clear_existing_configs,
    utils::migration::migrate_configs,
};
use log::{
    error,
    info,
    warn,
};
use reqwest::Url;
use tauri::{
    Error as TauriError,
    InvokeError,
};
use tempfile::TempDir;

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
            CustomError::Keyring(err) => InvokeError::from(format!("Keyring error: {:?}", err)),
            CustomError::Tauri(err) => InvokeError::from(format!("Tauri error: {:?}", err)),
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

mod credentials {
    use super::*;

    pub fn try_credentials_from_file() -> Vec<(String, String)> {
        let home_dir = std::env::var("HOME").unwrap_or_default();
        let credentials_path = Path::new(&home_dir).join(".git-credentials");

        match std::fs::read_to_string(credentials_path) {
            Ok(content) => parse_git_credentials(&content),
            Err(_) => Vec::new(),
        }
    }

    fn parse_git_credentials(content: &str) -> Vec<(String, String)> {
        content
            .lines()
            .filter_map(|line| {
                Url::parse(line.trim()).ok().and_then(|url| {
                    let username = url.username().to_string();
                    let password = url.password()?.to_string();
                    if !username.is_empty() {
                        Some((username, password))
                    } else {
                        None
                    }
                })
            })
            .collect()
    }

    pub fn get_git_credentials(url: &str, username: &str) -> Result<Cred, git2::Error> {
        info!("Getting credentials for URL: {}", url);

        if url.starts_with("git@") || url.starts_with("ssh://") {
            if let Ok(cred) = try_ssh_authentication(username) {
                return Ok(cred);
            }
        }
        if let Ok(cred) = try_credential_helper(url, username) {
            return Ok(cred);
        }

        // Fall back to stored credentials
        try_stored_credentials()
    }

    fn try_ssh_agent(username: &str) -> Result<Cred, git2::Error> {
        match Cred::ssh_key_from_agent(username) {
            Ok(cred) => {
                info!("Successfully authenticated with SSH agent");
                Ok(cred)
            }
            Err(e) => {
                info!("SSH agent authentication failed: {}", e);
                Err(e)
            }
        }
    }

    fn try_git_ssh_config(username: &str) -> Result<Cred, git2::Error> {
        let config = git2::Config::open_default()?;

        let key_path_str = config.get_string("core.sshCommand").map_err(|e| {
            info!("No core.sshCommand in git config: {}", e);
            e
        })?;

        let key_arg_pos = key_path_str.find(" -i ").ok_or_else(|| {
            info!("No -i flag in core.sshCommand");
            git2::Error::from_str("No -i flag in core.sshCommand")
        })?;

        let key_path_start = key_arg_pos + 4;
        let key_path_end = key_path_str[key_path_start..]
            .find(' ')
            .map(|pos| key_path_start + pos)
            .unwrap_or(key_path_str.len());

        let key_path_str = &key_path_str[key_path_start..key_path_end];
        let key_path = Path::new(key_path_str);

        if !key_path.exists() {
            info!(
                "SSH key from git config doesn't exist: {}",
                key_path.display()
            );
            return Err(git2::Error::from_str(
                "SSH key from git config doesn't exist",
            ));
        }

        info!("Trying SSH key from git config: {}", key_path.display());
        match Cred::ssh_key(username, None, key_path, None) {
            Ok(cred) => {
                info!("Successfully authenticated with SSH key from git config");
                Ok(cred)
            }
            Err(e) => {
                info!("Failed to use SSH key from git config: {}", e);
                Err(e)
            }
        }
    }

    fn try_env_ssh_key(username: &str) -> Result<Cred, git2::Error> {
        let key_path_str = std::env::var("SSH_KEY_PATH")
            .map_err(|_| git2::Error::from_str("SSH_KEY_PATH environment variable not set"))?;

        let key_path = PathBuf::from(key_path_str);
        if !key_path.exists() {
            info!(
                "SSH key from environment variable doesn't exist: {}",
                key_path.display()
            );
            return Err(git2::Error::from_str(
                "SSH key from environment variable doesn't exist",
            ));
        }

        info!("Trying SSH key from SSH_KEY_PATH: {}", key_path.display());
        match Cred::ssh_key(username, None, &key_path, None) {
            Ok(cred) => {
                info!("Successfully authenticated with SSH key from environment variable");
                Ok(cred)
            }
            Err(e) => {
                info!("Failed to use SSH key from environment variable: {}", e);
                Err(e)
            }
        }
    }

    fn get_ssh_directories() -> Vec<PathBuf> {
        let mut ssh_dirs = Vec::new();

        if let Ok(home_dir) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
            ssh_dirs.push(PathBuf::from(home_dir).join(".ssh"));
        }

        if let Ok(custom_ssh_dir) = std::env::var("SSH_DIR") {
            ssh_dirs.push(PathBuf::from(custom_ssh_dir));
        }

        ssh_dirs
    }

    fn try_standard_key_names(username: &str, ssh_dirs: &[PathBuf]) -> Result<Cred, git2::Error> {
        let key_names = ["id_ed25519", "id_rsa", "id_ecdsa", "id_dsa"];

        for dir in ssh_dirs {
            if !dir.exists() || !dir.is_dir() {
                continue;
            }

            for key_name in &key_names {
                let key_path = dir.join(key_name);
                if key_path.exists() {
                    info!("Trying standard SSH key: {}", key_path.display());
                    if let Ok(cred) = Cred::ssh_key(username, None, &key_path, None) {
                        info!(
                            "Successfully authenticated with standard SSH key: {}",
                            key_name
                        );
                        return Ok(cred);
                    }
                }
            }
        }

        Err(git2::Error::from_str(
            "No standard SSH keys found or none worked",
        ))
    }

    fn try_key_file(username: &str, path: &Path) -> Result<Cred, git2::Error> {
        if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
            if file_name.ends_with(".pub")
                || file_name == "known_hosts"
                || file_name == "authorized_keys"
                || file_name == "config"
            {
                return Err(git2::Error::from_str("Not a private key file"));
            }
        } else {
            return Err(git2::Error::from_str("Invalid file name"));
        }

        info!("Trying potential SSH key: {}", path.display());
        match Cred::ssh_key(username, None, path, None) {
            Ok(cred) => {
                info!(
                    "Successfully authenticated with SSH key: {}",
                    path.display()
                );
                Ok(cred)
            }
            Err(e) => Err(e),
        }
    }

    fn scan_directory_for_keys(username: &str, dir: &Path) -> Result<Cred, git2::Error> {
        if !dir.exists() || !dir.is_dir() {
            return Err(git2::Error::from_str(
                "Directory doesn't exist or is not a directory",
            ));
        }

        info!("Scanning for SSH keys in: {}", dir.display());

        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => {
                info!("Failed to read SSH directory {}: {}", dir.display(), e);
                return Err(git2::Error::from_str(&format!(
                    "Failed to read directory: {}",
                    e
                )));
            }
        };

        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };

            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            if let Ok(cred) = try_key_file(username, &path) {
                return Ok(cred);
            }
        }

        Err(git2::Error::from_str(
            "No valid SSH keys found in directory",
        ))
    }

    fn scan_subdirectories_for_keys(username: &str, dir: &Path) -> Result<Cred, git2::Error> {
        if !dir.exists() || !dir.is_dir() {
            return Err(git2::Error::from_str(
                "Directory doesn't exist or is not a directory",
            ));
        }

        let subdirs = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => {
                info!("Failed to read directory {}: {}", dir.display(), e);
                return Err(git2::Error::from_str(&format!(
                    "Failed to read directory: {}",
                    e
                )));
            }
        };

        for subdir_entry in subdirs {
            let subdir_entry = match subdir_entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };

            let subdir_path = subdir_entry.path();
            if !subdir_path.is_dir() {
                continue;
            }

            let subdir_entries = match std::fs::read_dir(&subdir_path) {
                Ok(entries) => entries,
                Err(_) => continue,
            };

            for file_entry in subdir_entries {
                let file_entry = match file_entry {
                    Ok(entry) => entry,
                    Err(_) => continue,
                };

                let file_path = file_entry.path();
                if !file_path.is_file() {
                    continue;
                }

                if let Ok(cred) = try_key_file(username, &file_path) {
                    return Ok(cred);
                }
            }
        }

        Err(git2::Error::from_str(
            "No valid SSH keys found in subdirectories",
        ))
    }

    fn try_ssh_authentication(username: &str) -> Result<Cred, git2::Error> {
        if let Ok(cred) = try_ssh_agent(username) {
            return Ok(cred);
        }

        if let Ok(cred) = try_git_ssh_config(username) {
            return Ok(cred);
        }

        if let Ok(cred) = try_env_ssh_key(username) {
            return Ok(cred);
        }

        let ssh_dirs = get_ssh_directories();

        if let Ok(cred) = try_standard_key_names(username, &ssh_dirs) {
            return Ok(cred);
        }

        for dir in &ssh_dirs {
            if let Ok(cred) = scan_directory_for_keys(username, dir) {
                return Ok(cred);
            }

            if let Ok(cred) = scan_subdirectories_for_keys(username, dir) {
                return Ok(cred);
            }
        }

        Err(git2::Error::from_str(
            "SSH authentication failed: no valid SSH keys found",
        ))
    }

    fn try_credential_helper(url: &str, username: &str) -> Result<Cred, git2::Error> {
        if let Ok(config) = git2::Config::open_default() {
            match Cred::credential_helper(&config, url, Some(username)) {
                Ok(cred) => {
                    info!("Successfully retrieved credentials from OS credential store");
                    Ok(cred)
                }
                Err(e) => {
                    info!("Credential helper failed: {}", e);
                    Err(e)
                }
            }
        } else {
            Err(git2::Error::from_str("Failed to open git config"))
        }
    }

    fn try_stored_credentials() -> Result<Cred, git2::Error> {
        let credentials = try_credentials_from_file();

        for (username, password) in credentials {
            info!("Trying stored credentials for username: {}", username);
            if let Ok(cred) = Cred::userpass_plaintext(&username, &password) {
                info!("Successfully authenticated with stored credentials");
                return Ok(cred);
            }
        }

        Err(git2::Error::from_str("No valid credentials found"))
    }
}

fn clone_and_read_config(
    repo_url: &str, config_path: &str, use_system_credentials: bool, github_token: Option<String>,
) -> Result<String, String> {
    let temp_dir = TempDir::new().map_err(|e| format!("Failed to create temp dir: {}", e))?;

    let callbacks = setup_git_callbacks(use_system_credentials, github_token);
    let mut builder = setup_repo_builder(callbacks);

    info!("Attempting to clone repository: {}", repo_url);
    clone_repository(&mut builder, repo_url, temp_dir.path())?;

    read_config_file(temp_dir.path(), config_path)
}

fn setup_git_callbacks(
    use_system_credentials: bool, github_token: Option<String>,
) -> RemoteCallbacks<'static> {
    let mut callbacks = RemoteCallbacks::new();

    // Only set up credentials callback if authentication is needed
    if use_system_credentials || github_token.is_some() {
        let token = github_token.clone();

        let attempts = std::sync::atomic::AtomicUsize::new(0);

        callbacks.credentials(move |url, username_from_url, allowed_types| {
            let current_attempt = attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if current_attempt >= 3 {
                return Err(git2::Error::from_str(
                    "Authentication failed after 3 attempts",
                ));
            }

            info!(
                "Auth attempt {} - URL: {}, Username: {:?}, Allowed types: {:?}",
                current_attempt + 1,
                url,
                username_from_url,
                allowed_types
            );

            let is_https_url = url.starts_with("https://");

            if is_https_url
                && token.is_some()
                && allowed_types.contains(git2::CredentialType::USER_PASS_PLAINTEXT)
            {
                info!("Using token authentication for HTTPS");
                return Cred::userpass_plaintext("git", token.as_ref().unwrap());
            }

            if use_system_credentials {
                let username = username_from_url.unwrap_or("git");
                credentials::get_git_credentials(url, username)
            } else {
                Err(git2::Error::from_str("No authentication method configured"))
            }
        });
    }

    callbacks.certificate_check(|_cert, _hostname| Ok(CertificateCheckStatus::CertificateOk));

    callbacks
}

fn setup_repo_builder(callbacks: RemoteCallbacks) -> git2::build::RepoBuilder {
    let mut fetch_opts = FetchOptions::new();
    fetch_opts.remote_callbacks(callbacks);

    let mut builder = git2::build::RepoBuilder::new();
    builder.fetch_options(fetch_opts);
    builder
}

fn clone_repository(
    builder: &mut git2::build::RepoBuilder, repo_url: &str, path: &Path,
) -> Result<Repository, String> {
    match builder.clone(repo_url, path) {
        Ok(repo) => Ok(repo),
        Err(e) => {
            warn!(
                "Repository clone failed: {}, trying fallback with system git command",
                e
            );

            try_clone_with_system_git(repo_url, path).map_err(|err| format!(
                    "Failed to clone repository. Please check your credentials and repository URL. Error: {}",
                    err
                ))
        }
    }
}

fn try_clone_with_system_git(repo_url: &str, path: &Path) -> Result<Repository, String> {
    use std::process::Command;

    let output = Command::new("git")
        .arg("clone")
        .arg("--depth=1")
        .arg("--single-branch")
        .arg("--no-tags")
        .arg("--filter=blob:none")
        .arg("--recurse-submodules=no")
        .arg(repo_url)
        .arg(path)
        .output()
        .map_err(|e| format!("Failed to execute git command: {}", e))?;

    if output.status.success() {
        info!("Successfully cloned repository using system git");
        Repository::open(path).map_err(|e| format!("Failed to open repository: {}", e))
    } else {
        let error_msg = String::from_utf8_lossy(&output.stderr);
        error!("System git clone failed: {}", error_msg);
        Err(format!("System git clone failed: {}", error_msg))
    }
}

fn read_config_file(temp_dir: &Path, config_path: &str) -> Result<String, String> {
    let config_path = Path::new(config_path);
    let full_path = temp_dir.join(config_path);

    std::fs::read_to_string(&full_path).map_err(|e| {
        format!(
            "Failed to read config file at {}: {}",
            full_path.display(),
            e
        )
    })
}

async fn process_config_content(config_content: &str, flush: bool) -> Result<(), String> {
    let configs: Vec<Config> = serde_json::from_str(config_content)
        .map_err(|e| format!("Failed to parse config JSON: {}", e))?;

    if flush {
        info!("Clearing existing configurations");
        clear_existing_configs().await.map_err(|e| e.to_string())?;
    }

    import_configurations(&configs).await?;
    migrate_configurations().await
}

async fn import_configurations(configs: &[Config]) -> Result<(), String> {
    info!("Importing {} new configurations", configs.len());
    for config in configs {
        let config_json = serde_json::to_string(config)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        import_configs(config_json).await?;
    }
    Ok(())
}

async fn migrate_configurations() -> Result<(), String> {
    match migrate_configs(None).await {
        Ok(_) => {
            info!("Configuration import completed successfully");
            Ok(())
        }
        Err(e) => {
            error!("Error migrating configs: {}. Please check if the configurations are valid and compatible with the current system/version.", e);
            Err(format!("Config migration failed: {}", e))
        }
    }
}

#[tauri::command]
pub async fn import_configs_from_github(
    repo_url: String, config_path: String, use_system_credentials: bool, flush: bool,
    github_token: Option<String>,
) -> Result<(), String> {
    let config_content = clone_and_read_config(
        &repo_url,
        &config_path,
        use_system_credentials,
        github_token,
    )?;
    process_config_content(&config_content, flush).await
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

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
    fn test_read_config_file() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let config_path = "test_config.yaml";
        let full_path = temp_dir.path().join(config_path);

        fs::write(&full_path, "test_content").expect("Failed to write test content");

        let result = read_config_file(temp_dir.path(), config_path);
        assert!(result.is_ok(), "Failed to read config file");
        assert_eq!(result.unwrap(), "test_content", "Config content mismatch");
    }

    #[test]
    fn test_read_config_file_not_found() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let result = read_config_file(temp_dir.path(), "nonexistent.yaml");
        assert!(result.is_err(), "Should fail when file doesn't exist");
        assert!(
            result.unwrap_err().contains("Failed to read config file"),
            "Error message should indicate file reading failure"
        );
    }

    #[test]
    fn test_try_credentials_from_file() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let cred_file = temp_dir.path().join(".git-credentials");
        fs::write(&cred_file, "https://testuser:testpass@github.com\n")
            .expect("Failed to write credentials file");

        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", temp_dir.path().to_str().unwrap());

        let credentials = credentials::try_credentials_from_file();
        assert_eq!(credentials.len(), 1, "Should find exactly one credential");
        assert_eq!(credentials[0].0, "testuser", "Username mismatch");
        assert_eq!(credentials[0].1, "testpass", "Password mismatch");

        if let Some(home) = original_home {
            std::env::set_var("HOME", home);
        } else {
            std::env::remove_var("HOME");
        }
    }

    #[tokio::test]
    async fn test_process_config_content() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let db_path = temp_dir.path().join("test.db");
        std::env::set_var("DATABASE_URL", format!("sqlite:{}", db_path.display()));

        let config_content = r#"[{
            "name": "test_config",
            "description": "test description",
            "local_port": 8080,
            "remote_port": 80,
            "namespace": "default",
            "service": "test-service"
        }]"#;

        let result = process_config_content(config_content, false).await;
        assert!(
            result.is_ok(),
            "Failed to process valid config content without flush: {:?}",
            result
        );

        let result = process_config_content(config_content, true).await;
        assert!(
            result.is_ok(),
            "Failed to process valid config content with flush: {:?}",
            result
        );

        std::env::remove_var("DATABASE_URL");
    }

    #[test]
    fn test_setup_git_callbacks() {
        let callbacks = setup_git_callbacks(true, Some("test_token".to_string()));
        let mut builder = setup_repo_builder(callbacks);
        let result = builder.clone(
            "https://github.com/nonexistent/repo",
            std::path::Path::new("/tmp"),
        );
        assert!(
            result.is_err(),
            "Clone should fail but builder should be properly configured"
        );

        let callbacks = setup_git_callbacks(false, None);
        let mut builder = setup_repo_builder(callbacks);
        let result = builder.clone(
            "https://github.com/nonexistent/repo",
            std::path::Path::new("/tmp"),
        );
        assert!(
            result.is_err(),
            "Clone should fail but builder should be properly configured"
        );
    }

    #[test]
    fn test_setup_repo_builder() {
        let callbacks = setup_git_callbacks(true, None);
        let mut builder = setup_repo_builder(callbacks);
        builder.bare(false);
        assert!(
            builder.clone("test", std::path::Path::new("/tmp")).is_err(),
            "Builder should be properly configured but fail on invalid repo"
        );
    }

    #[test]
    fn test_custom_error_conversion() {
        let keyring_error = KeyringError::NoEntry;
        let custom_error = CustomError::from(keyring_error);
        match custom_error {
            CustomError::Keyring(_) => (),
            _ => panic!("Wrong error variant for KeyringError conversion"),
        }

        let tauri_error = TauriError::InvalidWindowUrl("test");
        let custom_error = CustomError::from(tauri_error);
        match custom_error {
            CustomError::Tauri(_) => (),
            _ => panic!("Wrong error variant for TauriError conversion"),
        }

        let custom_error = CustomError::Keyring(KeyringError::NoEntry);
        let invoke_error = InvokeError::from(custom_error);
        let error_str = format!("{:?}", invoke_error);
        assert!(
            error_str.contains("Keyring error") && error_str.contains("NoEntry"),
            "InvokeError should contain both the error type and variant: {}",
            error_str
        );
    }

    #[test]
    fn test_clone_repository() {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let callbacks = setup_git_callbacks(true, Some("test_token".to_string()));
        let mut builder = setup_repo_builder(callbacks);

        let result = clone_repository(
            &mut builder,
            "https://github.com/nonexistent/repo",
            temp_dir.path(),
        );

        assert!(result.is_err(), "Should fail with invalid repository");
        if let Err(err) = result {
            assert!(
                err.contains("Failed to clone repository"),
                "Error should indicate clone failure"
            );
        }
    }

    #[test]
    fn test_clone_and_read_config() {
        let result = clone_and_read_config(
            "https://github.com/nonexistent/repo",
            "config.yaml",
            true,
            Some("test_token".to_string()),
        );

        assert!(result.is_err(), "Should fail with invalid repository");
        let err = result.unwrap_err();
        assert!(
            err.contains("Failed to clone repository") || err.contains("failed to resolve address"),
            "Error should indicate clone or network failure: {}",
            err
        );
    }

    #[test]
    fn test_git_credentials() {
        let callbacks = setup_git_callbacks(true, Some("test_token".to_string()));
        assert!(
            std::mem::size_of_val(&callbacks) > 0,
            "Callbacks should be configured"
        );

        let callbacks = setup_git_callbacks(false, None);
        assert!(
            std::mem::size_of_val(&callbacks) > 0,
            "Callbacks should be configured"
        );
    }
}

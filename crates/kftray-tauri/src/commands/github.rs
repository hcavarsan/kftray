use std::path::Path;

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
        info!(
            "Attempting to get credentials for URL: {} and username: {}",
            url, username
        );

        // Try default config first
        if let Ok(config) = get_git_config() {
            if let Ok(cred) = try_credential_helper(&config, url, username) {
                return Ok(cred);
            }
        }

        // Fall back to stored credentials
        try_stored_credentials(username)
    }

    fn get_git_config() -> Result<git2::Config, git2::Error> {
        git2::Config::open_default()
            .or_else(|_| Repository::open_from_env().and_then(|r| r.config()))
    }

    fn try_credential_helper(
        config: &git2::Config, url: &str, username: &str,
    ) -> Result<Cred, git2::Error> {
        match Cred::credential_helper(config, url, Some(username)) {
            Ok(cred) => {
                info!(
                    "Successfully retrieved credentials for username: {}",
                    username
                );
                Ok(cred)
            }
            Err(e) => {
                info!("Failed to get credentials for {}: {}", username, e);
                Err(e)
            }
        }
    }

    fn try_stored_credentials(_username: &str) -> Result<Cred, git2::Error> {
        let stored_credentials = try_credentials_from_file();
        info!(
            "Found {} stored credentials to try",
            stored_credentials.len()
        );

        for (stored_username, password) in stored_credentials {
            info!(
                "Trying stored credentials for username: {}",
                stored_username
            );
            if let Ok(cred) = Cred::userpass_plaintext(&stored_username, &password) {
                info!(
                    "Successfully created credentials for username: {}",
                    stored_username
                );
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
        callbacks.credentials(move |url, username_from_url, allowed_types| {
            info!(
                "Auth attempt - URL: {}, Username: {:?}, Allowed types: {:?}",
                url, username_from_url, allowed_types
            );

            if let Some(token) = &token {
                // Use GitHub token if provided
                Cred::userpass_plaintext("git", token)
            } else if use_system_credentials {
                // Use system credentials only if explicitly requested
                let initial_username = username_from_url.unwrap_or("git");
                credentials::get_git_credentials(url, initial_username)
            } else {
                // This shouldn't be reached due to the outer if condition
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
    builder.clone(repo_url, path).map_err(|e| {
        error!("Repository clone failed: {}", e);
        format!(
            "Failed to clone repository: {}. Please check your credentials and repository URL.",
            e
        )
    })
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
    match migrate_configs().await {
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

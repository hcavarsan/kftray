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

            try_clone_with_system_git(repo_url, path).or_else(|err| {
                Err(format!(
                    "Failed to clone repository. Please check your credentials and repository URL. Error: {}",
                    err
                ))
            })
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

use std::path::Path;
use std::process::Command;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use log::{
    error,
    info,
    warn,
};
use sqlx::SqlitePool;

use crate::db::get_db_pool;
use crate::utils::db_mode::{
    DatabaseManager,
    DatabaseMode,
};

pub struct GitHubConfig {
    pub repo_url: String,
    pub config_paths: Vec<String>,
    pub use_system_credentials: bool,
    pub github_token: Option<String>,
    pub flush_existing: bool,
}

pub type GitHubResult<T> = Result<T, String>;

pub struct GitHubRepository;

impl GitHubRepository {
    pub async fn import_configs(config: GitHubConfig, mode: DatabaseMode) -> GitHubResult<()> {
        let config_content = Self::fetch_config_content(&config)?;

        Self::process_config_content(&config_content, config.flush_existing, mode).await
    }

    pub fn fetch_config_content(config: &GitHubConfig) -> GitHubResult<String> {
        if config.config_paths.is_empty() {
            return Err("At least one config path must be provided".to_string());
        }

        Self::clone_and_read_config(
            &config.repo_url,
            &config.config_paths,
            config.use_system_credentials,
            config.github_token.clone(),
        )
    }

    fn clone_and_read_config(
        repo_url: &str, config_paths: &[String], use_system_credentials: bool,
        github_token: Option<String>,
    ) -> GitHubResult<String> {
        use git2::{
            CertificateCheckStatus,
            Cred,
            FetchOptions,
            RemoteCallbacks,
            build::RepoBuilder,
        };
        use tempfile::TempDir;

        let temp_dir = TempDir::new().map_err(|e| format!("Failed to create temp dir: {e}"))?;

        let mut callbacks = RemoteCallbacks::new();

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
                    "Auth attempt {} - URL: {}, Username: {:?}",
                    current_attempt + 1,
                    url,
                    username_from_url
                );

                let is_https_url = url.starts_with("https://");

                if is_https_url
                    && allowed_types.contains(git2::CredentialType::USER_PASS_PLAINTEXT)
                    && let Some(token) = &token
                {
                    info!("Using token authentication for HTTPS");
                    return Cred::userpass_plaintext("git", token);
                }

                if use_system_credentials {
                    let username = username_from_url.unwrap_or("git");
                    Self::get_system_credentials(url, username)
                } else {
                    Err(git2::Error::from_str("No authentication method configured"))
                }
            });
        }

        callbacks.certificate_check(|_cert, _hostname| Ok(CertificateCheckStatus::CertificateOk));

        let mut fetch_opts = FetchOptions::new();
        fetch_opts.remote_callbacks(callbacks);

        let mut builder = RepoBuilder::new();
        builder.fetch_options(fetch_opts);

        info!("Attempting to clone repository: {repo_url}");

        match builder.clone(repo_url, temp_dir.path()) {
            Ok(_) => {
                info!("Successfully cloned repository");
                Self::read_config_files(temp_dir.path(), config_paths)
            }
            Err(e) => {
                let token = github_token.as_deref().filter(|t| !t.is_empty());
                let git2_message = redact_token(e.message(), token);
                warn!(
                    "Repository clone failed: {git2_message}, trying fallback with system git command"
                );
                Self::try_clone_with_system_git(
                    repo_url,
                    temp_dir.path(),
                    config_paths,
                    use_system_credentials,
                    token,
                )
                .map_err(|fallback| {
                    clone_failure_message(&git2_message, is_git2_auth_error(&e), &fallback, token)
                })
            }
        }
    }

    fn get_system_credentials(url: &str, username: &str) -> Result<git2::Cred, git2::Error> {
        use git2::Cred;

        if (url.starts_with("git@") || url.starts_with("ssh://"))
            && let Ok(cred) = Cred::ssh_key_from_agent(username)
        {
            info!("Successfully authenticated with SSH agent");
            return Ok(cred);
        }

        if let Ok(config) = git2::Config::open_default()
            && let Ok(cred) = Cred::credential_helper(&config, url, Some(username))
        {
            info!("Successfully retrieved credentials from OS credential store");
            return Ok(cred);
        }

        Err(git2::Error::from_str("No valid credentials found"))
    }

    fn try_clone_with_system_git(
        repo_url: &str, path: &Path, config_paths: &[String], use_system_credentials: bool,
        token: Option<&str>,
    ) -> Result<String, SystemGitError> {
        let token = token.filter(|t| !t.is_empty());
        let existing_config_entries = std::env::var("GIT_CONFIG_COUNT")
            .ok()
            .and_then(|count| count.trim().parse().ok())
            .unwrap_or(0);

        let output = system_git_command(
            repo_url,
            path,
            use_system_credentials,
            token,
            existing_config_entries,
        )
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                SystemGitError::Unavailable(e.to_string())
            } else {
                SystemGitError::Failed(format!("Failed to execute git command: {e}"))
            }
        })?;

        if output.status.success() {
            info!("Successfully cloned repository using system git");
            Self::read_config_files(path, config_paths).map_err(SystemGitError::Read)
        } else {
            let error_msg = redact_token(&String::from_utf8_lossy(&output.stderr), token);
            error!("System git clone failed: {error_msg}");
            Err(SystemGitError::Failed(error_msg))
        }
    }

    fn read_config_files(temp_dir: &Path, config_paths: &[String]) -> GitHubResult<String> {
        let canonical_root = temp_dir
            .canonicalize()
            .map_err(|e| format!("Failed to resolve clone directory: {e}"))?;

        let mut merged_configs: Vec<serde_json::Value> = Vec::new();

        for config_path in config_paths {
            let full_path = Self::resolve_config_path(&canonical_root, config_path)?;

            let content = std::fs::read_to_string(&full_path).map_err(|e| {
                format!("Failed to read config file at {}: {e}", full_path.display())
            })?;

            let value: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
                format!(
                    "Failed to parse config file at {}: {e}",
                    full_path.display()
                )
            })?;

            match value {
                serde_json::Value::Array(items) => merged_configs.extend(items),
                other => merged_configs.push(other),
            }
        }

        serde_json::to_string(&merged_configs)
            .map_err(|e| format!("Failed to merge config files: {e}"))
    }

    fn resolve_config_path(
        canonical_root: &Path, config_path: &str,
    ) -> GitHubResult<std::path::PathBuf> {
        use std::path::Component;

        if config_path.trim().is_empty() {
            return Err("Config path must not be empty".to_string());
        }

        let candidate = Path::new(config_path);

        if candidate.is_absolute()
            || candidate
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
        {
            return Err(format!("Invalid config path: {config_path}"));
        }

        let joined = canonical_root.join(candidate);

        let canonical_file = joined
            .canonicalize()
            .map_err(|e| format!("Failed to read config file at {}: {e}", joined.display()))?;

        if !canonical_file.starts_with(canonical_root) {
            return Err(format!(
                "Config path escapes repository directory: {config_path}"
            ));
        }

        Ok(canonical_file)
    }

    pub async fn process_config_content(
        config_content: &str, flush_existing: bool, mode: DatabaseMode,
    ) -> GitHubResult<()> {
        let configs = crate::utils::config::parse_import_configs(config_content)
            .map_err(|e| format!("Failed to import configs: {e}"))?;

        if flush_existing && mode == DatabaseMode::File {
            info!("Flushing existing configurations before import");
            clear_existing_configs_with_mode(mode).await?;
        }

        let context = DatabaseManager::get_context(mode).await?;

        info!("Importing configurations using incremental merge");

        crate::utils::config::upsert_configs_with_pool_and_mode(configs, &context.pool, mode)
            .await
            .map_err(|e| format!("Failed to import configs: {e}"))?;

        info!("Configuration import completed successfully");
        Ok(())
    }
}

const SYSTEM_GIT_TOKEN_USER: &str = "git";

/// Builds the `git clone` fallback. The token only travels in environment
/// variables (`GIT_CONFIG_*`), never in argv or the URL. A missing credential
/// fails the sync instead of blocking it: `GIT_TERMINAL_PROMPT=0` stops the
/// terminal prompt, and an empty `GIT_ASKPASS` stops git from running an
/// askpass GUI found in `core.askPass` or `SSH_ASKPASS`.
fn system_git_command(
    repo_url: &str, path: &Path, use_system_credentials: bool, token: Option<&str>,
    existing_config_entries: usize,
) -> Command {
    let mut command = Command::new("git");
    command
        .args([
            "clone",
            "--depth=1",
            "--single-branch",
            "--no-tags",
            "--filter=blob:none",
            "--recurse-submodules=no",
        ])
        .arg(repo_url)
        .arg(path)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_ASKPASS", "");

    let settings = system_git_settings(repo_url, use_system_credentials, token);

    if !settings.is_empty() {
        command.env(
            "GIT_CONFIG_COUNT",
            (existing_config_entries + settings.len()).to_string(),
        );

        for (offset, (key, value)) in settings.iter().enumerate() {
            let index = existing_config_entries + offset;
            command
                .env(format!("GIT_CONFIG_KEY_{index}"), key)
                .env(format!("GIT_CONFIG_VALUE_{index}"), value);
        }
    }

    command
}

/// Mirrors the git2 path: the token is only for HTTPS (or plain HTTP to the
/// local machine), and stored credential helpers are only consulted when
/// the user chose system credentials.
fn system_git_settings(
    repo_url: &str, use_system_credentials: bool, token: Option<&str>,
) -> Vec<(String, String)> {
    let mut settings = Vec::new();

    if !use_system_credentials {
        settings.push(("credential.helper".to_owned(), String::new()));
    }

    if let Some(token) = token
        && let Some(origin) = token_origin(repo_url)
    {
        settings.push((
            format!("http.{origin}/.extraHeader"),
            format!("Authorization: {}", authorization_value(token)),
        ));
    }

    settings
}

/// The scheme and host the token may be sent to. Scoping the header to it
/// keeps it from following a redirect to another host.
fn token_origin(repo_url: &str) -> Option<String> {
    let (scheme, rest) = repo_url.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host_port = authority.rsplit('@').next()?;
    let host = match host_port.strip_prefix('[') {
        Some(ipv6) => ipv6.split(']').next()?,
        None => host_port.split(':').next()?,
    };
    let scheme = scheme.to_ascii_lowercase();

    let allowed = match scheme.as_str() {
        "https" => !host.is_empty(),
        "http" => matches!(host, "localhost" | "127.0.0.1" | "::1"),
        _ => false,
    };

    allowed.then(|| format!("{scheme}://{host_port}"))
}

fn authorization_value(token: &str) -> String {
    format!(
        "Basic {}",
        STANDARD.encode(format!("{SYSTEM_GIT_TOKEN_USER}:{token}"))
    )
}

#[derive(Debug)]
enum SystemGitError {
    Unavailable(String),
    Failed(String),
    Read(String),
}

fn is_git2_auth_error(error: &git2::Error) -> bool {
    let message = error.message();
    matches!(
        error.class(),
        git2::ErrorClass::Http | git2::ErrorClass::Ssh
    ) && error.code() == git2::ErrorCode::Auth
        || message.starts_with("Authentication failed")
        || message == "No authentication method configured"
        || message == "No valid credentials found"
}

fn is_stderr_auth_error(stderr: &str) -> bool {
    let lower = stderr.to_lowercase();
    lower.contains("authentication failed")
        || lower.contains("could not read username")
        || lower.contains("could not read password")
        || stderr.contains("401")
        || stderr.contains("403")
}

fn clone_failure_message(
    git2_message: &str, git2_auth: bool, fallback: &SystemGitError, token: Option<&str>,
) -> String {
    const CREDENTIAL_ADVICE: &str = " Please check your credentials and repository URL.";

    let git2_message = redact_token(git2_message, token);
    match fallback {
        SystemGitError::Read(message) => message.clone(),
        SystemGitError::Unavailable(reason) => {
            let advice = if git2_auth { CREDENTIAL_ADVICE } else { "" };
            format!(
                "Failed to clone repository: {git2_message}. System git is unavailable as a \
                 fallback ({reason}).{advice}"
            )
        }
        SystemGitError::Failed(stderr) => {
            let advice = if git2_auth || is_stderr_auth_error(stderr) {
                CREDENTIAL_ADVICE
            } else {
                ""
            };
            format!(
                "Failed to clone repository.{advice} Error: {git2_message}. System git fallback \
                 also failed: {}",
                stderr.trim()
            )
        }
    }
}

fn redact_token(text: &str, token: Option<&str>) -> String {
    let Some(token) = token else {
        return text.to_owned();
    };
    let encoded = STANDARD.encode(format!("{SYSTEM_GIT_TOKEN_USER}:{token}"));

    text.replace(&encoded, "[redacted]")
        .replace(token, "[redacted]")
}

/// Clear existing configurations from database
async fn clear_existing_configs_with_pool(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let mut conn = pool.acquire().await?;
    sqlx::query("DELETE FROM configs")
        .execute(&mut *conn)
        .await?;
    Ok(())
}

pub async fn clear_existing_configs() -> Result<(), sqlx::Error> {
    let pool = get_db_pool()
        .await
        .map_err(|e| sqlx::Error::Configuration(format!("DB Pool error: {e}").into()))?;
    clear_existing_configs_with_pool(&pool).await
}

pub async fn clear_existing_configs_with_mode(mode: DatabaseMode) -> Result<(), String> {
    use crate::utils::db_mode::DatabaseManager;

    let context = DatabaseManager::get_context(mode).await?;
    clear_existing_configs_with_pool(&context.pool)
        .await
        .map_err(|e| e.to_string())
}

pub fn build_github_api_url(repo_url: &str, config_path: &str) -> Result<String, String> {
    let base_api_url = "https://api.github.com/repos";

    if !repo_url.contains("github.com") {
        return Err("URL must be a GitHub repository URL".to_string());
    }

    let repo_url_without_query = repo_url.split('?').next().unwrap_or(repo_url);

    let relevant_part = repo_url_without_query
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("github.com/");

    let url_parts: Vec<&str> = relevant_part
        .split('/')
        .filter(|&x| !x.is_empty())
        .collect();

    if url_parts.len() < 2 {
        return Err("Invalid GitHub repository URL format after parsing".to_string());
    }

    let owner = url_parts[0];
    let repo = url_parts[1];

    Ok(format!(
        "{base_api_url}/{owner}/{repo}/contents/{config_path}"
    ))
}

#[cfg(test)]
mod tests {
    use std::io::{
        Read,
        Write,
    };
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::{
        AtomicBool,
        Ordering,
    };
    use std::time::Duration;

    use parking_lot::Mutex;
    use sqlx::SqlitePool;

    use super::*;
    use crate::db::create_db_table;
    use crate::models::config_model::Config;
    use crate::utils::config::insert_config_with_pool;

    async fn setup_test_db() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("Failed to connect to in-memory database");
        create_db_table(&pool)
            .await
            .expect("Failed to create tables");
        crate::utils::migration::migrate_configs(Some(&pool))
            .await
            .expect("Failed to run migrations");
        pool
    }

    #[tokio::test]
    async fn test_clear_existing_configs() {
        let pool = setup_test_db().await;

        insert_config_with_pool(Config::default(), &pool)
            .await
            .unwrap();
        insert_config_with_pool(Config::default(), &pool)
            .await
            .unwrap();

        let configs_before = crate::utils::config::read_configs_with_pool(&pool)
            .await
            .unwrap();
        assert_eq!(configs_before.len(), 2);

        clear_existing_configs_with_pool(&pool).await.unwrap();

        let configs_after = crate::utils::config::read_configs_with_pool(&pool)
            .await
            .unwrap();
        assert!(configs_after.is_empty());
    }

    #[tokio::test]
    async fn test_clear_existing_configs_public_function() {
        let pool = setup_test_db().await;

        insert_config_with_pool(Config::default(), &pool)
            .await
            .unwrap();

        let configs_before = crate::utils::config::read_configs_with_pool(&pool)
            .await
            .unwrap();
        assert_eq!(configs_before.len(), 1);

        let result = clear_existing_configs_with_pool(&pool).await;
        assert!(result.is_ok());

        let configs_after = crate::utils::config::read_configs_with_pool(&pool)
            .await
            .unwrap();
        assert!(configs_after.is_empty());
    }

    #[tokio::test]
    async fn test_clear_existing_configs_error_handling() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        pool.close().await;

        let result = clear_existing_configs_with_pool(&pool).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_flush_import_keeps_saved_configs_until_content_is_valid() {
        use crate::test_utils::{
            EnvVarGuard,
            test_db,
        };
        use crate::utils::config::{
            insert_config_with_mode,
            read_configs_with_mode,
        };

        let _db = test_db().await;
        let dir = tempfile::tempdir().unwrap();
        let _env = EnvVarGuard::set("KFTRAY_CONFIG", dir.path().to_str().unwrap());
        crate::utils::db::init().await.unwrap();
        let ctx = DatabaseManager::get_context(DatabaseMode::File)
            .await
            .unwrap();
        create_db_table(&ctx.pool).await.unwrap();
        crate::utils::migration::migrate_configs(Some(&ctx.pool))
            .await
            .unwrap();

        insert_config_with_mode(
            Config {
                alias: Some("keep-me".to_string()),
                service: Some("api".to_string()),
                namespace: "default".to_string(),
                ..Default::default()
            },
            DatabaseMode::File,
        )
        .await
        .unwrap();

        let aliases = || async {
            read_configs_with_mode(DatabaseMode::File)
                .await
                .unwrap()
                .into_iter()
                .map(|c| c.alias.unwrap_or_default())
                .collect::<Vec<_>>()
        };

        let ok = r#"{"alias":"ok","workload_type":"service","service":"api","namespace":"default","protocol":"tcp"}"#;
        let bad = r#"{"alias":"bad","workload_type":"service","service":"api","namespace":""}"#;
        for invalid in [
            "not json".to_string(),
            format!("[{bad}]"),
            format!("[{ok},{bad}]"),
        ] {
            let result =
                GitHubRepository::process_config_content(&invalid, true, DatabaseMode::File).await;
            assert!(result.is_err(), "{invalid} should be rejected");
            assert_eq!(aliases().await, vec!["keep-me"], "after {invalid}");
        }

        GitHubRepository::process_config_content(&format!("[{ok}]"), true, DatabaseMode::File)
            .await
            .unwrap();
        assert_eq!(aliases().await, vec!["ok"]);
    }

    #[test]
    fn test_read_config_files_merges_multiple_arrays() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        std::fs::write(
            temp_dir.path().join("configs-a.json"),
            r#"[{"id": 1}, {"id": 2}]"#,
        )
        .unwrap();
        std::fs::write(temp_dir.path().join("configs-b.json"), r#"[{"id": 3}]"#).unwrap();

        let merged = GitHubRepository::read_config_files(
            temp_dir.path(),
            &["configs-a.json".to_string(), "configs-b.json".to_string()],
        )
        .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(parsed.as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_read_config_files_merges_single_objects() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        std::fs::write(temp_dir.path().join("a.json"), r#"{"id": 1}"#).unwrap();
        std::fs::write(temp_dir.path().join("b.json"), r#"{"id": 2}"#).unwrap();

        let merged = GitHubRepository::read_config_files(
            temp_dir.path(),
            &["a.json".to_string(), "b.json".to_string()],
        )
        .unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(parsed.as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_read_config_files_missing_file_errors() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        let result =
            GitHubRepository::read_config_files(temp_dir.path(), &["missing.json".to_string()]);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to read config file"));
    }

    #[test]
    fn test_read_config_files_rejects_parent_dir_traversal() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        let result =
            GitHubRepository::read_config_files(temp_dir.path(), &["../config.json".to_string()]);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid config path"));
    }

    #[test]
    fn test_read_config_files_rejects_absolute_path() {
        let temp_dir = tempfile::TempDir::new().unwrap();

        let absolute_path = if cfg!(windows) {
            "C:\\config.json".to_string()
        } else {
            "/etc/passwd".to_string()
        };

        let result = GitHubRepository::read_config_files(temp_dir.path(), &[absolute_path]);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid config path"));
    }

    #[cfg(unix)]
    #[test]
    fn test_read_config_files_rejects_symlink_outside_repo() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let outside_dir = tempfile::TempDir::new().unwrap();

        let outside_file = outside_dir.path().join("secret.json");
        std::fs::write(&outside_file, r#"{"id": "secret"}"#).unwrap();

        let link_path = temp_dir.path().join("config.json");
        std::os::unix::fs::symlink(&outside_file, &link_path).unwrap();

        let result =
            GitHubRepository::read_config_files(temp_dir.path(), &["config.json".to_string()]);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("escapes repository directory"));
    }

    #[test]
    fn test_build_github_api_url_edge_cases() {
        let url1 =
            build_github_api_url("https://github.com///owner///repo///", "config.json").unwrap();
        assert_eq!(
            url1,
            "https://api.github.com/repos/owner/repo/contents/config.json"
        );

        let url2 =
            build_github_api_url("https://github.com/owner/repo?ref=main", "config.json").unwrap();
        assert_eq!(
            url2,
            "https://api.github.com/repos/owner/repo/contents/config.json"
        );

        let url3 = build_github_api_url("https://github.com/owner%20name/repo-name", "config.json")
            .unwrap();
        assert_eq!(
            url3,
            "https://api.github.com/repos/owner%20name/repo-name/contents/config.json"
        );
    }

    #[test]
    fn test_build_github_api_url_valid() {
        let url =
            build_github_api_url("https://github.com/owner/repo", "path/to/config.json").unwrap();
        assert_eq!(
            url,
            "https://api.github.com/repos/owner/repo/contents/path/to/config.json"
        );

        let url_no_https = build_github_api_url("github.com/owner/repo", "config.json").unwrap();
        assert_eq!(
            url_no_https,
            "https://api.github.com/repos/owner/repo/contents/config.json"
        );

        let url_trailing_slash =
            build_github_api_url("https://github.com/owner/repo/", "file").unwrap();
        assert_eq!(
            url_trailing_slash,
            "https://api.github.com/repos/owner/repo/contents/file"
        );
    }

    #[test]
    fn test_build_github_api_url_with_http_prefix() {
        let url = build_github_api_url("http://github.com/owner/repo", "config.json").unwrap();
        assert_eq!(
            url,
            "https://api.github.com/repos/owner/repo/contents/config.json"
        );
    }

    #[test]
    fn test_build_github_api_url_with_complex_paths() {
        let url = build_github_api_url(
            "https://github.com/owner/repo/tree/main/some/folder",
            "config.json",
        )
        .unwrap();
        assert_eq!(
            url,
            "https://api.github.com/repos/owner/repo/contents/config.json"
        );
    }

    #[test]
    fn test_build_github_api_url_invalid() {
        // Test completely invalid URL format
        let result_invalid_url = build_github_api_url("invalid-url", "path/to/config.json");
        assert!(result_invalid_url.is_err());
        // Should fail the github.com check first
        assert!(
            result_invalid_url
                .unwrap_err()
                .contains("URL must be a GitHub repository URL")
        );

        // Test non-GitHub URL (should fail the github.com check)
        let result_not_github =
            build_github_api_url("https://gitlab.com/owner/repo", "config.json");
        assert!(result_not_github.is_err());
        assert!(
            result_not_github
                .unwrap_err()
                .contains("URL must be a GitHub repository URL")
        );

        // Test URL missing repo part (should fail length check after parsing)
        let result_too_short = build_github_api_url("github.com/owner", "config.json");
        assert!(result_too_short.is_err());
        assert!(
            result_too_short
                .unwrap_err()
                .contains("Invalid GitHub repository URL format after parsing")
        );

        // Test empty URL (should fail github.com check)
        let result_empty = build_github_api_url("", "config.json");
        assert!(result_empty.is_err());
        assert!(
            result_empty
                .unwrap_err()
                .contains("URL must be a GitHub repository URL")
        );
    }

    const TOKEN: &str = "ghp_sup3rS3cretT0ken";

    struct HttpStub {
        port: u16,
        requests: Arc<Mutex<Vec<String>>>,
        stop: Arc<AtomicBool>,
        handle: std::thread::JoinHandle<()>,
    }

    impl HttpStub {
        fn start() -> Self {
            Self::answering(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
        }

        fn answering(response: &'static [u8]) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let port = listener.local_addr().unwrap().port();
            let requests = Arc::new(Mutex::new(Vec::new()));
            let stop = Arc::new(AtomicBool::new(false));

            let handle = {
                let requests = requests.clone();
                let stop = stop.clone();
                std::thread::spawn(move || {
                    while !stop.load(Ordering::SeqCst) {
                        let Ok((mut stream, _)) = listener.accept() else {
                            std::thread::sleep(Duration::from_millis(10));
                            continue;
                        };
                        stream.set_nonblocking(false).unwrap();
                        stream
                            .set_read_timeout(Some(Duration::from_secs(5)))
                            .unwrap();
                        let mut request = Vec::new();
                        let mut buf = [0u8; 1024];
                        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                            match stream.read(&mut buf) {
                                Ok(0) | Err(_) => break,
                                Ok(n) => request.extend_from_slice(&buf[..n]),
                            }
                        }
                        requests
                            .lock()
                            .push(String::from_utf8_lossy(&request).into_owned());
                        let _ = stream.write_all(response);
                    }
                })
            };

            Self {
                port,
                requests,
                stop,
                handle,
            }
        }

        fn url(&self) -> String {
            format!("http://127.0.0.1:{}/org/repo.git", self.port)
        }

        fn finish(self) -> Vec<String> {
            self.stop.store(true, Ordering::SeqCst);
            self.handle.join().unwrap();
            self.requests.lock().clone()
        }
    }

    fn expected_authorization(token: &str) -> String {
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("git:{token}"))
        )
    }

    fn authorization(request: &str) -> Option<&str> {
        request.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("authorization")
                .then(|| value.trim())
        })
    }

    #[test]
    fn fallback_clone_authenticates_with_the_saved_token_and_keeps_it_out_of_errors() {
        let stub = HttpStub::start();
        let url = stub.url();
        let dest = tempfile::TempDir::new().unwrap();

        let err = GitHubRepository::try_clone_with_system_git(
            &url,
            dest.path(),
            &["config.json".to_string()],
            false,
            Some(TOKEN),
        )
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
        .unwrap_err();

        let requests = stub.finish();
        assert!(!requests.is_empty(), "git never reached the stub");
        for request in &requests {
            assert_eq!(
                authorization(request),
                Some(expected_authorization(TOKEN).as_str()),
                "request: {request}"
            );
        }
        assert!(err.contains("not found"), "stderr cause was lost: {err}");
        assert!(!err.contains(TOKEN), "token leaked into error: {err}");
    }

    #[test]
    fn fallback_clone_without_a_token_sends_no_authorization() {
        let stub = HttpStub::start();
        let url = stub.url();
        let dest = tempfile::TempDir::new().unwrap();

        let err = GitHubRepository::try_clone_with_system_git(
            &url,
            dest.path(),
            &["config.json".to_string()],
            true,
            None,
        )
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
        .unwrap_err();

        let requests = stub.finish();
        assert!(!requests.is_empty(), "git never reached the stub");
        for request in &requests {
            assert_eq!(authorization(request), None, "request: {request}");
        }
        assert!(err.contains("not found"), "stderr cause was lost: {err}");
    }

    #[cfg(unix)]
    #[test]
    fn fallback_clone_fails_without_running_an_inherited_askpass_helper() {
        use std::os::unix::fs::PermissionsExt;

        let stub = HttpStub::answering(
            b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"repo\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        let url = stub.url();
        let dest = tempfile::TempDir::new().unwrap();
        let scratch = tempfile::TempDir::new().unwrap();
        let marker = scratch.path().join("asked");
        let helper = scratch.path().join("askpass.sh");
        std::fs::write(
            &helper,
            format!("#!/bin/sh\ntouch '{}'\necho x\n", marker.display()),
        )
        .unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();

        // A desktop session's askpass GUI, as git would find it.
        let mut command = system_git_command(&url, dest.path(), false, None, 0);
        command.env("SSH_ASKPASS", &helper);
        let output = command.output().unwrap();
        stub.finish();

        assert!(!output.status.success());
        assert!(!marker.exists(), "git ran the inherited askpass helper");
    }

    fn env_of(command: &Command) -> std::collections::HashMap<String, String> {
        command
            .get_envs()
            .filter_map(|(k, v)| {
                Some((
                    k.to_string_lossy().into_owned(),
                    v?.to_string_lossy().into_owned(),
                ))
            })
            .collect()
    }

    #[test]
    fn system_git_command_passes_the_token_only_through_the_environment() {
        let url = "https://github.com/org/repo.git";
        let command = system_git_command(url, Path::new("/tmp/dest"), false, Some(TOKEN), 0);

        let argv: Vec<String> = command
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(argv.iter().all(|a| !a.contains(TOKEN)), "argv: {argv:?}");
        assert!(argv.contains(&url.to_string()));

        let env = env_of(&command);
        assert_eq!(env["GIT_TERMINAL_PROMPT"], "0");
        assert_eq!(env["GIT_CONFIG_COUNT"], "2");
        assert_eq!(env["GIT_CONFIG_KEY_0"], "credential.helper");
        assert_eq!(env["GIT_CONFIG_VALUE_0"], "");
        assert_eq!(
            env["GIT_CONFIG_KEY_1"],
            "http.https://github.com/.extraHeader"
        );
        assert_eq!(
            env["GIT_CONFIG_VALUE_1"],
            format!("Authorization: {}", expected_authorization(TOKEN))
        );
    }

    #[test]
    fn system_git_command_appends_to_config_entries_already_in_the_environment() {
        let command = system_git_command(
            "https://github.com/org/repo.git",
            Path::new("/tmp/dest"),
            true,
            Some(TOKEN),
            3,
        );

        let env = env_of(&command);
        assert_eq!(env["GIT_CONFIG_COUNT"], "4");
        assert!(!env.contains_key("GIT_CONFIG_KEY_0"));
        assert_eq!(
            env["GIT_CONFIG_KEY_3"],
            "http.https://github.com/.extraHeader"
        );
    }

    #[test]
    fn system_git_command_keeps_system_credentials_when_no_token_is_saved() {
        let command = system_git_command(
            "https://github.com/org/repo.git",
            Path::new("/tmp/dest"),
            true,
            None,
            0,
        );

        let env = env_of(&command);
        assert_eq!(env["GIT_TERMINAL_PROMPT"], "0");
        assert!(!env.contains_key("GIT_CONFIG_COUNT"));
    }

    #[test]
    fn token_origin_only_allows_https_and_local_http() {
        assert_eq!(
            token_origin("https://github.com/org/repo.git").as_deref(),
            Some("https://github.com")
        );
        assert_eq!(
            token_origin("HTTPS://user@git.example.com:8443/org/repo").as_deref(),
            Some("https://git.example.com:8443")
        );
        assert_eq!(
            token_origin("http://127.0.0.1:9000/repo.git").as_deref(),
            Some("http://127.0.0.1:9000")
        );
        assert_eq!(
            token_origin("http://[::1]:9000/repo.git").as_deref(),
            Some("http://[::1]:9000")
        );
        assert_eq!(token_origin("http://github.com/org/repo.git"), None);
        assert_eq!(token_origin("git@github.com:org/repo.git"), None);
        assert_eq!(token_origin("ssh://git@github.com/org/repo.git"), None);
        assert_eq!(token_origin("https:///repo.git"), None);
    }

    #[test]
    fn redact_token_removes_the_token_and_its_basic_auth_form() {
        let stderr = format!(
            "fatal: bad header Authorization: {} for {TOKEN}",
            expected_authorization(TOKEN)
        );

        let redacted = redact_token(&stderr, Some(TOKEN));

        assert!(!redacted.contains(TOKEN), "{redacted}");
        assert!(
            !redacted.contains(&expected_authorization(TOKEN)),
            "{redacted}"
        );
        assert!(redacted.starts_with("fatal: bad header"));
    }

    #[test]
    fn failed_clone_reports_both_causes_without_credential_advice_for_not_found() {
        let stub = HttpStub::start();
        let url = stub.url();

        let err = GitHubRepository::clone_and_read_config(
            &url,
            &["config.json".to_string()],
            false,
            Some(TOKEN.to_string()),
        )
        .unwrap_err();

        stub.finish();
        assert!(err.contains("404"), "git2 cause was lost: {err}");
        assert!(
            err.contains("not found"),
            "system git cause was lost: {err}"
        );
        assert!(!err.contains("credentials"), "wrong advice: {err}");
        assert!(!err.contains(TOKEN), "token leaked into error: {err}");
    }

    #[test]
    fn failed_clone_suggests_checking_credentials_when_unauthorized() {
        let stub = HttpStub::answering(
            b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"repo\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        let url = stub.url();

        let err = GitHubRepository::clone_and_read_config(
            &url,
            &["config.json".to_string()],
            false,
            Some(TOKEN.to_string()),
        )
        .unwrap_err();

        stub.finish();
        assert!(err.contains("check your credentials"), "no advice: {err}");
        assert!(!err.contains(TOKEN), "token leaked into error: {err}");
    }

    #[test]
    fn failed_clone_redacts_the_token_from_the_first_error() {
        let message = clone_failure_message(
            &format!("cannot reach https://x:{TOKEN}@host/repo.git"),
            false,
            &SystemGitError::Failed("fatal: unable to access".to_string()),
            Some(TOKEN),
        );

        assert!(!message.contains(TOKEN), "{message}");
        assert!(message.contains("cannot reach"), "{message}");
    }

    #[test]
    fn failed_clone_says_system_git_is_unavailable_and_keeps_the_first_cause() {
        let message = clone_failure_message(
            "failed to resolve address for example.invalid",
            false,
            &SystemGitError::Unavailable("No such file or directory".to_string()),
            None,
        );

        assert!(message.contains("failed to resolve address"), "{message}");
        assert!(message.contains("System git is unavailable"), "{message}");
        assert!(!message.contains("Failed to execute git"), "{message}");
        assert!(!message.contains("credentials"), "{message}");
    }
}

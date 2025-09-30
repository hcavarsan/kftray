use std::path::Path;

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
    pub config_path: String,
    pub use_system_credentials: bool,
    pub github_token: Option<String>,
    pub flush_existing: bool,
}

pub type GitHubResult<T> = Result<T, String>;

pub struct GitHubRepository;

impl GitHubRepository {
    pub async fn import_configs(config: GitHubConfig, mode: DatabaseMode) -> GitHubResult<()> {
        let config_content = Self::clone_and_read_config(
            &config.repo_url,
            &config.config_path,
            config.use_system_credentials,
            config.github_token,
        )?;

        Self::process_config_content(&config_content, config.flush_existing, mode).await
    }

    fn clone_and_read_config(
        repo_url: &str, config_path: &str, use_system_credentials: bool,
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
                Self::read_config_file(temp_dir.path(), config_path)
            }
            Err(e) => {
                warn!("Repository clone failed: {e}, trying fallback with system git command");
                Self::try_clone_with_system_git(repo_url, temp_dir.path(), config_path)
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
        repo_url: &str, path: &Path, config_path: &str,
    ) -> GitHubResult<String> {
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
            .map_err(|e| format!("Failed to execute git command: {e}"))?;

        if output.status.success() {
            info!("Successfully cloned repository using system git");
            Self::read_config_file(path, config_path)
        } else {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            error!("System git clone failed: {error_msg}");
            Err(format!(
                "Failed to clone repository. Please check your credentials and repository URL. Error: {error_msg}"
            ))
        }
    }

    fn read_config_file(temp_dir: &Path, config_path: &str) -> GitHubResult<String> {
        let config_path = Path::new(config_path);
        let full_path = temp_dir.join(config_path);

        std::fs::read_to_string(&full_path)
            .map_err(|e| format!("Failed to read config file at {}: {e}", full_path.display()))
    }

    async fn process_config_content(
        config_content: &str, flush_existing: bool, mode: DatabaseMode,
    ) -> GitHubResult<()> {
        if flush_existing && mode == DatabaseMode::File {
            info!("Flushing existing configurations before import");
            clear_existing_configs_with_mode(mode).await?;
        }

        let context = DatabaseManager::get_context(mode).await?;

        info!("Importing configurations using incremental merge");

        crate::utils::config::import_configs_with_pool_and_mode(
            config_content.to_string(),
            &context.pool,
            mode,
        )
        .await
        .map_err(|e| format!("Failed to import configs: {e}"))?;

        info!("Configuration import completed successfully");
        Ok(())
    }
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
}

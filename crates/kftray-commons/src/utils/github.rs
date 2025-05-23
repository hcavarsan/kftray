use sqlx::SqlitePool;

use crate::db::get_db_pool;

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
        assert!(result_invalid_url
            .unwrap_err()
            .contains("URL must be a GitHub repository URL"));

        // Test non-GitHub URL (should fail the github.com check)
        let result_not_github =
            build_github_api_url("https://gitlab.com/owner/repo", "config.json");
        assert!(result_not_github.is_err());
        assert!(result_not_github
            .unwrap_err()
            .contains("URL must be a GitHub repository URL"));

        // Test URL missing repo part (should fail length check after parsing)
        let result_too_short = build_github_api_url("github.com/owner", "config.json");
        assert!(result_too_short.is_err());
        assert!(result_too_short
            .unwrap_err()
            .contains("Invalid GitHub repository URL format after parsing"));

        // Test empty URL (should fail github.com check)
        let result_empty = build_github_api_url("", "config.json");
        assert!(result_empty.is_err());
        assert!(result_empty
            .unwrap_err()
            .contains("URL must be a GitHub repository URL"));
    }
}

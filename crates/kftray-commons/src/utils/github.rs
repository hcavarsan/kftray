use crate::db::get_db_pool;

pub async fn clear_existing_configs() -> Result<(), sqlx::Error> {
    let pool = get_db_pool()
        .await
        .map_err(|e| sqlx::Error::Configuration(format!("DB Pool error: {}", e).into()))?;
    let mut conn = pool.acquire().await?;

    sqlx::query("DELETE FROM configs")
        .execute(&mut *conn)
        .await?;

    Ok(())
}

pub fn build_github_api_url(repo_url: &str, config_path: &str) -> Result<String, String> {
    let base_api_url = "https://api.github.com/repos";

    let url_parts: Vec<&str> = repo_url
        .split('/')
        .filter(|&x| !x.is_empty() && x != "https:" && x != "github.com")
        .collect();

    if url_parts.len() < 2 {
        return Err("Invalid GitHub repository URL".to_string());
    }

    let owner = url_parts[0];
    let repo = url_parts[1];

    Ok(format!(
        "{}/{}/{}/contents/{}",
        base_api_url, owner, repo, config_path
    ))
}

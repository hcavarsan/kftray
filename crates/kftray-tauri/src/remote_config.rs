use rusqlite::Connection;

use crate::utils::config_dir::get_db_file_path;

//  function to clear existing configs from the database
pub fn clear_existing_configs() -> Result<(), rusqlite::Error> {
    let db_dir = get_db_file_path().map_err(|e| {
        rusqlite::Error::InvalidPath(format!("Failed to get DB path: {}", e).into())
    })?;

    let conn = Connection::open(db_dir)?;

    conn.execute("DELETE FROM configs", ())?;

    Ok(())
}

//  function to build the github api url
pub fn build_github_api_url(repo_url: &str, config_path: &str) -> String {
    let base_api_url = "https://api.github.com/repos";

    let url_parts: Vec<&str> = repo_url
        .split('/')
        .filter(|&x| !x.is_empty() && x != "https:" && x != "github.com")
        .collect();

    if url_parts.len() < 2 {
        return "".to_string();
    }

    let owner = url_parts[0];

    let repo = url_parts[1];

    format!(
        "{}/{}/{}/contents/{}",
        base_api_url, owner, repo, config_path
    )
}

#[cfg(test)]

mod tests {

    use super::*;

    #[test]

    fn test_build_github_api_url() {
        let repo_url = "https://github.com/exampleUser/exampleRepo";

        let config_path = "path/to/config.json";

        let expected_url =
            "https://api.github.com/repos/exampleUser/exampleRepo/contents/path/to/config.json";

        assert_eq!(build_github_api_url(repo_url, config_path), expected_url);
    }
}

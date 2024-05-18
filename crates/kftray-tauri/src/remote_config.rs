use rusqlite::Connection;

//  function to clear existing configs from the database
pub fn clear_existing_configs() -> Result<(), rusqlite::Error> {
    let home_dir = dirs::home_dir().expect("Unable to find the home directory");

    let db_dir = home_dir.join(".kftray/configs.db");

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

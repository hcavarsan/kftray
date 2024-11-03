//! GitHub client for configuration management
//!
//! This module provides functionality for interacting with GitHub repositories,
//! including syncing configurations from a specified path in a repository.

use base64::{
    engine::general_purpose,
    Engine as _,
};
use reqwest::header::{
    HeaderMap,
    HeaderValue,
    AUTHORIZATION,
    USER_AGENT,
};

use crate::config::Config;
use crate::db::operations::Database;
use crate::error::{
    Error,
    Result,
};

#[derive(Debug)]
pub struct GithubClient {
    pub db: Database,
    client: reqwest::Client,
    default_headers: HeaderMap,
}

impl GithubClient {
    pub fn new(db: Database) -> Self {
        let mut default_headers = HeaderMap::new();
        default_headers.insert(USER_AGENT, HeaderValue::from_static("kftray-client"));

        Self {
            db,
            client: reqwest::Client::new(),
            default_headers,
        }
    }

    pub async fn sync_configs(
        &self, repo_url: &str, config_path: &str, is_private: bool, token: Option<String>,
    ) -> Result<()> {
        let url = build_github_api_url(repo_url, config_path)?;
        let mut headers = self.default_headers.clone();

        if is_private {
            let token =
                token.ok_or_else(|| Error::github("Token is required for private repositories"))?;
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("token {}", token))
                    .map_err(|e| Error::github(format!("Invalid token format: {}", e)))?,
            );
        }

        let response = self
            .client
            .get(&url)
            .headers(headers)
            .send()
            .await
            .map_err(|e| Error::github(format!("Failed to send request: {}", e)))?;

        let response = response
            .error_for_status()
            .map_err(|e| Error::github(format!("GitHub API request failed: {}", e)))?;

        let response_text = response
            .text()
            .await
            .map_err(|e| Error::github(format!("Failed to get response text: {}", e)))?;

        let json_response: serde_json::Value = serde_json::from_str(&response_text)
            .map_err(|e| Error::github(format!("Failed to parse JSON response: {}", e)))?;

        let content = json_response["content"]
            .as_str()
            .ok_or_else(|| Error::github("Content field not found in response"))?;

        let decoded_content = decode_github_content(content)?;
        let configs: Vec<Config> = serde_json::from_str(&decoded_content)?;

        for config in configs {
            self.db.save_config(&config).await?;
        }

        Ok(())
    }
}

fn build_github_api_url(repo_url: &str, config_path: &str) -> Result<String> {
    let repo_url = repo_url.trim_end_matches(".git");
    let parts: Vec<&str> = repo_url.split('/').collect();

    if parts.len() < 2 {
        return Err(Error::github("Invalid repository URL format"));
    }

    let owner = parts[parts.len() - 2];
    let repo = parts[parts.len() - 1];

    Ok(format!(
        "https://api.github.com/repos/{}/{}/contents/{}",
        owner, repo, config_path
    ))
}

fn decode_github_content(content: &str) -> Result<String> {
    let cleaned_content = content.replace(['\n', '\r'], "");

    let decoded = general_purpose::STANDARD
        .decode(&cleaned_content)
        .map_err(|e| Error::github(format!("Failed to decode base64 content: {}", e)))?;

    String::from_utf8(decoded).map_err(|e| {
        Error::github(format!(
            "Failed to convert decoded content to string: {}",
            e
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_github_url_building() {
        let test_cases = vec![
            ("https://github.com/owner/repo.git", "configs/test.json"),
            ("https://github.com/owner/repo", "configs/test.json"),
            ("git@github.com:owner/repo.git", "configs/test.json"),
        ];

        for (repo_url, config_path) in test_cases {
            let result = build_github_api_url(repo_url, config_path);
            assert!(result.is_ok());
            assert_eq!(
                result.unwrap(),
                "https://api.github.com/repos/owner/repo/contents/configs/test.json"
            );
        }
    }

    #[test]
    fn test_decode_github_content() {
        let content = "SGVsbG8sIFdvcmxkIQ=="; // "Hello, World!" in base64
        let result = decode_github_content(content);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Hello, World!");
    }
}

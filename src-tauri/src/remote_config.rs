extern crate base64;
use crate::config::import_configs;

use kubeforward::port_forward::Config;
use reqwest::header::{AUTHORIZATION, USER_AGENT};
use rusqlite::Connection;

#[tauri::command]
pub async fn import_configs_from_github(
    repo_url: String,
    config_path: String,
    is_private: bool,
    token: Option<String>,
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let url = build_github_api_url(&repo_url, &config_path);
    let mut request_builder = client.get(url);

    if is_private {
        let token = token.ok_or("Token is required for private repositories")?;
        request_builder = request_builder.header(AUTHORIZATION, format!("token {}", token));
    }

    let response = request_builder
        .header(USER_AGENT, "request")
        .send()
        .await
        .map_err(|e| format!("Failed to send request: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Request failed: {}", e))?;

    let json_content = response.text().await.map_err(|e| e.to_string())?;

    let json_obj: serde_json::Value = serde_json::from_str(&json_content)
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let base64_content = json_obj["content"]
        .as_str()
        .ok_or("Failed to extract content from response")?
        .trim();
    println!("base64_content: {}", base64_content);

    let base64_content_cleaned = base64_content.replace('\n', "").replace('\r', "");

    let decoded_content = base64::decode(&base64_content_cleaned)
        .map_err(|e| format!("Failed to decode base64 content: {}", e))?;

    let decoded_str = String::from_utf8(decoded_content)
        .map_err(|e| format!("Failed to convert decoded content to string: {}", e))?;

    println!("decoded_str: {}", decoded_str);
    let configs: Vec<Config> = serde_json::from_str(&decoded_str)
        .map_err(|e| format!("Failed to parse configs: {}", e))?;

    clear_existing_configs().map_err(|e| e.to_string())?;
    for config in configs {
        let config_json = serde_json::to_string(&config)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;
        import_configs(config_json).await?;
    }

    Ok(())
}

fn clear_existing_configs() -> Result<(), rusqlite::Error> {
    let home_dir = dirs::home_dir().expect("Unable to find the home directory");
    let db_dir = home_dir.join(".kftray/configs.db");

    let conn = Connection::open(db_dir)?;
    conn.execute("DELETE FROM configs", ())?;

    Ok(())
}

fn build_github_api_url(repo_url: &str, config_path: &str) -> String {
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

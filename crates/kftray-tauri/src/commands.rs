use std::sync::atomic::Ordering;

use base64::{
    engine::general_purpose,
    Engine as _,
};
use reqwest::header::{
    AUTHORIZATION,
    USER_AGENT,
};
use tauri::State;

use crate::utils::config_dir::get_log_folder_path;
use crate::{
    config::{
        import_configs,
        migrate_configs,
    },
    models::{
        config::Config,
        window::SaveDialogState,
    },
    remote_config::{
        build_github_api_url,
        clear_existing_configs,
    },
};

//  command to save the dialog state when is open
#[tauri::command]

pub fn open_save_dialog(state: State<SaveDialogState>) {
    state.is_open.store(true, Ordering::SeqCst);
}

// command to save the dialog state when is closed
#[tauri::command]

pub fn close_save_dialog(state: State<SaveDialogState>) {
    state.is_open.store(false, Ordering::SeqCst);
}

// command to import configs from github
#[tauri::command]

pub async fn import_configs_from_github(
    repo_url: String, config_path: String, is_private: bool, flush: bool, token: Option<String>,
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

    let base64_content_cleaned = base64_content.replace(['\n', '\r'], "");

    let decoded_content = general_purpose::STANDARD
        .decode(&base64_content_cleaned)
        .map_err(|e| format!("Failed to decode base64 content: {}", e))?;

    let decoded_str = String::from_utf8(decoded_content)
        .map_err(|e| format!("Failed to convert decoded content to string: {}", e))?;

    println!("decoded_str: {}", decoded_str);

    let configs: Vec<Config> = serde_json::from_str(&decoded_str)
        .map_err(|e| format!("Failed to parse configs: {}", e))?;

    if flush {
        clear_existing_configs().map_err(|e| e.to_string())?;
    }

    for config in configs {
        let config_json = serde_json::to_string(&config)
            .map_err(|e| format!("Failed to serialize config: {}", e))?;

        import_configs(config_json).await?;
    }

    if let Err(e) = migrate_configs() {
        eprintln!("Error migrating configs: {}. Please check if the configurations are valid and compatible with the current system/version.", e);
    }

    Ok(())
}

#[tauri::command]
pub async fn open_log_file(log_file_path: String) -> Result<(), String> {
    use std::env;
    use std::fs;

    use open::that_in_background;

    println!("Opening log file: {}", log_file_path);

    if fs::metadata(&log_file_path).is_err() {
        return Err(format!("Log file does not exist: {}", log_file_path));
    }

    let editor = env::var("EDITOR").unwrap_or_else(|_| default_editor());

    match try_open_with_editor(&log_file_path, &editor) {
        Ok(_) => Ok(()),
        Err(err) => {
            println!(
                "Error opening with editor '{}': {}. Trying default method...",
                editor, err
            );

            match that_in_background(&log_file_path).join() {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(err)) => {
                    println!("Error opening log file with default method: {}. Trying fallback methods...", err);
                    fallback_methods(&log_file_path)
                }
                Err(err) => Err(format!("Failed to join thread: {:?}", err)),
            }
        }
    }
}

fn default_editor() -> String {
    if cfg!(target_os = "macos") {
        "open".to_string()
    } else if cfg!(target_os = "windows") {
        "notepad".to_string()
    } else {
        "nano".to_string()
    }
}

fn try_open_with_editor(log_file_path: &str, editor: &str) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::process::Command;

    use open::with_in_background;

    let editor_parts: Vec<&str> = editor.split_whitespace().collect();
    if editor_parts.len() > 1 {
        let app = editor_parts[0];
        let args: Vec<&OsStr> = editor_parts[1..].iter().map(OsStr::new).collect();
        let mut command = Command::new(app);
        command.args(&args).arg(log_file_path);
        match command.spawn() {
            Ok(mut child) => match child.wait() {
                Ok(status) if status.success() => Ok(()),
                Ok(status) => Err(format!("Editor exited with status: {}", status)),
                Err(err) => Err(format!("Failed to wait on editor process: {}", err)),
            },
            Err(err) => Err(format!("Failed to start editor: {}", err)),
        }
    } else {
        match with_in_background(log_file_path, editor).join() {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(err)) => Err(format!("Failed to open with {}: {}", editor, err)),
            Err(err) => Err(format!("Failed to join thread: {:?}", err)),
        }
    }
}

fn fallback_methods(log_file_path: &str) -> Result<(), String> {
    if cfg!(target_os = "windows") {
        try_open_with_editor(log_file_path, "notepad")
    } else if cfg!(target_os = "macos") {
        try_open_with_editor(log_file_path, "open")
            .or_else(|_| try_open_with_editor(log_file_path, "nano"))
            .or_else(|_| try_open_with_editor(log_file_path, "vim"))
    } else {
        try_open_with_editor(log_file_path, "xdg-open")
            .or_else(|_| try_open_with_editor(log_file_path, "nano"))
            .or_else(|_| try_open_with_editor(log_file_path, "vim"))
    }
}

#[tauri::command]
pub async fn clear_http_logs() -> Result<(), String> {
    use std::fs;
    use std::path::PathBuf;

    fn delete_files_in_folder(path: &PathBuf) -> Result<(), String> {
        if path.is_dir() {
            for entry in
                fs::read_dir(path).map_err(|e| format!("Failed to read directory: {}", e))?
            {
                let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
                let path = entry.path();
                if path.is_file() {
                    fs::remove_file(&path).map_err(|e| format!("Failed to delete file: {}", e))?;
                }
            }
        } else {
            return Err(format!("Path is not a directory: {}", path.display()));
        }

        Ok(())
    }

    let log_folder_path = get_log_folder_path()?;

    if !log_folder_path.exists() {
        return Err(format!(
            "Log folder does not exist: {}",
            log_folder_path.display()
        ));
    }

    delete_files_in_folder(&log_folder_path)
}

#[tauri::command]
pub async fn get_http_log_size() -> Result<u64, String> {
    use std::fs;
    use std::path::PathBuf;

    fn calculate_folder_size(path: &PathBuf) -> Result<u64, String> {
        let mut size = 0;

        if path.is_dir() {
            for entry in
                fs::read_dir(path).map_err(|e| format!("Failed to read directory: {}", e))?
            {
                let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
                let path = entry.path();
                if path.is_file() {
                    size += fs::metadata(&path)
                        .map_err(|e| format!("Failed to get file metadata: {}", e))?
                        .len();
                } else if path.is_dir() {
                    size += calculate_folder_size(&path)?;
                }
            }
        } else {
            return Err(format!("Path is not a directory: {}", path.display()));
        }

        Ok(size)
    }

    let log_folder_path = get_log_folder_path()?;

    if !log_folder_path.exists() {
        return Err(format!(
            "Log folder does not exist: {}",
            log_folder_path.display()
        ));
    }

    calculate_folder_size(&log_folder_path)
}

use kftray_commons::utils::config_dir::get_log_folder_path;
use kftray_portforward::http_logs::HttpLogState;
use log::error;
use log::info;

#[tauri::command]
pub async fn set_http_logs_cmd(
    state: tauri::State<'_, HttpLogState>, config_id: i64, enable: bool,
) -> Result<(), String> {
    state.set_http_logs(config_id, enable).await;
    Ok(())
}

#[tauri::command]
pub async fn get_http_logs_cmd(
    state: tauri::State<'_, HttpLogState>, config_id: i64,
) -> Result<bool, String> {
    let current_state = state.get_http_logs(config_id).await;
    Ok(current_state)
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

#[tauri::command]
pub async fn open_log_file(log_file_name: String) -> Result<(), String> {
    use std::env;
    use std::fs;

    use open::that_in_background;

    let log_folder_path = get_log_folder_path()?;
    let log_file_path = log_folder_path.join(log_file_name);

    if !log_file_path.exists() {
        return Err(format!(
            "Log file does not exist: {}",
            log_file_path.display()
        ));
    }

    info!("Opening log file: {}", log_file_path.display());

    if fs::metadata(&log_file_path).is_err() {
        return Err(format!(
            "Log file does not exist: {}",
            log_file_path.display()
        ));
    }

    let editor = env::var("EDITOR").unwrap_or_else(|_| default_editor());

    match try_open_with_editor(log_file_path.to_str().unwrap(), &editor) {
        Ok(_) => Ok(()),
        Err(err) => {
            error!(
                "Error opening with editor '{}': {}. Trying default method...",
                editor, err
            );

            match that_in_background(&log_file_path).join() {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(err)) => {
                    error!("Error opening log file with default method: {}. Trying fallback methods...", err);
                    fallback_methods(log_file_path.to_str().unwrap())
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

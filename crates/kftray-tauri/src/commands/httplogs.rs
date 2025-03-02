use std::fs;
use std::path::{
    Path,
    PathBuf,
};

use kftray_commons::utils::config_dir::get_log_folder_path;
use kftray_http_logs::HttpLogState;
use log::{
    error,
    info,
};

// HTTP Log State Management Commands

#[tauri::command]
pub async fn set_http_logs_cmd(
    state: tauri::State<'_, HttpLogState>, config_id: i64, enable: bool,
) -> Result<(), String> {
    state
        .set_http_logs(config_id, enable)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_http_logs_cmd(
    state: tauri::State<'_, HttpLogState>, config_id: i64,
) -> Result<bool, String> {
    state
        .get_http_logs(config_id)
        .await
        .map_err(|e| e.to_string())
}

// File System Operations

#[tauri::command]
pub async fn clear_http_logs() -> Result<(), String> {
    let log_folder_path = get_and_validate_log_folder()?;
    delete_files_in_folder(&log_folder_path)
}

#[tauri::command]
pub async fn get_http_log_size() -> Result<u64, String> {
    let log_folder_path = get_and_validate_log_folder()?;
    calculate_folder_size(&log_folder_path)
}

fn get_and_validate_log_folder() -> Result<PathBuf, String> {
    let log_folder_path = get_log_folder_path()?;

    if !log_folder_path.exists() {
        return Err(format!(
            "Log folder does not exist: {}",
            log_folder_path.display()
        ));
    }

    Ok(log_folder_path)
}

fn delete_files_in_folder(path: &Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err(format!("Path is not a directory: {}", path.display()));
    }

    let mut success_count = 0;
    let mut error_count = 0;
    let mut errors = Vec::new();

    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(e) => return Err(format!("Failed to read directory: {}", e)),
    };
    for entry_result in entries {
        match entry_result {
            Ok(entry) => {
                let file_path = entry.path();
                if file_path.is_file() {
                    match fs::remove_file(&file_path) {
                        Ok(_) => {
                            success_count += 1;
                        }
                        Err(e) => {
                            let error_msg =
                                format!("Failed to delete file {}: {}", file_path.display(), e);
                            error!("{}", error_msg);
                            errors.push(error_msg);
                            error_count += 1;
                        }
                    }
                }
            }
            Err(e) => {
                let error_msg = format!("Failed to read directory entry: {}", e);
                error!("{}", error_msg);
                errors.push(error_msg);
                error_count += 1;
            }
        }
    }

    // Report summary
    info!(
        "Deleted {} files, encountered {} errors",
        success_count, error_count
    );

    if error_count > 0 {
        if success_count > 0 {
            // Partial success - we deleted some files but not all
            Err(format!(
                "Partially deleted files: {} succeeded, {} failed. First error: {}",
                success_count, error_count, errors[0]
            ))
        } else {
            // Complete failure - couldn't delete any files
            Err(format!("Failed to delete any files: {}", errors[0]))
        }
    } else {
        Ok(())
    }
}

fn calculate_folder_size(path: &Path) -> Result<u64, String> {
    calculate_folder_size_with_depth(path, 0)
}

fn calculate_folder_size_with_depth(path: &Path, depth: usize) -> Result<u64, String> {
    // Prevent excessive recursion
    const MAX_DEPTH: usize = 32;
    if depth > MAX_DEPTH {
        return Err(format!(
            "Maximum directory depth exceeded: {}",
            path.display()
        ));
    }

    if !path.is_dir() {
        return Err(format!("Path is not a directory: {}", path.display()));
    }

    let mut size = 0;
    let mut visited_paths = std::collections::HashSet::new();

    // Try to get the canonical path to handle symlinks properly
    let canonical_path = path
        .canonicalize()
        .map_err(|e| format!("Failed to canonicalize path: {}", e))?;

    // Check if we've already visited this path (symlink loop detection)
    if !visited_paths.insert(canonical_path.clone()) {
        return Err(format!("Symlink loop detected at: {}", path.display()));
    }

    for entry in fs::read_dir(path).map_err(|e| format!("Failed to read directory: {}", e))? {
        let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
        let path = entry.path();

        if path.is_file() {
            size += fs::metadata(&path)
                .map_err(|e| format!("Failed to get file metadata: {}", e))?
                .len();
        } else if path.is_dir() {
            size += calculate_folder_size_with_depth(&path, depth + 1)?;
        }
    }

    Ok(size)
}

// File Opening Commands

#[tauri::command]
pub async fn open_log_file(log_file_name: String) -> Result<(), String> {
    use std::env;

    use open::that_in_background;

    let log_folder_path = get_log_folder_path()?;
    let log_file_path = log_folder_path.join(log_file_name);

    // Canonicalize paths to resolve any .. or symlinks
    let canonical_log_folder = log_folder_path
        .canonicalize()
        .map_err(|e| format!("Failed to canonicalize log folder path: {}", e))?;

    let canonical_file_path = log_file_path
        .canonicalize()
        .map_err(|e| format!("Failed to canonicalize log file path: {}", e))?;

    // Verify the file is actually within the log directory
    if !canonical_file_path.starts_with(&canonical_log_folder) {
        return Err("Invalid log file path: file is outside the log directory".to_string());
    }

    validate_file_exists(&canonical_file_path)?;
    info!("Opening log file: {}", canonical_file_path.display());

    let file_path_str = canonical_file_path
        .to_str()
        .ok_or_else(|| "Invalid file path: contains non-UTF-8 characters".to_string())?;

    // Try with EDITOR environment variable first
    if let Ok(editor) = env::var("EDITOR") {
        info!("Trying to open with EDITOR: {}", editor);
        if open_with_editor(file_path_str, &editor).is_ok() {
            return Ok(());
        }
        error!("Failed to open with EDITOR: {}", editor);
    }

    // Try with VISUAL environment variable (common on Unix systems)
    if let Ok(visual) = env::var("VISUAL") {
        info!("Trying to open with VISUAL: {}", visual);
        if open_with_editor(file_path_str, &visual).is_ok() {
            return Ok(());
        }
        error!("Failed to open with VISUAL: {}", visual);
    }

    // Then try with default editor detection
    let default_editor = detect_default_editor();
    info!(
        "Trying to open with detected default editor: {}",
        default_editor
    );

    match open_with_editor(file_path_str, &default_editor) {
        Ok(_) => Ok(()),
        Err(err) => {
            error!(
                "Error opening with editor '{}': {}. Trying default method...",
                default_editor, err
            );

            // Try with the open crate's default method
            match that_in_background(&canonical_file_path).join() {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(err)) => {
                    error!("Error opening log file with default method: {}. Trying fallback methods...", err);
                    try_fallback_editors(file_path_str)
                }
                Err(err) => Err(format!("Failed to join thread: {:?}", err)),
            }
        }
    }
}

fn validate_file_exists(file_path: &Path) -> Result<(), String> {
    if !file_path.exists() || fs::metadata(file_path).is_err() {
        return Err(format!("Log file does not exist: {}", file_path.display()));
    }
    Ok(())
}

// Editor Detection and File Opening

fn detect_default_editor() -> String {
    // Try code editors with HTTP file support first
    for editor in &["code", "cursor", "vscode", "atom", "sublime_text"] {
        if is_editor_available(editor) {
            return editor.to_string();
        }
    }

    // Fallback to OS defaults
    if cfg!(target_os = "macos") {
        "open".to_string()
    } else if cfg!(target_os = "windows") {
        "notepad".to_string()
    } else {
        // On Linux, try to find a common editor
        for editor in &["xdg-open", "gedit", "kate", "nano", "vim"] {
            if is_editor_available(editor) {
                return editor.to_string();
            }
        }
        "nano".to_string() // Final fallback
    }
}

fn is_editor_available(editor: &str) -> bool {
    std::process::Command::new(editor)
        .arg("--version")
        .output()
        .is_ok()
}

fn open_with_editor(file_path: &str, editor: &str) -> Result<(), String> {
    use std::ffi::OsStr;
    use std::process::{
        Command,
        Stdio,
    };
    use std::thread;
    use std::time::{
        Duration,
        Instant,
    };

    // Special handling for macOS 'open' command
    #[cfg(target_os = "macos")]
    if editor == "open" {
        return run_command("open", &["-t", file_path]);
    }

    // Handle editors with arguments
    let editor_parts: Vec<&str> = editor.split_whitespace().collect();
    if editor_parts.len() > 1 {
        let app = editor_parts[0];
        let args: Vec<&OsStr> = editor_parts[1..].iter().map(OsStr::new).collect();

        let mut command = Command::new(app);
        command.args(&args).arg(file_path);

        // Use Stdio::null() to prevent the editor from blocking on input
        command.stdin(Stdio::null());
        command.stdout(Stdio::null());
        command.stderr(Stdio::null());

        return match command.spawn() {
            Ok(mut child) => {
                // Set a timeout for waiting on the editor process
                let timeout = Duration::from_secs(5);
                let start_time = Instant::now();

                loop {
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            if status.success() {
                                return Ok(());
                            } else {
                                return Err(format!("Editor exited with status: {}", status));
                            }
                        }
                        Ok(None) => {
                            // Process still running
                            if start_time.elapsed() > timeout {
                                // Process is taking too long, assume it's running in background
                                info!("Editor process is still running after timeout, assuming success");
                                return Ok(());
                            }
                            // Small sleep to prevent CPU spinning
                            thread::sleep(Duration::from_millis(100));
                        }
                        Err(err) => {
                            return Err(format!("Failed to wait on editor process: {}", err))
                        }
                    }
                }
            }
            Err(err) => Err(format!("Failed to start editor: {}", err)),
        };
    }

    // Use the open crate for simple editor commands
    use open::with_in_background;

    // Create a copy of the file path and editor for the closure
    let editor_clone = editor.to_string();

    // Start the editor in a background thread
    let handle = with_in_background(file_path, editor);

    // Set a timeout for the operation
    let timeout = Duration::from_secs(5);
    let start = Instant::now();

    // Create a channel to communicate when the thread is done
    let (tx, rx) = std::sync::mpsc::channel();

    // Spawn a thread to join the handle
    thread::spawn(move || {
        let result = handle.join();
        let _ = tx.send(result); // Send the result, ignore errors if receiver
                                 // is dropped
    });

    // Wait for the result with a timeout
    loop {
        match rx.try_recv() {
            Ok(result) => {
                // We got a result from the thread
                return match result {
                    Ok(Ok(_)) => Ok(()),
                    Ok(Err(err)) => Err(format!("Failed to open with {}: {}", editor_clone, err)),
                    Err(_) => Err(format!(
                        "Thread panicked while opening with {}",
                        editor_clone
                    )),
                };
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                // No result yet, check timeout
                if start.elapsed() > timeout {
                    // Timeout occurred, assume success
                    info!(
                        "Editor {} is still running after timeout, assuming success",
                        editor_clone
                    );
                    return Ok(());
                }
                // Sleep a bit to avoid spinning
                thread::sleep(Duration::from_millis(100));
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                // Channel disconnected, assume failure
                return Err(format!(
                    "Channel disconnected while waiting for {}",
                    editor_clone
                ));
            }
        }
    }
}

fn run_command(cmd: &str, args: &[&str]) -> Result<(), String> {
    let status = std::process::Command::new(cmd)
        .args(args)
        .status()
        .map_err(|e| format!("Failed to execute {} command: {}", cmd, e))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("{} command failed with status: {}", cmd, status))
    }
}

fn try_fallback_editors(file_path: &str) -> Result<(), String> {
    // First try editors that handle HTTP files well
    let default_editor = detect_default_editor();
    let http_capable_editors = ["code", "cursor", "webstorm", "idea"];

    for editor in &http_capable_editors {
        // Skip if we already tried this editor as the default
        if *editor == default_editor {
            continue;
        }

        info!("Trying fallback editor: {}", editor);
        if open_with_editor(file_path, editor).is_ok() {
            return Ok(());
        }
    }

    // OS-specific fallbacks
    if cfg!(target_os = "windows") {
        try_windows_fallbacks(file_path)
    } else if cfg!(target_os = "macos") {
        try_macos_fallbacks(file_path)
    } else {
        try_linux_fallbacks(file_path)
    }
}

fn try_windows_fallbacks(file_path: &str) -> Result<(), String> {
    // Try Windows-specific editors in order of preference
    open_with_editor(file_path, "notepad")
        .or_else(|_| open_with_editor(file_path, "wordpad"))
        .or_else(|_| open_with_editor(file_path, "write"))
        .or_else(|_| {
            // Try the Windows default file association as a last resort
            run_command("cmd", &["/c", "start", "", file_path])
        })
}

fn try_macos_fallbacks(file_path: &str) -> Result<(), String> {
    // Try TextEdit first
    if run_command("open", &["-a", "TextEdit", file_path]).is_ok() {
        return Ok(());
    }

    // Try with -t flag (opens in default text editor)
    if run_command("open", &["-t", file_path]).is_ok() {
        return Ok(());
    }

    // Then try other options
    open_with_editor(file_path, "open")
        .or_else(|_| open_with_editor(file_path, "nano"))
        .or_else(|_| open_with_editor(file_path, "vim"))
        .or_else(|_| {
            // Final fallback - try plain open command
            match run_command("open", &[file_path]) {
                Ok(_) => Ok(()),
                Err(e) => {
                    error!("All macOS fallback methods failed. Last error: {}", e);
                    Err(format!(
                        "Failed to open file with any available method: {}",
                        e
                    ))
                }
            }
        })
}

fn try_linux_fallbacks(file_path: &str) -> Result<(), String> {
    // Try xdg-open first (should use desktop's file association)
    open_with_editor(file_path, "xdg-open")
        .or_else(|_| open_with_editor(file_path, "gedit"))
        .or_else(|_| open_with_editor(file_path, "kate"))
        .or_else(|_| open_with_editor(file_path, "kwrite"))
        .or_else(|_| open_with_editor(file_path, "leafpad"))
        .or_else(|_| open_with_editor(file_path, "mousepad"))
        .or_else(|_| open_with_editor(file_path, "nano"))
        .or_else(|_| open_with_editor(file_path, "vim"))
}

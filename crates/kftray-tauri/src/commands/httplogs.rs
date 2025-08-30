use std::fs;
use std::path::{
    Path,
    PathBuf,
};

use kftray_commons::models::http_logs_config_model::HttpLogsConfig;
use kftray_commons::utils::config_dir::get_log_folder_path;
use kftray_commons::utils::http_logs_config::{
    get_http_logs_config,
    update_http_logs_config,
};
use log::{
    error,
    info,
};

// HTTP Log Management Commands (using direct database access)

#[tauri::command]
pub async fn set_http_logs_cmd(config_id: i64, enable: bool) -> Result<(), String> {
    let mut config = match get_http_logs_config(config_id).await {
        Ok(config) => config,
        Err(_) => HttpLogsConfig::new(config_id),
    };

    config.enabled = enable;
    update_http_logs_config(&config)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_http_logs_cmd(config_id: i64) -> Result<bool, String> {
    match get_http_logs_config(config_id).await {
        Ok(config) => Ok(config.enabled),
        Err(_) => Ok(false),
    }
}

#[tauri::command]
pub async fn get_http_logs_config_cmd(config_id: i64) -> Result<HttpLogsConfig, String> {
    get_http_logs_config(config_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_http_logs_config_cmd(config: HttpLogsConfig) -> Result<(), String> {
    update_http_logs_config(&config)
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
        Err(e) => return Err(format!("Failed to read directory: {e}")),
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
                            error!("{error_msg}");
                            errors.push(error_msg);
                            error_count += 1;
                        }
                    }
                }
            }
            Err(e) => {
                let error_msg = format!("Failed to read directory entry: {e}");
                error!("{error_msg}");
                errors.push(error_msg);
                error_count += 1;
            }
        }
    }

    // Report summary
    info!("Deleted {success_count} files, encountered {error_count} errors");

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
        .map_err(|e| format!("Failed to canonicalize path: {e}"))?;

    // Check if we've already visited this path (symlink loop detection)
    if !visited_paths.insert(canonical_path.clone()) {
        return Err(format!("Symlink loop detected at: {}", path.display()));
    }

    for entry in fs::read_dir(path).map_err(|e| format!("Failed to read directory: {e}"))? {
        let entry = entry.map_err(|e| format!("Failed to read directory entry: {e}"))?;
        let path = entry.path();

        if path.is_file() {
            size += fs::metadata(&path)
                .map_err(|e| format!("Failed to get file metadata: {e}"))?
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
        .map_err(|e| format!("Failed to canonicalize log folder path: {e}"))?;

    let canonical_file_path = log_file_path
        .canonicalize()
        .map_err(|e| format!("Failed to canonicalize log file path: {e}"))?;

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
        info!("Trying to open with EDITOR: {editor}");
        if open_with_editor(file_path_str, &editor).is_ok() {
            return Ok(());
        }
        error!("Failed to open with EDITOR: {editor}");
    }

    // Try with VISUAL environment variable (common on Unix systems)
    if let Ok(visual) = env::var("VISUAL") {
        info!("Trying to open with VISUAL: {visual}");
        if open_with_editor(file_path_str, &visual).is_ok() {
            return Ok(());
        }
        error!("Failed to open with VISUAL: {visual}");
    }

    // Then try with default editor detection
    let default_editor = detect_default_editor();
    info!("Trying to open with detected default editor: {default_editor}");

    match open_with_editor(file_path_str, &default_editor) {
        Ok(_) => Ok(()),
        Err(err) => {
            error!("Error opening with editor '{default_editor}': {err}. Trying default method...");

            // Try with the open crate's default method
            match that_in_background(&canonical_file_path).join() {
                Ok(Ok(_)) => Ok(()),
                Ok(Err(err)) => {
                    error!(
                        "Error opening log file with default method: {err}. Trying fallback methods..."
                    );
                    try_fallback_editors(file_path_str)
                }
                Err(err) => Err(format!("Failed to join thread: {err:?}")),
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
                                return Err(format!("Editor exited with status: {status}"));
                            }
                        }
                        Ok(None) => {
                            // Process still running
                            if start_time.elapsed() > timeout {
                                // Process is taking too long, assume it's running in background
                                info!(
                                    "Editor process is still running after timeout, assuming success"
                                );
                                return Ok(());
                            }
                            // Small sleep to prevent CPU spinning
                            thread::sleep(Duration::from_millis(100));
                        }
                        Err(err) => return Err(format!("Failed to wait on editor process: {err}")),
                    }
                }
            }
            Err(err) => Err(format!("Failed to start editor: {err}")),
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
                    Ok(Err(err)) => Err(format!("Failed to open with {editor_clone}: {err}")),
                    Err(_) => Err(format!("Thread panicked while opening with {editor_clone}")),
                };
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                // No result yet, check timeout
                if start.elapsed() > timeout {
                    // Timeout occurred, assume success
                    info!("Editor {editor_clone} is still running after timeout, assuming success");
                    return Ok(());
                }
                // Sleep a bit to avoid spinning
                thread::sleep(Duration::from_millis(100));
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                // Channel disconnected, assume failure
                return Err(format!(
                    "Channel disconnected while waiting for {editor_clone}"
                ));
            }
        }
    }
}

fn run_command(cmd: &str, args: &[&str]) -> Result<(), String> {
    let status = std::process::Command::new(cmd)
        .args(args)
        .status()
        .map_err(|e| format!("Failed to execute {cmd} command: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("{cmd} command failed with status: {status}"))
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

        info!("Trying fallback editor: {editor}");
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
                    error!("All macOS fallback methods failed. Last error: {e}");
                    Err(format!(
                        "Failed to open file with any available method: {e}"
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::env;
    use std::io::Write;
    use std::sync::{
        Arc,
        Mutex,
    };

    use kftray_commons::models::config_model::Config;
    use lazy_static::lazy_static;
    use sqlx::SqlitePool;
    use tempfile::TempDir;
    use tokio::sync::Mutex as TokioMutex;

    use super::*;

    lazy_static! {
        static ref TEST_MUTEX: TokioMutex<()> = TokioMutex::new(());
    }

    async fn setup_isolated_test_db() -> Arc<SqlitePool> {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        kftray_commons::utils::db::create_db_table(&pool)
            .await
            .unwrap();
        kftray_commons::utils::migration::migrate_configs(Some(&pool))
            .await
            .unwrap();

        let arc_pool = Arc::new(pool);

        // Set this as the global pool for command functions to use
        let _ = kftray_commons::utils::db::DB_POOL.set(arc_pool.clone());

        arc_pool
    }

    async fn create_test_config() -> Result<i64, String> {
        use std::time::SystemTime;

        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let service_name = format!("test-service-{}", timestamp);

        let config = Config {
            service: Some(service_name.clone()),
            namespace: "test-namespace".to_string(),
            local_port: Some(8080),
            remote_port: Some(80),
            context: Some("test-context".to_string()),
            workload_type: Some("service".to_string()),
            protocol: "tcp".to_string(),
            remote_address: Some("remote-address".to_string()),
            local_address: Some("127.0.0.1".to_string()),
            auto_loopback_address: false,
            alias: Some("test-alias".to_string()),
            domain_enabled: Some(false),
            kubeconfig: None,
            target: Some("test-target".to_string()),
            http_logs_enabled: Some(false),
            http_logs_max_file_size: Some(10 * 1024 * 1024),
            http_logs_retention_days: Some(7),
            http_logs_auto_cleanup: Some(true),
            ..Default::default()
        };

        kftray_commons::config::insert_config(config)
            .await
            .map_err(|e| e.to_string())?;

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let configs = kftray_commons::config::get_configs()
            .await
            .map_err(|e| e.to_string())?;
        let inserted_config = configs
            .iter()
            .find(|c| c.service == Some(service_name.clone()))
            .ok_or("Failed to find inserted config")?;

        inserted_config.id.ok_or("Config missing ID".to_string())
    }

    struct EnvGuard {
        vars: Vec<(String, Option<String>)>,
    }

    impl EnvGuard {
        fn new(vars: &[&str]) -> Self {
            let vars = vars
                .iter()
                .map(|&name| (name.to_string(), env::var(name).ok()))
                .collect();
            EnvGuard { vars }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (name, value) in &self.vars {
                match value {
                    Some(val) => unsafe { env::set_var(name, val) },

                    None => unsafe { env::remove_var(name) },
                }
            }
        }
    }

    struct MockHttpLogState {
        logs_enabled: Arc<Mutex<HashMap<i64, bool>>>,
    }

    impl MockHttpLogState {
        fn new() -> Self {
            Self {
                logs_enabled: Arc::new(Mutex::new(HashMap::new())),
            }
        }

        async fn set_http_logs(&self, config_id: i64, enable: bool) -> Result<(), String> {
            let mut map = self.logs_enabled.lock().unwrap();
            map.insert(config_id, enable);
            Ok(())
        }

        async fn get_http_logs(&self, config_id: i64) -> Result<bool, String> {
            let map = self.logs_enabled.lock().unwrap();
            Ok(*map.get(&config_id).unwrap_or(&false))
        }
    }

    fn create_test_log_folder() -> TempDir {
        let temp_dir = tempfile::tempdir().unwrap();

        for i in 1..5 {
            let file_path = temp_dir.path().join(format!("test_log_{i}.log"));
            let mut file = std::fs::File::create(&file_path).unwrap();
            writeln!(file, "Test log content {i}").unwrap();
        }

        temp_dir
    }

    #[test]
    fn test_http_logs_state() {
        let rt = tokio::runtime::Runtime::new().unwrap();

        rt.block_on(async {
            let state = MockHttpLogState::new();
            let config_id = 123;

            let initial_state = state.get_http_logs(config_id).await.unwrap();
            assert!(!initial_state, "Initial HTTP logs state should be false");

            state.set_http_logs(config_id, true).await.unwrap();
            let enabled_state = state.get_http_logs(config_id).await.unwrap();
            assert!(
                enabled_state,
                "HTTP logs should be enabled after set_http_logs(true)"
            );

            state.set_http_logs(config_id, false).await.unwrap();
            let disabled_state = state.get_http_logs(config_id).await.unwrap();
            assert!(
                !disabled_state,
                "HTTP logs should be disabled after set_http_logs(false)"
            );
        });
    }

    #[test]
    fn test_calculate_folder_size() {
        let temp_dir = create_test_log_folder();

        let size = calculate_folder_size(temp_dir.path()).unwrap();

        assert!(size > 0, "Folder size should be greater than 0");

        let mut expected_size = 0;
        for entry in std::fs::read_dir(temp_dir.path()).unwrap() {
            let entry = entry.unwrap();
            if entry.path().is_file() {
                expected_size += std::fs::metadata(entry.path()).unwrap().len();
            }
        }

        assert_eq!(
            size, expected_size,
            "Calculated size should match expected size"
        );
    }

    #[test]
    fn test_calculate_folder_size_nonexistent() {
        let path = std::path::Path::new("/this/path/does/not/exist");

        let result = calculate_folder_size(path);

        assert!(
            result.is_err(),
            "Should return error for non-existent directory"
        );
    }

    #[test]
    fn test_calculate_folder_size_not_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test_file.txt");
        std::fs::write(&file_path, "test content").unwrap();

        let result = calculate_folder_size(&file_path);

        assert!(
            result.is_err(),
            "Should return error when path is not a directory"
        );
        assert!(
            result.unwrap_err().contains("not a directory"),
            "Error message should indicate path is not a directory"
        );
    }

    #[test]
    fn test_delete_files_in_folder() {
        let temp_dir = create_test_log_folder();

        let file_count_before = std::fs::read_dir(temp_dir.path()).unwrap().count();
        assert!(
            file_count_before > 0,
            "Should have test files before deletion"
        );

        let result = delete_files_in_folder(temp_dir.path());

        assert!(result.is_ok(), "Should successfully delete files");

        let file_count_after = std::fs::read_dir(temp_dir.path()).unwrap().count();
        assert_eq!(file_count_after, 0, "All files should be deleted");
    }

    #[test]
    fn test_delete_files_in_nonexistent_folder() {
        let path = std::path::Path::new("/this/path/does/not/exist");

        let result = delete_files_in_folder(path);

        assert!(
            result.is_err(),
            "Should return error for non-existent directory"
        );
    }

    #[test]
    fn test_detect_default_editor() {
        let editor = detect_default_editor();

        assert!(!editor.is_empty(), "Should detect a default editor");

        if cfg!(target_os = "macos") {
            assert!(
                editor == "code" || editor == "open",
                "Default editor on macOS should be 'code' or 'open'"
            );
        }

        if cfg!(target_os = "windows") && !is_editor_available("code") {
            assert_eq!(
                editor, "notepad",
                "Default fallback editor on Windows should be 'notepad'"
            );
        }
    }

    #[test]
    fn test_is_editor_available() {
        if cfg!(target_os = "macos") {
            assert!(
                is_editor_available("open"),
                "The 'open' command should be available on macOS"
            );
        } else if cfg!(target_os = "windows") {
            assert!(
                is_editor_available("notepad"),
                "Notepad should be available on Windows"
            );
        } else {
            let linux_editors = ["xdg-open", "gedit", "nano", "vim"];
            let any_available = linux_editors
                .iter()
                .any(|editor| is_editor_available(editor));
            assert!(
                any_available,
                "At least one common editor should be available on Linux"
            );
        }

        assert!(
            !is_editor_available("this_editor_does_not_exist_12345"),
            "Non-existent editor should not be available"
        );
    }

    #[test]
    fn test_validate_file_exists() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test_file.txt");
        std::fs::write(&file_path, "test content").unwrap();

        // Test existing file
        let result = validate_file_exists(&file_path);
        assert!(result.is_ok(), "Should succeed for existing file");

        // Test non-existent file
        let non_existent = temp_dir.path().join("non_existent.txt");
        let result = validate_file_exists(&non_existent);
        assert!(
            result.is_err(),
            "Should fail for non-existent file but got Ok"
        );
        assert!(
            result.unwrap_err().contains("does not exist"),
            "Error should mention file does not exist"
        );
    }

    #[test]
    fn test_get_and_validate_log_folder() {
        let _guard = EnvGuard::new(&["KFTRAY_CONFIG", "XDG_CONFIG_HOME", "HOME"]);
        let temp_dir = tempfile::tempdir().unwrap();
        let non_existent_path = temp_dir.path().join("non_existent");

        unsafe { env::set_var("KFTRAY_CONFIG", non_existent_path.to_str().unwrap()) };

        let result = get_and_validate_log_folder();
        assert!(
            result.is_err(),
            "Should fail when log folder doesn't exist but got Ok"
        );
        assert!(
            result.unwrap_err().contains("does not exist"),
            "Error should mention folder does not exist"
        );
    }

    #[tokio::test]
    async fn test_open_log_file_validation() {
        let _guard = EnvGuard::new(&["KFTRAY_CONFIG", "XDG_CONFIG_HOME", "HOME"]);

        let temp_dir = tempfile::tempdir().unwrap();
        let log_folder = temp_dir.path().join("http_logs");

        unsafe { env::set_var("KFTRAY_CONFIG", temp_dir.path().to_str().unwrap()) };
        std::fs::create_dir_all(&log_folder).unwrap();

        let outside_dir = tempfile::tempdir().unwrap();
        let file_path = outside_dir.path().join("outside_logs.txt");
        std::fs::write(&file_path, "test content").unwrap();

        match open_log_file(file_path.to_string_lossy().to_string()).await {
            Ok(_) => panic!("Should fail for file outside log directory"),
            Err(e) => {
                let contains_outside =
                    e.contains("outside the log directory") || e.contains("Invalid log file path");
                assert!(
                    contains_outside,
                    "Error '{e}' should indicate file is outside log directory"
                );
            }
        }

        match open_log_file("invalid_utf8_\u{FFFF}".to_string()).await {
            Ok(_) => panic!("Should fail for invalid UTF-8 path"),
            Err(e) => assert!(
                !e.is_empty(),
                "Error message should not be empty for invalid UTF-8 path"
            ),
        }
    }

    #[tokio::test]
    async fn test_http_logs_config_persistence() {
        let _guard = TEST_MUTEX.lock().await;
        let _pool = setup_isolated_test_db().await;

        let config_id = create_test_config()
            .await
            .expect("Failed to create test config");

        let initial_config = get_http_logs_config_cmd(config_id).await.unwrap();
        assert_eq!(initial_config.config_id, config_id);
        assert!(!initial_config.enabled);
        assert_eq!(initial_config.max_file_size, 10 * 1024 * 1024);
        assert_eq!(initial_config.retention_days, 7);
        assert!(initial_config.auto_cleanup);

        let updated_config = HttpLogsConfig {
            config_id,
            enabled: true,
            max_file_size: 20 * 1024 * 1024,
            retention_days: 14,
            auto_cleanup: false,
        };

        update_http_logs_config_cmd(updated_config.clone())
            .await
            .unwrap();

        let retrieved_config = get_http_logs_config_cmd(config_id).await.unwrap();
        assert_eq!(retrieved_config.config_id, config_id);
        assert!(retrieved_config.enabled);
        assert_eq!(retrieved_config.max_file_size, 20 * 1024 * 1024);
        assert_eq!(retrieved_config.retention_days, 14);
        assert!(!retrieved_config.auto_cleanup);
    }

    #[tokio::test]
    async fn test_http_logs_direct_database_integration() {
        let _guard = TEST_MUTEX.lock().await;
        let _pool = setup_isolated_test_db().await;

        let config_id = create_test_config()
            .await
            .expect("Failed to create test config");

        let initial_state = get_http_logs_cmd(config_id).await.unwrap();
        assert!(!initial_state);

        set_http_logs_cmd(config_id, true).await.unwrap();
        let enabled_state = get_http_logs_cmd(config_id).await.unwrap();
        assert!(enabled_state);

        let db_config = get_http_logs_config_cmd(config_id).await.unwrap();
        assert_eq!(db_config.config_id, config_id);
        assert!(db_config.enabled);

        set_http_logs_cmd(config_id, false).await.unwrap();
        let disabled_state = get_http_logs_cmd(config_id).await.unwrap();
        assert!(!disabled_state);

        let db_config_disabled = get_http_logs_config_cmd(config_id).await.unwrap();
        assert_eq!(db_config_disabled.config_id, config_id);
        assert!(!db_config_disabled.enabled);
    }

    #[tokio::test]
    async fn test_http_logs_config_validation() {
        let _guard = TEST_MUTEX.lock().await;
        let _pool = setup_isolated_test_db().await;

        let config_id = create_test_config()
            .await
            .expect("Failed to create test config");

        let max_size_config = HttpLogsConfig {
            config_id,
            enabled: true,
            max_file_size: 100 * 1024 * 1024,
            retention_days: 365,
            auto_cleanup: true,
        };

        update_http_logs_config_cmd(max_size_config.clone())
            .await
            .unwrap();
        let retrieved = get_http_logs_config_cmd(config_id).await.unwrap();
        assert_eq!(retrieved.max_file_size, 100 * 1024 * 1024);
        assert_eq!(retrieved.retention_days, 365);

        let min_config = HttpLogsConfig {
            config_id,
            enabled: false,
            max_file_size: 1024,
            retention_days: 1,
            auto_cleanup: false,
        };

        update_http_logs_config_cmd(min_config.clone())
            .await
            .unwrap();
        let retrieved_min = get_http_logs_config_cmd(config_id).await.unwrap();
        assert_eq!(retrieved_min.max_file_size, 1024);
        assert_eq!(retrieved_min.retention_days, 1);
        assert!(!retrieved_min.enabled);
        assert!(!retrieved_min.auto_cleanup);
    }

    #[tokio::test]
    async fn test_http_logs_state_load_from_database() {
        let _guard = TEST_MUTEX.lock().await;
        let _pool = setup_isolated_test_db().await;

        let config_id = create_test_config()
            .await
            .expect("Failed to create test config");
        let test_config = HttpLogsConfig {
            config_id,
            enabled: true,
            max_file_size: 15 * 1024 * 1024,
            retention_days: 10,
            auto_cleanup: true,
        };
        update_http_logs_config_cmd(test_config.clone())
            .await
            .unwrap();

        let loaded_state = get_http_logs_cmd(config_id).await.unwrap();
        assert!(
            loaded_state,
            "HTTP logs should be enabled after loading from database"
        );

        let db_config = get_http_logs_config_cmd(config_id).await.unwrap();
        assert_eq!(db_config.max_file_size, 15 * 1024 * 1024);
        assert_eq!(db_config.retention_days, 10);
        assert!(db_config.auto_cleanup);
    }
}

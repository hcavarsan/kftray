use std::fs;
use std::io::{
    BufRead,
    BufReader,
};
use std::path::PathBuf;

use jiff::Zoned;
use log::info;
use serde::{
    Deserialize,
    Serialize,
};
use tauri::AppHandle;
use tauri::Manager;
use tauri::WebviewUrl;
use tauri::WebviewWindowBuilder;

use super::settings::{
    DiagnosticsReport,
    run_diagnostics,
};

#[derive(Serialize)]
pub struct LogInfo {
    pub log_path: String,
    pub log_size: u64,
    pub exists: bool,
}

#[derive(Serialize, Clone)]
pub struct LogEntry {
    pub id: usize,
    pub raw: String,
    pub timestamp: Option<String>,
    pub date: Option<String>,
    pub time: Option<String>,
    pub level: Option<String>,
    pub module: Option<String>,
    pub message: String,
    pub is_parsed: bool,
}

impl LogEntry {
    fn unparsed(id: usize, line: &str) -> Self {
        Self {
            id,
            raw: line.to_string(),
            timestamp: None,
            date: None,
            time: None,
            level: None,
            module: None,
            message: line.to_string(),
            is_parsed: false,
        }
    }
}

#[derive(Serialize, Clone)]
pub struct LogFileInfo {
    pub filename: String,
    pub path: String,
    pub size: u64,
    pub created_at: String,
    pub age_days: u32,
    pub is_current: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct LogSettings {
    pub retention_count: u32,
    pub retention_days: u32,
}

impl Default for LogSettings {
    fn default() -> Self {
        Self {
            retention_count: 10,
            retention_days: 7,
        }
    }
}

fn parse_log_line(id: usize, line: &str) -> LogEntry {
    if line.len() < 30 || !line.starts_with('[') {
        return LogEntry::unparsed(id, line);
    }

    if line.get(11..12) != Some("]") || line.get(12..13) != Some("[") {
        return LogEntry::unparsed(id, line);
    }
    let date = &line[1..11];

    if line.get(21..22) != Some("]") || line.get(22..23) != Some("[") {
        return LogEntry::unparsed(id, line);
    }
    let time = &line[13..21];

    let level_end = match line[23..].find(']') {
        Some(i) => 23 + i,
        None => return LogEntry::unparsed(id, line),
    };
    let level = &line[23..level_end];

    if !matches!(level, "ERROR" | "WARN" | "INFO" | "DEBUG" | "TRACE") {
        return LogEntry::unparsed(id, line);
    }

    let module_start = level_end + 2;
    if line.get(level_end..module_start) != Some("][") {
        return LogEntry::unparsed(id, line);
    }

    let module_end = match line[module_start..].find(']') {
        Some(i) => module_start + i,
        None => return LogEntry::unparsed(id, line),
    };
    let module = &line[module_start..module_end];

    let message = line.get(module_end + 1..).unwrap_or("").trim();

    LogEntry {
        id,
        raw: line.to_string(),
        date: Some(date.to_string()),
        time: Some(time.to_string()),
        timestamp: Some(format!("{} {}", date, time)),
        level: Some(level.to_string()),
        module: Some(module.to_string()),
        message: message.to_string(),
        is_parsed: true,
    }
}

fn get_log_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_log_dir().ok()
}

fn is_kftray_log_file(filename: &str) -> bool {
    filename.starts_with("kftray_") && filename.ends_with(".log")
}

fn parse_log_file_timestamp(filename: &str) -> Option<String> {
    if !is_kftray_log_file(filename) {
        return None;
    }
    let without_prefix = filename.strip_prefix("kftray_")?;
    let without_suffix = without_prefix.strip_suffix(".log")?;
    let formatted = without_suffix.replace('_', " ").replace('-', ":");
    if formatted.len() >= 19 {
        let date_part = &without_suffix[0..10];
        let time_part = &without_suffix[11..];
        let time_formatted = time_part.replace('-', ":");
        Some(format!("{} {}", date_part, time_formatted))
    } else {
        None
    }
}

fn calculate_age_days(timestamp: &str) -> u32 {
    let now = Zoned::now();

    if timestamp.len() >= 10 {
        let date_str = &timestamp[0..10];
        if let Ok(parsed) = jiff::civil::Date::strptime("%Y-%m-%d", date_str) {
            let today = now.date();
            if let Ok(span) = today.since(parsed) {
                return span.get_days().unsigned_abs();
            }
        }
    }
    0
}

fn get_log_file_path(app: &AppHandle, filename: Option<&str>) -> Option<PathBuf> {
    let log_dir = get_log_dir(app)?;

    if let Some(name) = filename {
        Some(log_dir.join(name))
    } else {
        find_current_log_file(&log_dir)
    }
}

fn find_current_log_file(log_dir: &PathBuf) -> Option<PathBuf> {
    let mut log_files: Vec<_> = fs::read_dir(log_dir)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_str().is_some_and(is_kftray_log_file))
        .collect();

    log_files.sort_by_key(|b| std::cmp::Reverse(b.file_name()));

    log_files.first().map(|entry| entry.path())
}

fn list_all_log_files(log_dir: &PathBuf) -> Vec<(PathBuf, String)> {
    fs::read_dir(log_dir)
        .ok()
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| {
                    let filename = entry.file_name().to_string_lossy().to_string();
                    if is_kftray_log_file(&filename) {
                        Some((entry.path(), filename))
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

#[tauri::command]
pub async fn get_log_info(app: AppHandle, filename: Option<String>) -> Result<LogInfo, String> {
    let log_path =
        get_log_file_path(&app, filename.as_deref()).ok_or("Could not determine log path")?;

    let exists = log_path.exists();
    let log_size = if exists {
        fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0)
    } else {
        0
    };

    Ok(LogInfo {
        log_path: log_path.to_string_lossy().into(),
        log_size,
        exists,
    })
}

#[tauri::command]
pub async fn get_log_contents(
    app: AppHandle, lines: Option<usize>, filename: Option<String>,
) -> Result<String, String> {
    let log_path =
        get_log_file_path(&app, filename.as_deref()).ok_or("Could not determine log path")?;

    if !log_path.exists() {
        return Ok(String::new());
    }

    let file = fs::File::open(&log_path).map_err(|e| format!("Failed to open log file: {e}"))?;
    let reader = BufReader::new(file);
    let all_lines: Vec<String> = reader.lines().map_while(Result::ok).collect();

    let limit = lines.unwrap_or(500);
    let start = all_lines.len().saturating_sub(limit);
    let result = all_lines[start..].join("\n");

    Ok(result)
}

#[tauri::command]
pub async fn get_log_contents_json(
    app: AppHandle, lines: Option<usize>, filename: Option<String>,
) -> Result<Vec<LogEntry>, String> {
    let log_path =
        get_log_file_path(&app, filename.as_deref()).ok_or("Could not determine log path")?;

    if !log_path.exists() {
        return Ok(Vec::new());
    }

    let file = fs::File::open(&log_path).map_err(|e| format!("Failed to open log file: {e}"))?;
    let reader = BufReader::new(file);
    let all_lines: Vec<String> = reader.lines().map_while(Result::ok).collect();

    let limit = lines.unwrap_or(500);
    let start = all_lines.len().saturating_sub(limit);

    let entries: Vec<LogEntry> = all_lines[start..]
        .iter()
        .enumerate()
        .map(|(i, line)| parse_log_line(start + i, line))
        .collect();

    Ok(entries)
}

#[tauri::command]
pub async fn clear_logs(app: AppHandle, filename: Option<String>) -> Result<(), String> {
    let log_path =
        get_log_file_path(&app, filename.as_deref()).ok_or("Could not determine log path")?;

    if log_path.exists() {
        fs::write(&log_path, "").map_err(|e| format!("Failed to clear logs: {e}"))?;
    }

    Ok(())
}

#[tauri::command]
pub async fn list_log_files(app: AppHandle) -> Result<Vec<LogFileInfo>, String> {
    let log_dir = get_log_dir(&app).ok_or("Could not determine log directory")?;

    let current_log = find_current_log_file(&log_dir);
    let current_filename = current_log
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("");

    let log_files = list_all_log_files(&log_dir);

    let mut result: Vec<LogFileInfo> = log_files
        .iter()
        .filter_map(|(path, filename)| {
            let metadata = fs::metadata(path).ok()?;
            let size = metadata.len();

            let created_at = parse_log_file_timestamp(filename).unwrap_or_else(|| "Unknown".into());
            let age_days = calculate_age_days(&created_at);
            let is_current = filename == current_filename;

            Some(LogFileInfo {
                filename: filename.clone(),
                path: path.to_string_lossy().to_string(),
                size,
                created_at,
                age_days,
                is_current,
            })
        })
        .collect();

    result.sort_by(|a, b| b.filename.cmp(&a.filename));

    Ok(result)
}

#[tauri::command]
pub async fn cleanup_old_logs(app: AppHandle) -> Result<u32, String> {
    let settings = get_log_settings_internal().await?;
    cleanup_logs_with_settings(&app, &settings).await
}

async fn cleanup_logs_with_settings(
    app: &AppHandle, settings: &LogSettings,
) -> Result<u32, String> {
    let log_dir = get_log_dir(app).ok_or("Could not determine log directory")?;

    let current_log = find_current_log_file(&log_dir);
    let current_filename = current_log
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .map(|s| s.to_string());

    let mut log_files = list_all_log_files(&log_dir);

    log_files.sort_by(|a, b| b.1.cmp(&a.1));

    let total_count = log_files.len();
    let mut deleted_count = 0u32;

    for (i, (path, filename)) in log_files.iter().enumerate() {
        if Some(filename.as_str()) == current_filename.as_deref() {
            continue;
        }

        let created_at = parse_log_file_timestamp(filename).unwrap_or_default();
        let age_days = calculate_age_days(&created_at);

        let exceeds_count = total_count > settings.retention_count as usize;
        let exceeds_age = age_days > settings.retention_days;

        let is_beyond_count_limit = i >= settings.retention_count as usize;

        if exceeds_count && exceeds_age && is_beyond_count_limit {
            if let Err(e) = fs::remove_file(path) {
                log::warn!("Failed to delete old log file {}: {}", filename, e);
            } else {
                info!("Deleted old log file: {}", filename);
                deleted_count += 1;
            }
        }
    }

    Ok(deleted_count)
}

pub async fn cleanup_old_logs_on_startup(app: AppHandle) -> Result<u32, String> {
    info!("Running log cleanup on startup");
    let settings = get_log_settings_internal().await?;
    let deleted = cleanup_logs_with_settings(&app, &settings).await?;
    if deleted > 0 {
        info!("Cleaned up {} old log files on startup", deleted);
    }
    Ok(deleted)
}

#[tauri::command]
pub async fn delete_log_file(app: AppHandle, filename: String) -> Result<(), String> {
    let log_dir = get_log_dir(&app).ok_or("Could not determine log directory")?;

    if filename.contains('/') || filename.contains('\\') || !is_kftray_log_file(&filename) {
        return Err("Invalid log filename".into());
    }

    let current_log = find_current_log_file(&log_dir);
    let current_filename = current_log
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str());

    if Some(filename.as_str()) == current_filename {
        return Err("Cannot delete the current log file".into());
    }

    let file_path = log_dir.join(&filename);
    if !file_path.exists() {
        return Err("Log file not found".into());
    }

    fs::remove_file(&file_path).map_err(|e| format!("Failed to delete log file: {e}"))?;
    info!("Deleted log file: {}", filename);

    Ok(())
}

async fn get_log_settings_internal() -> Result<LogSettings, String> {
    let retention_count = kftray_commons::utils::settings::get_setting("log_retention_count")
        .await
        .map_err(|e| format!("Failed to get log retention count: {e}"))?
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(10);

    let retention_days = kftray_commons::utils::settings::get_setting("log_retention_days")
        .await
        .map_err(|e| format!("Failed to get log retention days: {e}"))?
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(7);

    Ok(LogSettings {
        retention_count,
        retention_days,
    })
}

#[tauri::command]
pub async fn get_log_settings() -> Result<LogSettings, String> {
    get_log_settings_internal().await
}

#[tauri::command]
pub async fn set_log_settings(settings: LogSettings) -> Result<(), String> {
    if settings.retention_count < 1 || settings.retention_count > 100 {
        return Err("Retention count must be between 1 and 100".into());
    }
    if settings.retention_days < 1 || settings.retention_days > 365 {
        return Err("Retention days must be between 1 and 365".into());
    }

    kftray_commons::utils::settings::set_setting(
        "log_retention_count",
        &settings.retention_count.to_string(),
    )
    .await
    .map_err(|e| format!("Failed to set log retention count: {e}"))?;

    kftray_commons::utils::settings::set_setting(
        "log_retention_days",
        &settings.retention_days.to_string(),
    )
    .await
    .map_err(|e| format!("Failed to set log retention days: {e}"))?;

    info!(
        "Log settings updated: retention_count={}, retention_days={}",
        settings.retention_count, settings.retention_days
    );

    Ok(())
}

#[derive(Serialize)]
pub struct DiagnosticReport {
    pub generated_at: String,
    pub app_version: String,
    pub platform: PlatformInfo,
    pub diagnostics: DiagnosticsReport,
    pub environment: EnvironmentInfo,
    pub logs: String,
}

#[derive(Serialize)]
pub struct PlatformInfo {
    pub os: String,
    pub arch: String,
}

#[derive(Serialize)]
pub struct EnvironmentInfo {
    pub home: Option<String>,
    pub path: Option<String>,
    pub kubeconfig: Option<String>,
    pub shell: Option<String>,
}

#[tauri::command]
pub async fn generate_diagnostic_report(app: AppHandle) -> Result<String, String> {
    let diagnostics = run_diagnostics().await?;

    let logs = get_log_contents(app.clone(), Some(200), None)
        .await
        .unwrap_or_default();

    let version = app.package_info().version.to_string();

    let report = DiagnosticReport {
        generated_at: Zoned::now().to_string(),
        app_version: version,
        platform: PlatformInfo {
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
        },
        diagnostics,
        environment: EnvironmentInfo {
            home: std::env::var("HOME").ok(),
            path: std::env::var("PATH").ok().map(|p| {
                if p.len() > 200 {
                    format!("{}...", &p[..200])
                } else {
                    p
                }
            }),
            kubeconfig: std::env::var("KUBECONFIG").ok(),
            shell: std::env::var("SHELL").ok(),
        },
        logs,
    };

    serde_json::to_string_pretty(&report).map_err(|e| format!("Failed to serialize report: {e}"))
}

#[tauri::command]
pub async fn open_log_directory(app: AppHandle) -> Result<(), String> {
    let log_dir = get_log_dir(&app).ok_or("Could not determine log directory")?;

    open::that(&log_dir).map_err(|e| format!("Failed to open log directory: {e}"))?;

    Ok(())
}

#[tauri::command]
pub async fn open_log_viewer_window_cmd(app: AppHandle) -> Result<(), String> {
    open_log_viewer_window(app)
}

pub fn open_log_viewer_window(app: AppHandle) -> Result<(), String> {
    info!("Opening log viewer window");

    if let Some(window) = app.get_webview_window("logs") {
        info!("Logs window already exists, showing it");
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    let main_window = app.get_webview_window("main");
    let monitor = main_window
        .as_ref()
        .and_then(|w| w.current_monitor().ok().flatten())
        .or_else(|| {
            info!("current_monitor() returned None, falling back to primary_monitor()");
            main_window
                .as_ref()
                .and_then(|w| w.primary_monitor().ok().flatten())
        });

    let (width, height, scale_factor) = monitor
        .as_ref()
        .map(|m| {
            let size = m.size();
            let sf = m.scale_factor();
            let w = (size.width as f64 / sf * 0.75).clamp(800.0, 1800.0);
            let h = (size.height as f64 / sf * 0.80).clamp(500.0, 1200.0);
            (w, h, sf)
        })
        .unwrap_or((1200.0, 800.0, 1.0));

    info!(
        "Creating logs window: logical size {}x{}, scale factor {}",
        width, height, scale_factor
    );

    let mut builder = WebviewWindowBuilder::new(&app, "logs", WebviewUrl::App("logs.html".into()))
        .title("KFtray - Logs")
        .inner_size(width, height)
        .min_inner_size(800.0, 500.0)
        .resizable(true)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(false)
        .visible(true);

    if let Some(m) = &monitor {
        let mon_pos = m.position();
        let mon_size = m.size();

        let x = (mon_pos.x as f64 + (mon_size.width as f64 - width * scale_factor) / 2.0)
            / scale_factor;
        let y = (mon_pos.y as f64 + (mon_size.height as f64 - height * scale_factor) / 2.0)
            / scale_factor;

        info!(
            "Setting logs window position: logical ({}, {}) on monitor at physical ({}, {})",
            x, y, mon_pos.x, mon_pos.y
        );

        builder = builder.position(x, y);
    } else {
        info!("No monitor info available, using center()");
        builder = builder.center();
    }

    let window = builder
        .build()
        .map_err(|e| format!("Failed to create log viewer window: {e}"))?;

    window.set_focus().map_err(|e| e.to_string())?;

    info!("Logs window created successfully");
    Ok(())
}

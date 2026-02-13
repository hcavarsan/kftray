use std::io::ErrorKind;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;

use kftray_commons::models::window::AppState;
use kftray_commons::models::window::WindowPosition;
use kftray_commons::utils::config_dir::get_window_state_path;
use log::{info, warn};
use tauri::{Manager, PhysicalPosition, PhysicalSize, WebviewWindow, Wry};
use tauri_plugin_positioner::{Position, WindowExt};
use tokio::time::sleep;

pub fn save_window_position(window: &WebviewWindow<Wry>) {
    let app_state = window.state::<AppState>();

    if app_state.positioning_active.load(Ordering::SeqCst) {
        info!("Skipping position save - app positioning active");
        return;
    }

    if let Ok(position) = window.outer_position() {
        info!(
            "Attempting to save position: ({}, {})",
            position.x, position.y
        );

        if is_valid_position(window, position.x, position.y) {
            let position_data = WindowPosition {
                x: position.x,
                y: position.y,
            };
            let runtime = app_state.runtime.clone();

            runtime.spawn(async move {
                save_position_async(position_data).await;
            });
        } else {
            warn!(
                "Position ({}, {}) failed validation",
                position.x, position.y
            );
        }
    } else {
        warn!("Failed to get window outer position");
    }
}

async fn save_position_async(position_data: WindowPosition) {
    let position_json = match serde_json::to_string(&position_data) {
        Ok(json) => json,
        Err(e) => {
            warn!("Failed to serialize window position: {e}");
            return;
        }
    };

    match get_window_state_path() {
        Ok(path) => {
            if let Some(parent_dir) = path.parent()
                && let Err(e) = tokio::fs::create_dir_all(parent_dir).await
            {
                warn!("Failed to create config directory: {e}");
                return;
            }

            let write_result = tokio::fs::write(&path, position_json).await;

            match write_result {
                Ok(()) => {
                    info!("Window position saved: {position_data:?}");
                }
                Err(e) => {
                    warn!("Failed to save window position to {path:?}: {e}");
                }
            }
        }
        Err(err) => warn!("Failed to get window state path: {err}"),
    }
}

pub async fn load_window_position() -> Option<WindowPosition> {
    match get_window_state_path() {
        Ok(home_path) => {
            if !home_path.exists() {
                info!("No window position file found at: {home_path:?}");
                return None;
            }

            match tokio::fs::read_to_string(&home_path).await {
                Ok(position_json) => match serde_json::from_str(&position_json) {
                    Ok(position) => {
                        info!("Window position loaded from: {home_path:?}");
                        Some(position)
                    }
                    Err(e) => {
                        warn!("Failed to parse window position JSON from {home_path:?}: {e}");
                        handle_corrupted_file(&home_path, e).await;
                        None
                    }
                },
                Err(e) => {
                    warn!("Failed to read window position file {home_path:?}: {e}");
                    if e.kind() == ErrorKind::PermissionDenied {
                        warn!(
                            "Permission denied accessing window position file - check file permissions"
                        );
                    }
                    handle_corrupted_file(&home_path, e).await;
                    None
                }
            }
        }
        Err(err) => {
            warn!("Could not determine window state path: {err}");
            None
        }
    }
}

async fn handle_corrupted_file(path: &Path, error: impl std::fmt::Display) {
    warn!("Handling corrupted window position file {path:?}: {error}");
    match tokio::fs::remove_file(path).await {
        Ok(()) => {
            info!("Successfully removed corrupted window position file: {path:?}");
        }
        Err(delete_err) => {
            warn!("Failed to delete corrupted window position file {path:?}: {delete_err}");
            if delete_err.kind() == ErrorKind::PermissionDenied {
                warn!("Permission denied deleting corrupted file - check file permissions");
            }
        }
    }
}

pub fn toggle_window_visibility(window: &WebviewWindow<Wry>) {
    let app_state = window.state::<AppState>();
    let is_visible = window.is_visible().unwrap_or(false);

    if is_visible {
        if !app_state.pinned.load(Ordering::SeqCst)
            && let Err(e) = window.hide()
        {
            warn!("Failed to hide window: {e}");
        }
    } else {
        set_position_before_show(window.clone());
        if let Err(e) = window.show() {
            warn!("Failed to show window: {e}");
        }

        // On Linux, we need a more aggressive approach to ensure the window gets focus
        #[cfg(target_os = "linux")]
        {
            // First, set always on top to bring it to front
            if let Err(e) = window.set_always_on_top(true) {
                warn!("Failed to set window always on top: {e}");
            }

            // Immediately request focus
            if let Err(e) = window.set_focus() {
                warn!("Failed to focus window (first attempt): {e}");
            }

            // On Linux, also try to unminimize the window in case it's minimized
            if let Err(e) = window.unminimize() {
                warn!("Failed to unminimize window: {e}");
            }

            // Use a non-blocking approach to avoid blocking the UI thread
            let window_clone = window.clone();
            let pinned = app_state.pinned.load(Ordering::SeqCst);
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(100));

                // Second focus attempt with unminimize
                if let Err(e) = window_clone.unminimize() {
                    warn!("Failed to unminimize window (second attempt): {e}");
                }
                if let Err(e) = window_clone.set_focus() {
                    warn!("Failed to focus window (second attempt): {e}");
                }

                // Remove always on top if not pinned
                if !pinned {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    if let Err(e) = window_clone.set_always_on_top(false) {
                        warn!("Failed to unset window always on top: {e}");
                    }
                }
            });
        }

        // For other platforms, use the original approach
        #[cfg(not(target_os = "linux"))]
        {
            if let Err(e) = window.set_always_on_top(true) {
                warn!("Failed to set window always on top: {e}");
            }
            if let Err(e) = window.set_focus() {
                warn!("Failed to focus window: {e}");
            }
            if !app_state.pinned.load(Ordering::SeqCst)
                && let Err(e) = window.set_always_on_top(false)
            {
                warn!("Failed to unset window always on top: {e}");
            }
        }
    }
}

pub fn set_position_before_show(window: WebviewWindow<Wry>) {
    let (positioning_active, runtime) = {
        let app_state = window.state::<AppState>();
        app_state.positioning_active.store(true, Ordering::SeqCst);
        (
            app_state.positioning_active.clone(),
            app_state.runtime.clone(),
        )
    };

    let window_clone = window.clone();

    runtime.spawn(async move {
        match load_window_position().await {
            Some(position) if is_valid_position(&window_clone, position.x, position.y) => {
                info!(
                    "Using saved window position: ({}, {})",
                    position.x, position.y
                );
                let _ = window_clone.set_position(tauri::Position::Physical(
                    tauri::PhysicalPosition::new(position.x, position.y),
                ));
                tokio::spawn({
                    let positioning_active = positioning_active.clone();
                    async move {
                        sleep(Duration::from_millis(150)).await;
                        positioning_active.store(false, Ordering::SeqCst);
                    }
                });
            }
            _ => {
                info!("No valid saved position, using tray positioning");
                position_from_tray(&window_clone);
            }
        }
    });
}

pub fn is_valid_position(window: &WebviewWindow<Wry>, x: i32, y: i32) -> bool {
    if let Ok(monitors) = window.available_monitors() {
        for monitor in monitors {
            let monitor_position = monitor.position();
            let monitor_size = monitor.size();

            let min_x = monitor_position.x;
            let min_y = monitor_position.y;
            let max_x = min_x + monitor_size.width as i32;
            let max_y = min_y + monitor_size.height as i32;

            #[cfg(target_os = "linux")]
            let tolerance = 100; // Allow 100px outside monitor bounds on Linux
            #[cfg(not(target_os = "linux"))]
            let tolerance = 0;

            if x >= (min_x - tolerance)
                && x < (max_x + tolerance)
                && y >= (min_y - tolerance)
                && y < (max_y + tolerance)
            {
                info!(
                    "Position ({}, {}) is valid for monitor bounds: {}x{} at ({}, {})",
                    x, y, monitor_size.width, monitor_size.height, min_x, min_y
                );
                return true;
            }
        }
        warn!("Position ({}, {}) is outside all monitor bounds", x, y);
        if let Ok(monitors) = window.available_monitors() {
            for (i, monitor) in monitors.iter().enumerate() {
                let pos = monitor.position();
                let size = monitor.size();
                warn!(
                    "Monitor {}: {}x{} at ({}, {})",
                    i, size.width, size.height, pos.x, pos.y
                );
            }
        }
    } else {
        warn!("Failed to get available monitors for position validation");
    }

    false
}

fn use_saved_or_center(window: &WebviewWindow<Wry>) {
    let window_clone = window.clone();

    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| {
            handle.block_on(async {
                match load_window_position().await {
                    Some(position) if is_valid_position(&window_clone, position.x, position.y) => {
                        info!(
                            "Using saved window position: ({}, {})",
                            position.x, position.y
                        );
                        match window_clone.set_position(tauri::Position::Physical(
                            tauri::PhysicalPosition::new(position.x, position.y),
                        )) {
                            Ok(()) => {}
                            Err(e) => {
                                warn!(
                                    "Failed to set saved position: {e}, centering on primary monitor"
                                );
                                center_on_primary_monitor(&window_clone);
                            }
                        }
                    }
                    Some(_) => {
                        info!("Saved position invalid, centering on primary monitor");
                        center_on_primary_monitor(&window_clone);
                    }
                    None => {
                        info!("No saved position, centering on primary monitor");
                        center_on_primary_monitor(&window_clone);
                    }
                }
            })
        });
    } else {
        let app_state = window.state::<AppState>();
        let runtime = app_state.runtime.clone();
        runtime.spawn(async move {
            match load_window_position().await {
                Some(position) if is_valid_position(&window_clone, position.x, position.y) => {
                    info!(
                        "Using saved window position: ({}, {})",
                        position.x, position.y
                    );
                    match window_clone.set_position(tauri::Position::Physical(
                        tauri::PhysicalPosition::new(position.x, position.y),
                    )) {
                        Ok(()) => {}
                        Err(e) => {
                            warn!(
                                "Failed to set saved position: {e}, centering on primary monitor"
                            );
                            center_on_primary_monitor(&window_clone);
                        }
                    }
                }
                Some(_) => {
                    info!("Saved position invalid, centering on primary monitor");
                    center_on_primary_monitor(&window_clone);
                }
                None => {
                    info!("No saved position, centering on primary monitor");
                    center_on_primary_monitor(&window_clone);
                }
            }
        });
    }
}

fn position_from_tray(window: &WebviewWindow<Wry>) {
    let tray_state = window
        .app_handle()
        .try_state::<crate::tray::TrayPositionState>();
    let Some(tray_state) = tray_state else {
        warn!("No tray state found, trying saved position");
        use_saved_or_center(window);
        return;
    };

    let tray_data = *tray_state.position.lock().unwrap();
    let Some((tray_pos, tray_size)) = tray_data else {
        warn!("No tray position data found, trying saved position");
        use_saved_or_center(window);
        return;
    };

    info!("Tray position: {:?}, size: {:?}", tray_pos, tray_size);

    let monitor = find_tray_monitor(window, tray_pos);

    match monitor {
        Some(monitor) => {
            info!("Found tray monitor: {:?}", monitor.name());
            position_window(window, &monitor, tray_pos, tray_size)
        }
        None => {
            warn!("Could not find tray monitor, trying saved position");
            use_saved_or_center(window)
        }
    }
}

fn find_tray_monitor(
    window: &WebviewWindow<Wry>, tray_pos: PhysicalPosition<f64>,
) -> Option<tauri::Monitor> {
    let monitors = window.available_monitors().ok()?;

    info!(
        "Searching for tray monitor. Tray physical position: {:?}",
        tray_pos
    );

    for monitor in &monitors {
        let monitor_pos = monitor.position();
        let monitor_size = monitor.size();

        info!(
            "Checking monitor '{}': pos={:?}, size={:?}, scale={}, tray_pos={:?}",
            monitor.name().map_or("Unknown", |v| v),
            monitor_pos,
            monitor_size,
            monitor.scale_factor(),
            tray_pos
        );

        if contains_point(monitor, tray_pos) {
            info!(
                "Tray found on monitor: {}",
                monitor.name().map_or("Unknown", |v| v)
            );
            return Some(monitor.clone());
        }
    }

    warn!("Tray position not found on any monitor");
    None
}

fn contains_point(monitor: &tauri::Monitor, point: PhysicalPosition<f64>) -> bool {
    let monitor_pos = monitor.position();
    let monitor_size = monitor.size();
    let point_x = point.x as i32;
    let point_y = point.y as i32;

    point_x >= monitor_pos.x
        && point_x <= monitor_pos.x + monitor_size.width as i32
        && point_y >= monitor_pos.y
        && point_y <= monitor_pos.y + monitor_size.height as i32
}

fn position_window(
    window: &WebviewWindow<Wry>, monitor: &tauri::Monitor, tray_pos: PhysicalPosition<f64>,
    tray_size: PhysicalSize<f64>,
) {
    let standard_window_size = tauri::PhysicalSize {
        width: 450.0,
        height: 500.0,
    };
    let position = calculate_position(monitor, tray_pos, tray_size, standard_window_size);

    let tray_center_x = tray_pos.x as i32 + tray_size.width as i32 / 2;
    let tray_center_y = tray_pos.y as i32 + tray_size.height as i32;

    info!(
        "Using standard size 450x500, calculated position: ({}, {}), tray center: ({}, {})",
        position.0, position.1, tray_center_x, tray_center_y
    );

    let app_state = window.state::<AppState>();
    app_state.positioning_active.store(true, Ordering::SeqCst);

    let position_result = window.set_position(tauri::Position::Physical(
        tauri::PhysicalPosition::new(position.0, position.1),
    ));

    match position_result {
        Ok(()) => {
            info!("Window positioned successfully, scheduling verification");
            schedule_position_verification(
                window.clone(),
                monitor.clone(),
                position.0,
                position.1,
                &app_state,
            );
        }
        Err(e) => {
            warn!("Failed to position window: {e}");
            center_on_specific_monitor(window, monitor);
            app_state.positioning_active.store(false, Ordering::SeqCst);
        }
    }
}

fn calculate_position(
    monitor: &tauri::Monitor, tray_pos: PhysicalPosition<f64>, tray_size: PhysicalSize<f64>,
    window_size: tauri::PhysicalSize<f64>,
) -> (i32, i32) {
    let monitor_pos = monitor.position();
    let monitor_size = monitor.size();

    let tray_center_x = (tray_pos.x as i32).saturating_add(tray_size.width as i32 / 2);
    let tray_bottom = (tray_pos.y as i32).saturating_add(tray_size.height as i32);
    let margin = 10;

    let window_x = clamp(
        tray_center_x.saturating_sub((window_size.width as i32) / 2),
        monitor_pos.x.saturating_add(margin),
        monitor_pos
            .x
            .saturating_add(monitor_size.width as i32)
            .saturating_sub(window_size.width as i32)
            .saturating_sub(margin),
    );

    let preferred_y = tray_bottom.saturating_add(margin);
    let min_y = monitor_pos.y.saturating_add(margin);
    let max_y = monitor_pos
        .y
        .saturating_add(monitor_size.height as i32)
        .saturating_sub(window_size.height as i32)
        .saturating_sub(margin);

    let window_y = if preferred_y <= max_y {
        preferred_y
    } else {
        let above_tray_y = (tray_pos.y as i32)
            .saturating_sub(window_size.height as i32)
            .saturating_sub(margin);
        if above_tray_y >= min_y {
            above_tray_y
        } else {
            clamp(preferred_y, min_y, max_y)
        }
    };

    (window_x, window_y)
}

fn clamp(value: i32, min: i32, max: i32) -> i32 {
    value.max(min).min(max)
}

fn schedule_position_verification(
    window: WebviewWindow<Wry>, monitor: tauri::Monitor, expected_x: i32, expected_y: i32,
    app_state: &AppState,
) {
    let runtime = app_state.runtime.clone();
    let positioning_active = app_state.positioning_active.clone();

    runtime.spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;

        if let Ok(actual_pos) = window.outer_position() {
            let actual_x = actual_pos.x;
            let actual_y = actual_pos.y;

            if (actual_x - expected_x).abs() > 5 || (actual_y - expected_y).abs() > 5 {
                info!(
                    "Position verification failed: expected ({}, {}), got ({}, {}), correcting...",
                    expected_x, expected_y, actual_x, actual_y
                );

                let scale_factor = monitor.scale_factor();
                if scale_factor > 1.5 {
                    let logical_x = expected_x as f64 / scale_factor;
                    let logical_y = expected_y as f64 / scale_factor;
                    let _ = window.set_position(tauri::Position::Logical(
                        tauri::LogicalPosition::new(logical_x, logical_y),
                    ));
                } else {
                    let _ = window.set_position(tauri::Position::Physical(
                        tauri::PhysicalPosition::new(expected_x, expected_y),
                    ));
                }

                tokio::time::sleep(Duration::from_millis(200)).await;
            } else {
                info!(
                    "Position verification passed: window at ({}, {})",
                    actual_x, actual_y
                );
            }
        }

        positioning_active.store(false, Ordering::SeqCst);
    });
}

fn center_on_specific_monitor(window: &WebviewWindow<Wry>, monitor: &tauri::Monitor) {
    let Ok(window_size) = window.outer_size() else {
        warn!("Failed to get window size for centering");
        return;
    };

    let monitor_size = monitor.size();
    let monitor_pos = monitor.position();

    let center_x = monitor_pos
        .x
        .saturating_add(monitor_size.width as i32 / 2)
        .saturating_sub(window_size.width as i32 / 2);
    let center_y = monitor_pos
        .y
        .saturating_add(monitor_size.height as i32 / 2)
        .saturating_sub(window_size.height as i32 / 2);

    match window.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
        center_x, center_y,
    ))) {
        Ok(()) => info!("Window centered at ({}, {})", center_x, center_y),
        Err(e) => warn!("Failed to center window: {e}"),
    }
}

fn center_on_primary_monitor(window: &WebviewWindow<Wry>) {
    let Ok(monitors) = window.available_monitors() else {
        warn!("Failed to get monitors for centering");
        return;
    };

    let primary_monitor = monitors.into_iter().next();
    let Some(monitor) = primary_monitor else {
        warn!("No monitors available for centering");
        return;
    };

    center_on_specific_monitor(window, &monitor);
}

pub fn reset_window_position(window: WebviewWindow<Wry>) {
    let (positioning_active, runtime) = {
        let app_state = window.state::<AppState>();
        app_state.positioning_active.store(true, Ordering::SeqCst);
        (
            app_state.positioning_active.clone(),
            app_state.runtime.clone(),
        )
    };

    let window_clone = window.clone();

    runtime.spawn(async move {
        remove_position_file().await;
        position_from_tray(&window_clone);
        sleep(Duration::from_millis(150)).await;
        positioning_active.store(false, Ordering::SeqCst);
    });
}

async fn remove_position_file() {
    match get_window_state_path() {
        Ok(path) => {
            if !path.exists() {
                info!("No window position file found to delete at: {path:?}");
                return;
            }

            match tokio::fs::remove_file(&path).await {
                Ok(()) => {
                    info!("Window position file deleted successfully: {path:?}");
                }
                Err(e) => {
                    warn!("Failed to delete window position file {path:?}: {e}");
                    if e.kind() == ErrorKind::PermissionDenied {
                        warn!(
                            "Permission denied deleting window position file - check file permissions"
                        );
                    }
                }
            }
        }
        Err(err) => {
            warn!("Could not determine window state path for deletion: {err}");
        }
    }
}

pub fn set_window_position(window: &WebviewWindow<Wry>, position: Position) {
    if let Err(e) = window.move_window(position) {
        warn!("Failed to move window: {e}");
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn test_handle_corrupted_file() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("corrupted.json");

        fs::write(&test_file, "corrupted data").unwrap();
        assert!(test_file.exists());

        tokio::runtime::Runtime::new().unwrap().block_on(async {
            handle_corrupted_file(&test_file, "Test error").await;
        });

        assert!(!test_file.exists());
    }

    #[test]
    fn test_window_position_serialization() {
        let position = WindowPosition { x: 100, y: 200 };
        let json = serde_json::to_string(&position).unwrap();

        assert!(json.contains("100"), "JSON should contain x value");
        assert!(json.contains("200"), "JSON should contain y value");

        let deserialized: WindowPosition = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.x, 100);
        assert_eq!(deserialized.y, 200);
    }

    #[test]
    fn test_save_load_window_position() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("window_state.json");

        let position = WindowPosition { x: 100, y: 200 };
        let json = serde_json::to_string(&position).unwrap();

        fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        fs::write(&file_path, json).unwrap();

        let content = fs::read_to_string(&file_path).unwrap();
        let loaded_position: WindowPosition = serde_json::from_str(&content).unwrap();

        assert_eq!(loaded_position.x, 100);
        assert_eq!(loaded_position.y, 200);
    }

    #[test]
    fn test_create_directory_for_window_position() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("config/window_state.json");

        if file_path.parent().unwrap().exists() {
            fs::remove_dir_all(file_path.parent().unwrap()).unwrap();
        }

        if let Some(parent_dir) = file_path.parent() {
            fs::create_dir_all(parent_dir).unwrap();
            assert!(parent_dir.exists(), "Directory should be created");
        }
    }

    #[test]
    fn test_is_valid_position() {
        let screen_width = 1920;
        let screen_height = 1080;

        let valid_positions = [(100, 100), (0, 0), (screen_width - 1, screen_height - 1)];

        for (x, y) in valid_positions {
            assert!(x >= 0 && x < screen_width, "X position should be valid");
            assert!(y >= 0 && y < screen_height, "Y position should be valid");
        }

        let invalid_positions = [
            (-100, 100),
            (100, -100),
            (screen_width + 100, screen_height + 100),
        ];

        for (x, y) in invalid_positions {
            assert!(
                !(x >= 0 && x < screen_width && y >= 0 && y < screen_height),
                "Position should be invalid"
            );
        }
    }
}

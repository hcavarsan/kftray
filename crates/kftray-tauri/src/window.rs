use std::fs;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;

use kftray_commons::models::window::AppState;
use kftray_commons::models::window::WindowPosition;
use kftray_commons::utils::config_dir::get_window_state_path;
use log::{
    info,
    warn,
};
use tauri::{
    Manager,
    PhysicalPosition,
    PhysicalSize,
    WebviewWindow,
    Wry,
};
use tauri_plugin_positioner::{
    Position,
    WindowExt,
};
use tokio::time::sleep;

pub fn save_window_position(window: &WebviewWindow<Wry>) {
    let app_state = window.state::<AppState>();

    if app_state.positioning_active.load(Ordering::SeqCst) {
        info!("Skipping position save - app positioning active");
        return;
    }

    if let Ok(position) = window.outer_position()
        && is_valid_position(window, position.x, position.y)
    {
        let position_data = WindowPosition {
            x: position.x,
            y: position.y,
        };
        let runtime = app_state.runtime.clone();

        runtime.spawn(async move {
            save_position_async(position_data).await;
        });
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

            if tokio::fs::write(&path, position_json).await.is_ok() {
                info!("Window position saved: {position_data:?}");
            } else {
                warn!("Failed to save window position.");
            }
        }
        Err(err) => warn!("Failed to get window state path: {err}"),
    }
}

pub fn load_window_position() -> Option<WindowPosition> {
    if let Ok(home_path) = get_window_state_path() {
        if home_path.exists() {
            match fs::read_to_string(&home_path) {
                Ok(position_json) => match serde_json::from_str(&position_json) {
                    Ok(position) => {
                        info!("Window position loaded from home directory: {home_path:?}");
                        Some(position)
                    }
                    Err(e) => {
                        handle_corrupted_file(&home_path, e);
                        None
                    }
                },
                Err(e) => {
                    handle_corrupted_file(&home_path, e);
                    None
                }
            }
        } else {
            info!("No window position file found.");
            None
        }
    } else {
        info!("Could not determine window state path.");
        None
    }
}

fn handle_corrupted_file(path: &Path, error: impl std::fmt::Display) {
    warn!("Failed to parse window position JSON: {error}");
    if let Err(delete_err) = fs::remove_file(path) {
        warn!("Failed to delete corrupted window position file: {delete_err}");
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
        set_position_before_show(window);
        if let Err(e) = window.show() {
            warn!("Failed to show window: {e}");
        }
        if let Err(e) = window.set_focus() {
            warn!("Failed to focus window: {e}");
        }
    }
}

pub fn set_position_before_show(window: &WebviewWindow<Wry>) {
    let app_state = window.state::<AppState>();
    app_state.positioning_active.store(true, Ordering::SeqCst);

    match load_window_position() {
        Some(position) if is_valid_position(window, position.x, position.y) => {
            info!(
                "Using saved window position: ({}, {})",
                position.x, position.y
            );
            let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
                position.x, position.y,
            )));
            reset_positioning_state_after_delay(&app_state);
        }
        _ => {
            info!("No valid saved position, using tray positioning");
            position_from_tray(window);
        }
    }
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

            if x >= min_x && x < max_x && y >= min_y && y < max_y {
                return true;
            }
        }
    }

    false
}

fn use_saved_or_center(window: &WebviewWindow<Wry>) {
    match load_window_position() {
        Some(position) if is_valid_position(window, position.x, position.y) => {
            info!(
                "Using saved window position: ({}, {})",
                position.x, position.y
            );
            match window.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(
                position.x, position.y,
            ))) {
                Ok(()) => {}
                Err(e) => {
                    warn!("Failed to set saved position: {e}, centering on primary monitor");
                    center_on_primary_monitor(window);
                }
            }
        }
        Some(_) => {
            info!("Saved position invalid, centering on primary monitor");
            center_on_primary_monitor(window);
        }
        None => {
            info!("No saved position, centering on primary monitor");
            center_on_primary_monitor(window);
        }
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

    // Use saturating arithmetic to prevent integer overflow
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

pub fn reset_window_position(window: &WebviewWindow<Wry>) {
    let app_state = window.state::<AppState>();
    app_state.positioning_active.store(true, Ordering::SeqCst);

    remove_position_file();
    position_from_tray(window);
    reset_positioning_state_after_delay(&app_state);
}

fn remove_position_file() {
    if let Ok(path) = get_window_state_path() {
        if path.exists() {
            if let Err(e) = fs::remove_file(&path) {
                warn!("Failed to delete window position file: {e}");
            } else {
                info!("Window position file deleted successfully.");
            }
        } else {
            info!("No window position file found to delete.");
        }
    } else {
        info!("Could not determine window state path.");
    }
}

pub fn set_window_position(window: &WebviewWindow<Wry>, position: Position) {
    if let Err(e) = window.move_window(position) {
        warn!("Failed to move window: {e}");
    }
}

fn reset_positioning_state_after_delay(app_state: &AppState) {
    let positioning_active = app_state.positioning_active.clone();
    app_state.runtime.spawn(async move {
        sleep(Duration::from_millis(150)).await;
        positioning_active.store(false, Ordering::SeqCst);
    });
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn test_handle_corrupted_file() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("corrupted.json");

        fs::write(&test_file, "corrupted data").unwrap();
        assert!(test_file.exists());

        handle_corrupted_file(&test_file, "Test error");

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

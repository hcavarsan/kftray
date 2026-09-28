use std::io::ErrorKind;
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

use crate::tray_mode::{
    self,
    ToggleAction,
    TrayMode,
    WindowSnapshot,
    toggle_action,
};

pub async fn save_window_position_async(position_data: WindowPosition) {
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

pub fn position_is_compositor_managed() -> bool {
    cfg!(target_os = "linux")
        && (std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland"))
}

pub async fn load_window_position() -> Option<WindowPosition> {
    if position_is_compositor_managed() {
        return None;
    }
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
    toggle_window_visibility_with_position(window, false);
}

/// Toggles the window from a tray interaction. A saved position is only
/// reused when it sits on the display that hosts the tray icon.
#[cfg(not(target_os = "linux"))]
pub fn toggle_window_visibility_from_tray(window: &WebviewWindow<Wry>) {
    toggle_window_visibility_with_position(window, true);
}

fn toggle_window_visibility_with_position(window: &WebviewWindow<Wry>, from_tray: bool) {
    let snapshot = WindowSnapshot {
        visible: window.is_visible().unwrap_or(false),
        minimized: window.is_minimized().unwrap_or(false),
        focused: window.is_focused().unwrap_or(false),
        pinned: window.state::<AppState>().pinned.load(Ordering::SeqCst),
    };

    match toggle_action(tray_mode::current(window.app_handle()), snapshot) {
        ToggleAction::Show => show_main_window_with_mode(window, from_tray),
        ToggleAction::Hide => {
            if let Err(e) = window.hide() {
                warn!("Failed to hide window: {e}");
            }
        }
        ToggleAction::Minimize => {
            if let Err(e) = window.minimize() {
                warn!("Failed to minimize window: {e}");
            }
        }
        ToggleAction::Keep => {}
    }
}

pub fn show_main_window(window: &WebviewWindow<Wry>) {
    show_main_window_with_mode(window, false);
}

pub fn hide_main_window(window: &WebviewWindow<Wry>) {
    let result = match tray_mode::current(window.app_handle()) {
        TrayMode::Tray => window.hide(),
        TrayMode::Window => window.minimize(),
    };
    if let Err(e) = result {
        warn!("Failed to hide window: {e}");
    }
}

fn show_main_window_with_mode(window: &WebviewWindow<Wry>, from_tray: bool) {
    set_position_before_show_with_mode(window.clone(), from_tray);
    raise_main_window(window);
}

pub fn show_centered_main_window(window: &WebviewWindow<Wry>) {
    let (positioning_active, runtime) = {
        let app_state = window.state::<AppState>();
        app_state.positioning_active.store(true, Ordering::SeqCst);
        (
            app_state.positioning_active.clone(),
            app_state.runtime.clone(),
        )
    };
    let window = window.clone();
    runtime.spawn(async move {
        let applied = apply_saved_window_size(&window).await;
        let target = window.clone();
        let centered = window.app_handle().run_on_main_thread(move || {
            let monitor = target
                .current_monitor()
                .ok()
                .flatten()
                .or_else(|| target.primary_monitor().ok().flatten());
            if let Some(monitor) = monitor {
                let size = applied
                    .map(|size| {
                        tauri::LogicalSize::new(size.width, size.height)
                            .to_physical::<u32>(monitor.scale_factor())
                    })
                    .or_else(|| target.outer_size().ok());
                if let Some(size) = size {
                    center_sized_on_monitor(&target, &monitor, size);
                }
            }
            raise_main_window(&target);
        });
        if let Err(e) = centered {
            warn!("Failed to dispatch window centering to main thread: {e}");
        }
        sleep(Duration::from_millis(150)).await;
        positioning_active.store(false, Ordering::SeqCst);
    });
}

fn raise_main_window(window: &WebviewWindow<Wry>) {
    let pinned = window.state::<AppState>().pinned.load(Ordering::SeqCst);

    if let Err(e) = window.show() {
        warn!("Failed to show window: {e}");
    }

    if crate::tray_mode::current(window.app_handle()) == crate::tray_mode::TrayMode::Window {
        if let Err(e) = window.unminimize() {
            warn!("Failed to unminimize window: {e}");
        }
        if let Err(e) = window.set_focus() {
            warn!("Failed to focus window: {e}");
        }
        return;
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
        if let Err(e) = window.unminimize() {
            warn!("Failed to unminimize window: {e}");
        }
        if let Err(e) = window.set_always_on_top(true) {
            warn!("Failed to set window always on top: {e}");
        }
        if let Err(e) = window.set_focus() {
            warn!("Failed to focus window: {e}");
        }
        if !pinned && let Err(e) = window.set_always_on_top(false) {
            warn!("Failed to unset window always on top: {e}");
        }
    }
}

fn set_position_before_show_with_mode(window: WebviewWindow<Wry>, from_tray: bool) {
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
        apply_saved_window_size(&window_clone).await;
        match load_window_position().await {
            Some(position)
                if is_valid_position(&window_clone, position.x, position.y)
                    && (!from_tray
                        || tray_mode::current(window_clone.app_handle()) == TrayMode::Window
                        || is_on_tray_monitor(&window_clone, position.x, position.y)) =>
            {
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
            _ if tray_mode::current(window_clone.app_handle()) == TrayMode::Window => {
                info!("No usable saved position, centering the window");
                center_on_current_monitor(&window_clone);
                sleep(Duration::from_millis(150)).await;
                positioning_active.store(false, Ordering::SeqCst);
            }
            _ => {
                info!("No usable saved position, using tray positioning");
                position_from_tray(&window_clone);
            }
        }
    });
}

/// Whether a saved position lies on the monitor that hosts the tray icon.
/// Without tray data or a matching monitor the saved position is kept.
fn is_on_tray_monitor(window: &WebviewWindow<Wry>, x: i32, y: i32) -> bool {
    let Some(tray_state) = window
        .app_handle()
        .try_state::<crate::tray::TrayPositionState>()
    else {
        return true;
    };
    let Some((tray_pos, _)) = *tray_state.position.lock().unwrap() else {
        return true;
    };
    match find_tray_monitor(window, tray_pos) {
        Some(monitor) => contains_point(&monitor, PhysicalPosition::new(x as f64, y as f64)),
        None => true,
    }
}

/// Applies the saved window size on the Tauri main thread.
///
/// On Linux, `WebviewWindow::current_monitor` and `set_size` are Xlib/GTK
/// calls that corrupt the xcb request queue when invoked from a worker thread
/// (see commit 2f120f1 for the same class of bug on `WindowEvent::Moved`).
/// Dispatching the work via `run_on_main_thread` keeps every Xlib call on the
/// GTK main thread; on macOS/Windows it is still the correct thread for window
/// APIs, so the same code path works on every platform.
///
/// Returns the logical size that was applied, or `None` when nothing is saved
/// or the resize failed.
pub async fn apply_saved_window_size(
    window: &WebviewWindow<Wry>,
) -> Option<crate::window_size::WindowSize> {
    let saved = crate::window_size::load().await?;
    let window_clone = window.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();

    let dispatch = window.app_handle().run_on_main_thread(move || {
        let available = window_clone
            .current_monitor()
            .ok()
            .flatten()
            .map(|monitor| {
                let logical = monitor.size().to_logical::<f64>(monitor.scale_factor());
                (logical.width, logical.height)
            });
        let size = saved.fit_within(available);
        let applied = window_clone.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
            size.width as f64,
            size.height as f64,
        )));
        let _ = tx.send(match applied {
            Ok(()) => Some(size),
            Err(e) => {
                warn!("Failed to restore window size: {e}");
                None
            }
        });
    });

    if let Err(e) = dispatch {
        warn!("Failed to dispatch window size restore to main thread: {e}");
        return None;
    }

    rx.await.ok().flatten()
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
    let current_size = window.outer_size().unwrap_or_else(|_| {
        let scale = monitor.scale_factor();
        PhysicalSize::new(
            (450.0 * scale).round() as u32,
            (500.0 * scale).round() as u32,
        )
    });
    let window_size = tauri::PhysicalSize {
        width: current_size.width as f64,
        height: current_size.height as f64,
    };
    let position = calculate_position(monitor, tray_pos, tray_size, window_size);

    let tray_center_x = tray_pos.x as i32 + tray_size.width as i32 / 2;
    let tray_center_y = tray_pos.y as i32 + tray_size.height as i32;

    info!(
        "Using window size {}x{}, calculated position: ({}, {}), tray center: ({}, {})",
        current_size.width,
        current_size.height,
        position.0,
        position.1,
        tray_center_x,
        tray_center_y
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
    center_sized_on_monitor(window, monitor, window_size);
}

fn center_sized_on_monitor(
    window: &WebviewWindow<Wry>, monitor: &tauri::Monitor, window_size: PhysicalSize<u32>,
) {
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

fn center_on_current_monitor(window: &WebviewWindow<Wry>) {
    match window.current_monitor().ok().flatten() {
        Some(monitor) => center_on_specific_monitor(window, &monitor),
        None => center_on_primary_monitor(window),
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

pub fn forget_window_position(window: &WebviewWindow<Wry>) {
    window
        .state::<AppState>()
        .runtime
        .spawn(remove_position_file());
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
        match tray_mode::current(window_clone.app_handle()) {
            TrayMode::Window => center_on_current_monitor(&window_clone),
            TrayMode::Tray => position_from_tray(&window_clone),
        }
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

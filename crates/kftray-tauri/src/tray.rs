use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{
    Arc,
    Mutex,
};
use std::time::Duration;

use kftray_commons::models::window::AppState;
use kftray_commons::models::window::SaveDialogState;
use log::{
    error,
    info,
};
use tauri::{
    Manager,
    PhysicalPosition,
    PhysicalSize,
    RunEvent,
    WindowEvent,
    Wry,
    menu::{
        MenuBuilder,
        MenuItemBuilder,
        PredefinedMenuItem,
        SubmenuBuilder,
    },
    tray::{
        MouseButton,
        MouseButtonState,
        TrayIconBuilder,
        TrayIconEvent,
    },
};
use tauri_plugin_positioner::Position;
use tokio::time::sleep;

type TrayPosition = Option<(PhysicalPosition<f64>, PhysicalSize<f64>)>;

#[derive(Default)]
pub struct TrayPositionState {
    pub position: Arc<Mutex<TrayPosition>>,
}

use crate::commands::portforward::handle_exit_app;
use crate::commands::window_state::toggle_pin_state;
use crate::window::{
    adjust_window_size_and_position,
    reset_window_position,
    save_window_position,
    set_default_position,
    set_window_position,
    toggle_window_visibility,
};

/// Check if the application is running inside Flatpak
#[allow(dead_code)]
pub fn is_flatpak_environment() -> bool {
    std::env::var("FLATPAK").is_ok()
}

/// Get the appropriate tray icon directory path for the current environment
pub fn get_tray_icon_path(
    _app_handle: &tauri::AppHandle<Wry>,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    {
        if is_flatpak_environment() {
            let local_data_path = _app_handle
                .path()
                .app_local_data_dir()
                .map_err(|e| format!("Failed to get app local data dir: {}", e))?
                .join("tray-icon");

            std::fs::create_dir_all(&local_data_path)
                .map_err(|e| format!("Failed to create tray icon directory: {}", e))?;

            return Ok(local_data_path);
        }
    }

    let temp_path = std::env::temp_dir().join("kftray-tray-icons");
    std::fs::create_dir_all(&temp_path)
        .map_err(|e| format!("Failed to create temp tray icon directory: {}", e))?;
    Ok(temp_path)
}

/// Update tray icon with Flatpak compatibility
/// This function ensures tray icons are stored in the correct location for
/// sandbox environments
///
/// # Usage
/// ```rust
/// // Update tray icon dynamically with Flatpak support
/// let icon_path = PathBuf::from("path/to/icon.png");
/// let icon_image = tauri::image::Image::from_path(icon_path)?;
/// update_tray_icon_with_flatpak_support(&tray, icon_image, app_handle)?;
/// ```
#[allow(dead_code)]
pub fn update_tray_icon_with_flatpak_support(
    tray: &tauri::tray::TrayIcon<Wry>, icon_image: tauri::image::Image,
    app_handle: &tauri::AppHandle<Wry>,
) -> Result<(), Box<dyn std::error::Error>> {
    let tray_icon_path = get_tray_icon_path(app_handle)?;

    #[cfg(target_os = "linux")]
    {
        if is_flatpak_environment() {
            info!(
                "Setting tray icon temp directory for Flatpak: {:?}",
                tray_icon_path
            );
        }
    }

    tray.set_temp_dir_path(Some(tray_icon_path))
        .map_err(|e| format!("Failed to set tray temp dir: {}", e))?;

    tray.set_icon(Some(icon_image))
        .map_err(|e| format!("Failed to set tray icon: {}", e))?;

    Ok(())
}

/// Create a dynamically generated tray icon with Flatpak support
/// This is useful for cases where you need to generate tray icons at runtime
///
/// # Usage
/// ```rust
/// // Create a dynamic tray icon (e.g., with status indicators)
/// let icon_bytes = generate_dynamic_icon_bytes(); // Your icon generation logic
/// let icon_image = tauri::image::Image::from_bytes(&icon_bytes)?;
/// set_dynamic_tray_icon(&tray, icon_image, app_handle)?;
/// ```
#[allow(dead_code)]
pub fn set_dynamic_tray_icon(
    tray: &tauri::tray::TrayIcon<Wry>, icon_image: tauri::image::Image,
    app_handle: &tauri::AppHandle<Wry>,
) -> Result<(), Box<dyn std::error::Error>> {
    update_tray_icon_with_flatpak_support(tray, icon_image, app_handle)
}

pub fn create_tray_icon(app: &tauri::App<Wry>) -> Result<tauri::tray::TrayIcon<Wry>, tauri::Error> {
    let quit = MenuItemBuilder::with_id("quit", "Quit")
        .accelerator("CmdOrCtrl+Shift+Q")
        .build(app)?;
    let open = MenuItemBuilder::with_id("toggle", "Toggle App").build(app)?;
    let pin = MenuItemBuilder::with_id("pin", "Pin Window").build(app)?;

    let set_center_position =
        MenuItemBuilder::with_id("set_center_position", "Center").build(app)?;
    let set_top_right_position =
        MenuItemBuilder::with_id("set_top_right_position", "Top Right").build(app)?;
    let set_bottom_right_position =
        MenuItemBuilder::with_id("set_bottom_right_position", "Bottom Right").build(app)?;
    let set_bottom_left_position =
        MenuItemBuilder::with_id("set_bottom_left_position", "Bottom Left").build(app)?;
    let set_top_left_position =
        MenuItemBuilder::with_id("set_top_left_position", "Top Left").build(app)?;

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let set_traycenter_position =
        MenuItemBuilder::with_id("set_traycenter_position", "System Tray Center").build(app)?;

    let mut position_menu_items = vec![
        &set_center_position,
        &set_top_right_position,
        &set_bottom_right_position,
        &set_bottom_left_position,
        &set_top_left_position,
    ];

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    position_menu_items.push(&set_traycenter_position);

    let reset_position = MenuItemBuilder::with_id("reset_position", "Reset Position").build(app)?;

    let separator = PredefinedMenuItem::separator(app)?;
    let mut position_submenu_builder = SubmenuBuilder::new(app, "Set Window Position");

    for item in position_menu_items {
        position_submenu_builder = position_submenu_builder.item(item);
    }

    let set_window_position_submenu = position_submenu_builder
        .item(&separator)
        .item(&reset_position)
        .build()?;

    let main_separator = PredefinedMenuItem::separator(app)?;
    let menu = MenuBuilder::new(app)
        .item(&open)
        .item(&main_separator)
        .item(&pin)
        .item(&set_window_position_submenu)
        .item(&quit)
        .build()?;
    let icon_bytes = include_bytes!("../icons/tray.ico");
    let icon = tauri::image::Image::from_bytes(icon_bytes)?;

    let mut tray_builder = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(false);

    // Set the temp directory for Flatpak compatibility before setting the icon
    if let Ok(tray_icon_path) = get_tray_icon_path(&app.handle()) {
        #[cfg(target_os = "linux")]
        {
            if is_flatpak_environment() {
                info!(
                    "Configuring tray icon temp directory for Flatpak: {:?}",
                    tray_icon_path
                );
                tray_builder = tray_builder.temp_dir_path(tray_icon_path);
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            tray_builder = tray_builder.temp_dir_path(tray_icon_path);
        }
    }

    let tray = tray_builder
        .icon(icon)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "quit" => {
                tauri::async_runtime::block_on(handle_exit_app(app.clone()));
            }
            "toggle" => match app.get_webview_window("main") {
                Some(window) => {
                    toggle_window_visibility(&window);
                }
                _ => {
                    error!("Main window not found on menu event");
                }
            },
            "reset_position" => {
                if let Some(window) = app.get_webview_window("main") {
                    reset_window_position(&window);
                }
            }
            "set_center_position" => {
                if let Some(window) = app.get_webview_window("main") {
                    set_window_position(&window, Position::Center);
                }
            }
            "set_top_right_position" => {
                if let Some(window) = app.get_webview_window("main") {
                    set_window_position(&window, Position::TopRight);
                }
            }
            "set_bottom_right_position" => {
                if let Some(window) = app.get_webview_window("main") {
                    set_window_position(&window, Position::BottomRight);
                }
            }
            "set_bottom_left_position" => {
                if let Some(window) = app.get_webview_window("main") {
                    set_window_position(&window, Position::BottomLeft);
                }
            }
            "set_top_left_position" => {
                if let Some(window) = app.get_webview_window("main") {
                    set_window_position(&window, Position::TopLeft);
                }
            }
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            "set_traycenter_position" => {
                if let Some(window) = app.get_webview_window("main") {
                    set_window_position(&window, Position::TrayCenter);
                }
            }
            "pin" => {
                if let Some(window) = app.get_webview_window("main") {
                    toggle_pin_state(app.state::<AppState>(), window);
                }
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);

            match &event {
                TrayIconEvent::Click { rect, .. }
                | TrayIconEvent::Enter { rect, .. }
                | TrayIconEvent::Leave { rect, .. }
                | TrayIconEvent::Move { rect, .. } => {
                    let scale_factor = tray
                        .app_handle()
                        .get_webview_window("main")
                        .and_then(|w| w.current_monitor().ok()?)
                        .map(|m| m.scale_factor())
                        .unwrap_or(1.0);

                    let size = rect.size.to_physical(scale_factor);
                    let position = rect.position.to_physical(scale_factor);

                    if let Some(tray_state) = tray.app_handle().try_state::<TrayPositionState>() {
                        *tray_state.position.lock().unwrap() = Some((position, size));
                    }
                }
                _ => {}
            }

            match event {
                TrayIconEvent::Click {
                    button,
                    button_state,
                    ..
                } => {
                    info!(
                        "Click event - button: {:?}, state: {:?}",
                        button, button_state
                    );
                    if button == MouseButton::Left && button_state == MouseButtonState::Up {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            toggle_window_visibility(&window);
                        }
                    }
                }
                TrayIconEvent::DoubleClick { button, .. } => {
                    info!("Double click event - button: {:?}", button);
                    if button == MouseButton::Left {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            toggle_window_visibility(&window);
                        }
                    }
                }
                _ => {}
            }
        })
        .build(app)?;

    Ok(tray)
}

pub fn handle_window_event(window: &tauri::Window<Wry>, event: &WindowEvent) {
    // Get the webview window for window management operations
    let webview_window = match window.app_handle().get_webview_window(window.label()) {
        Some(webview_window) => webview_window,
        _ => {
            error!("Failed to get webview window for label: {}", window.label());
            return;
        }
    };

    if let WindowEvent::ScaleFactorChanged {
        scale_factor,
        new_inner_size,
        ..
    } = event
    {
        adjust_window_size_and_position(&webview_window, *scale_factor, *new_inner_size);
        let is_visible = webview_window.is_visible().unwrap_or(false);
        let is_focused = webview_window.is_focused().unwrap_or(false);

        if !is_visible || !is_focused {
            set_default_position(&webview_window);
            if let Err(e) = webview_window.show() {
                error!("Failed to show window: {e}");
            }
            if let Err(e) = webview_window.set_focus() {
                error!("Failed to focus window: {e}");
            }
        }

        return;
    }

    info!("event: {:?}", event);
    let app_state = webview_window.state::<AppState>();
    let is_moving = app_state.is_moving.lock().unwrap();

    if let WindowEvent::Focused(is_focused) = event
        && !is_focused
        && !*is_moving
        && !app_state.is_pinned.load(Ordering::SeqCst)
    {
        let app_handle = webview_window.app_handle();

        if let Some(state) = app_handle.try_state::<SaveDialogState>()
            && !state.is_open.load(Ordering::SeqCst)
        {
            let webview_window_clone = webview_window.clone();
            let app_handle_clone = app_handle.clone();
            let runtime = app_state.runtime.clone();
            runtime.spawn(async move {
                sleep(Duration::from_millis(200)).await;
                if let Some(main_window) = app_handle_clone.get_webview_window("main") {
                    if !main_window.is_focused().unwrap_or(false) {
                        if let Err(e) = webview_window_clone.hide() {
                            error!("Failed to hide window: {e}");
                        }
                    }
                }
            });
        }
    }

    if let WindowEvent::Moved(_) = event {
        #[warn(unused_must_use)]
        let _ = webview_window.with_webview(|_webview| {
            #[cfg(target_os = "linux")]
            {}

            #[cfg(windows)]
            unsafe {
                // https://github.com/MicrosoftEdge/WebView2Feedback/issues/780#issuecomment-808306938
                // https://docs.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller?view=webview2-1.0.774.44#notifyparentwindowpositionchanged
                _webview
                    .controller()
                    .NotifyParentWindowPositionChanged()
                    .unwrap();
            }

            #[cfg(target_os = "macos")]
            {}
        });

        if let Ok(mut moving_guard) = app_state.is_moving.try_lock() {
            if !*moving_guard && !app_state.is_plugin_moving.load(Ordering::SeqCst) {
                info!(
                    "is_plugin_moving: {}",
                    app_state.is_plugin_moving.load(Ordering::SeqCst)
                );
                *moving_guard = true;

                drop(moving_guard);
                save_window_position(&webview_window);

                if let Ok(mut moving_guard) = app_state.is_moving.try_lock() {
                    *moving_guard = false;
                }
            }
        }
    }

    if let WindowEvent::CloseRequested { api, .. } = event
        && !app_state.is_pinned.load(Ordering::SeqCst)
    {
        api.prevent_close();
        // Call the same exit logic as tray quit to handle active port forwards
        let app_handle = webview_window.app_handle();
        tauri::async_runtime::block_on(handle_exit_app(app_handle.clone()));
    }
}

pub fn handle_run_event(app_handle: &tauri::AppHandle<Wry>, event: RunEvent) {
    match event {
        RunEvent::ExitRequested { ref api, .. } => {
            api.prevent_exit();
            tauri::async_runtime::block_on(handle_exit_app(app_handle.clone()));
        }
        RunEvent::Exit => {
            tauri::async_runtime::block_on(handle_exit_app(app_handle.clone()));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use kftray_commons::models::window::{
        AppState,
        SaveDialogState,
    };
    use tokio::runtime::Runtime;

    #[test]
    fn test_app_pin_state() {
        let app_state = AppState {
            is_moving: Arc::new(std::sync::Mutex::new(false)),
            is_plugin_moving: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            is_pinned: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            runtime: Arc::new(Runtime::new().unwrap()),
        };

        assert!(
            !app_state
                .is_pinned
                .load(std::sync::atomic::Ordering::SeqCst),
            "App should start unpinned"
        );

        let initial_state = app_state
            .is_pinned
            .load(std::sync::atomic::Ordering::SeqCst);
        app_state
            .is_pinned
            .store(!initial_state, std::sync::atomic::Ordering::SeqCst);

        assert!(
            app_state
                .is_pinned
                .load(std::sync::atomic::Ordering::SeqCst),
            "App should be pinned after toggle"
        );

        let current_state = app_state
            .is_pinned
            .load(std::sync::atomic::Ordering::SeqCst);
        app_state
            .is_pinned
            .store(!current_state, std::sync::atomic::Ordering::SeqCst);

        assert!(
            !app_state
                .is_pinned
                .load(std::sync::atomic::Ordering::SeqCst),
            "App should be unpinned after second toggle"
        );
    }

    #[test]
    fn test_save_dialog_state() {
        let save_dialog_state = SaveDialogState::default();

        assert!(
            !save_dialog_state
                .is_open
                .load(std::sync::atomic::Ordering::SeqCst),
            "Dialog should start closed"
        );

        save_dialog_state
            .is_open
            .store(true, std::sync::atomic::Ordering::SeqCst);

        assert!(
            save_dialog_state
                .is_open
                .load(std::sync::atomic::Ordering::SeqCst),
            "Dialog should be open after setting to true"
        );

        save_dialog_state
            .is_open
            .store(false, std::sync::atomic::Ordering::SeqCst);

        assert!(
            !save_dialog_state
                .is_open
                .load(std::sync::atomic::Ordering::SeqCst),
            "Dialog should be closed after setting to false"
        );
    }

    #[test]
    fn test_flatpak_environment_detection() {
        // Test when FLATPAK environment variable is not set
        std::env::remove_var("FLATPAK");
        assert!(
            !super::is_flatpak_environment(),
            "Should not detect Flatpak when FLATPAK env var is not set"
        );

        // Test when FLATPAK environment variable is set
        std::env::set_var("FLATPAK", "1");
        assert!(
            super::is_flatpak_environment(),
            "Should detect Flatpak when FLATPAK env var is set"
        );

        // Clean up
        std::env::remove_var("FLATPAK");
    }
}

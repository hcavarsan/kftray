use std::sync::atomic::Ordering;
use std::sync::{
    Arc,
    Mutex,
};
use std::time::Duration;

use kftray_commons::models::window::AppState;
use kftray_commons::models::window::SaveDialogState;
use kftray_commons::models::window::WindowPosition;
use log::{
    error,
    info,
    warn,
};
use tauri::PhysicalPosition;
use tauri::PhysicalSize;
use tauri::{
    Manager,
    RunEvent,
    WindowEvent,
    Wry,
};
#[cfg(not(target_os = "linux"))]
use tauri::{
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
#[cfg(not(target_os = "linux"))]
use tauri_plugin_positioner::Position;
use tokio::time::sleep;

type TrayPosition = Option<(PhysicalPosition<f64>, PhysicalSize<f64>)>;

#[derive(Default)]
pub(crate) struct TrayPositionState {
    pub position: Arc<Mutex<TrayPosition>>,
}

use crate::commands::portforward::handle_exit_app;
#[cfg(not(target_os = "linux"))]
use crate::commands::window_state::toggle_pin_state;
use crate::window::{
    is_valid_position,
    save_window_position_async,
};
#[cfg(not(target_os = "linux"))]
use crate::window::{
    reset_window_position,
    set_window_position,
    toggle_window_visibility,
};

#[cfg(not(target_os = "linux"))]
pub(crate) const TRAY_ID: &str = "kftray-main";

#[cfg(target_os = "windows")]
const TRAY_LIGHT_ICO: &[u8] = include_bytes!("../icons/tray-light.ico");
#[cfg(not(target_os = "linux"))]
const TRAY_DARK_ICO: &[u8] = include_bytes!("../icons/tray-dark.ico");

#[cfg(not(target_os = "linux"))]
const fn current_tray_icon_bytes() -> &'static [u8] {
    #[cfg(target_os = "windows")]
    {
        match crate::tray_theme::current() {
            crate::tray_theme::TaskbarTheme::Dark => TRAY_LIGHT_ICO,
            crate::tray_theme::TaskbarTheme::Light => TRAY_DARK_ICO,
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        TRAY_DARK_ICO
    }
}

#[cfg(target_os = "windows")]
pub fn refresh_tray_icon_for_theme(app: &tauri::AppHandle<Wry>) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    match tauri::image::Image::from_bytes(current_tray_icon_bytes()) {
        Ok(icon) => {
            if let Err(e) = tray.set_icon(Some(icon)) {
                warn!("Failed to update tray icon for theme change: {e}");
            }
        }
        Err(e) => warn!("Failed to decode tray icon image: {e}"),
    }
}

#[cfg(target_os = "linux")]
pub fn create_tray_icon(app: &tauri::App<Wry>) -> Result<(), tauri::Error> {
    crate::tray_linux::spawn(app);
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn create_tray_icon(app: &tauri::App<Wry>) -> Result<(), tauri::Error> {
    let quit = MenuItemBuilder::with_id("quit", "Quit")
        .accelerator("CmdOrCtrl+Shift+Q")
        .build(app)?;
    let open = MenuItemBuilder::with_id("toggle", "Toggle App").build(app)?;
    let pin = MenuItemBuilder::with_id("pin", "Pin Window").build(app)?;
    let view_logs = MenuItemBuilder::with_id("view_logs", "View Logs").build(app)?;

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

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    let mut position_menu_items = vec![
        &set_center_position,
        &set_top_right_position,
        &set_bottom_right_position,
        &set_bottom_left_position,
        &set_top_left_position,
    ];

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    position_menu_items.push(&set_traycenter_position);

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let position_menu_items = vec![
        &set_center_position,
        &set_top_right_position,
        &set_bottom_right_position,
        &set_bottom_left_position,
        &set_top_left_position,
    ];

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

    let set_size_xs = MenuItemBuilder::with_id("set_size_xs", "Extra Small").build(app)?;
    let set_size_small = MenuItemBuilder::with_id("set_size_small", "Small").build(app)?;
    let set_size_default = MenuItemBuilder::with_id("set_size_default", "Default").build(app)?;
    let set_size_medium = MenuItemBuilder::with_id("set_size_medium", "Medium").build(app)?;
    let set_size_large = MenuItemBuilder::with_id("set_size_large", "Large").build(app)?;
    let set_size_xl = MenuItemBuilder::with_id("set_size_xl", "Extra Large").build(app)?;
    let set_window_size_submenu = SubmenuBuilder::new(app, "Set Window Size")
        .item(&set_size_xs)
        .item(&set_size_small)
        .item(&set_size_default)
        .item(&set_size_medium)
        .item(&set_size_large)
        .item(&set_size_xl)
        .build()?;

    let main_separator = PredefinedMenuItem::separator(app)?;
    let logs_separator = PredefinedMenuItem::separator(app)?;
    let menu = MenuBuilder::new(app)
        .item(&open)
        .item(&main_separator)
        .item(&pin)
        .item(&set_window_position_submenu)
        .item(&set_window_size_submenu)
        .item(&logs_separator)
        .item(&view_logs)
        .item(&quit)
        .build()?;
    let icon = tauri::image::Image::from_bytes(current_tray_icon_bytes())?;

    let tray = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .icon_as_template(true)
        .tooltip("kftray")
        .show_menu_on_left_click(false)
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
                    reset_window_position(window);
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
            "view_logs" => {
                if let Err(e) = crate::commands::logs::open_log_viewer_window(app) {
                    error!("Failed to open log viewer window: {e}");
                }
            }
            id if id.starts_with("set_size_") => {
                let preset_id = id.trim_start_matches("set_size_");
                if let Some(preset) = crate::window_size::WindowSizePreset::from_id(preset_id) {
                    if let Some(window) = app.get_webview_window("main") {
                        let app_state = window.state::<AppState>();
                        let runtime = app_state.runtime.clone();
                        runtime.spawn(async move {
                            crate::window::apply_window_size_preset(&window, preset).await;
                        });
                    } else {
                        error!("Main window not found on size preset event");
                    }
                }
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            match &event {
                TrayIconEvent::Click { rect, .. }
                | TrayIconEvent::Enter { rect, .. }
                | TrayIconEvent::Leave { rect, .. }
                | TrayIconEvent::Move { rect, .. } => {
                    if let Some(tray_state) = tray.app_handle().try_state::<TrayPositionState>() {
                        let physical_position = match rect.position {
                            tauri::Position::Physical(pos) => {
                                PhysicalPosition::new(f64::from(pos.x), f64::from(pos.y))
                            }
                            tauri::Position::Logical(pos) => {
                                let scale = tray
                                    .app_handle()
                                    .get_webview_window("main")
                                    .and_then(|w| w.current_monitor().ok()?)
                                    .map(|m| m.scale_factor())
                                    .unwrap_or(1.0);
                                pos.to_physical(scale)
                            }
                        };
                        let physical_size = match rect.size {
                            tauri::Size::Physical(size) => {
                                PhysicalSize::new(f64::from(size.width), f64::from(size.height))
                            }
                            tauri::Size::Logical(size) => {
                                let scale = tray
                                    .app_handle()
                                    .get_webview_window("main")
                                    .and_then(|w| w.current_monitor().ok()?)
                                    .map(|m| m.scale_factor())
                                    .unwrap_or(1.0);
                                size.to_physical(scale)
                            }
                        };

                        info!(
                            "Tray event captured - position: {physical_position:?}, size: {physical_size:?}"
                        );
                        *tray_state.position.lock().unwrap() =
                            Some((physical_position, physical_size));
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
                        "Click event - button: {button:?}, state: {button_state:?}"
                    );
                    if button == MouseButton::Left && button_state == MouseButtonState::Up {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            toggle_window_visibility(&window);
                        }
                    }
                }
                TrayIconEvent::DoubleClick { button, .. } => {
                    info!("Double click event - button: {button:?}");
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

    let _ = tray;
    Ok(())
}

pub(crate) fn handle_window_event(window: &tauri::Window<Wry>, event: &WindowEvent) {
    let Some(webview_window) = window.app_handle().get_webview_window(window.label()) else {
        error!("Failed to get webview window for label: {}", window.label());
        return;
    };

    if let WindowEvent::ScaleFactorChanged { new_inner_size, .. } = event {
        if let Err(e) = webview_window.set_size(tauri::Size::Physical(*new_inner_size)) {
            warn!("Failed to set window size during scale change: {e}");
        }
        return;
    }

    #[cfg(target_os = "windows")]
    if let WindowEvent::ThemeChanged(_) = event {
        refresh_tray_icon_for_theme(window.app_handle());
    }

    info!("event: {event:?}");
    let app_state = webview_window.state::<AppState>();

    if let WindowEvent::Focused(is_focused) = event
        && !is_focused
        && !app_state.pinned.load(Ordering::SeqCst)
        && webview_window.label() == "main"
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
                let main_focused = app_handle_clone
                    .get_webview_window("main")
                    .map(|w| w.is_focused().unwrap_or(false))
                    .unwrap_or(false);
                let logs_focused = app_handle_clone
                    .get_webview_window("logs")
                    .map(|w| w.is_focused().unwrap_or(false))
                    .unwrap_or(false);

                if !main_focused
                    && !logs_focused
                    && let Err(e) = webview_window_clone.hide()
                {
                    error!("Failed to hide window: {e}");
                }
            });
        }
    }

    if let WindowEvent::Moved(physical_position) = event {
        #[warn(unused_must_use)]
        let _ = webview_window.with_webview(|_webview| {
            #[cfg(target_os = "linux")]
            {}

            #[cfg(windows)]
            unsafe {
                _webview
                    .controller()
                    .NotifyParentWindowPositionChanged()
                    .unwrap();
            }

            #[cfg(target_os = "macos")]
            {}
        });

        if webview_window.label() == "main" {
            let app_state = webview_window.state::<AppState>();

            #[cfg(target_os = "linux")]
            {
                app_state.positioning_active.store(false, Ordering::SeqCst);
            }

            if !app_state.positioning_active.load(Ordering::SeqCst) {
                let x = physical_position.x;
                let y = physical_position.y;

                if is_valid_position(&webview_window, x, y) {
                    let runtime = app_state.runtime.clone();
                    runtime.spawn(async move {
                        sleep(Duration::from_millis(500)).await;
                        save_window_position_async(WindowPosition { x, y }).await;
                    });
                } else {
                    warn!("Position ({x}, {y}) failed validation");
                }
            }
        }
    }

    if let WindowEvent::CloseRequested { api, .. } = event
        && webview_window.label() == "main"
        && !app_state.pinned.load(Ordering::SeqCst)
    {
        api.prevent_close();
        let app_handle = webview_window.app_handle();
        tauri::async_runtime::block_on(handle_exit_app(app_handle.clone()));
    }
}

// Signature is fixed by `App::run`'s callback contract.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn handle_run_event(app_handle: &tauri::AppHandle<Wry>, event: RunEvent) {
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
            positioning_active: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            pinned: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            runtime: Arc::new(Runtime::new().unwrap()),
        };

        assert!(
            !app_state.pinned.load(std::sync::atomic::Ordering::SeqCst),
            "App should start unpinned"
        );

        let initial_state = app_state.pinned.load(std::sync::atomic::Ordering::SeqCst);
        app_state
            .pinned
            .store(!initial_state, std::sync::atomic::Ordering::SeqCst);

        assert!(
            app_state.pinned.load(std::sync::atomic::Ordering::SeqCst),
            "App should be pinned after toggle"
        );

        let current_state = app_state.pinned.load(std::sync::atomic::Ordering::SeqCst);
        app_state
            .pinned
            .store(!current_state, std::sync::atomic::Ordering::SeqCst);

        assert!(
            !app_state.pinned.load(std::sync::atomic::Ordering::SeqCst),
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
}

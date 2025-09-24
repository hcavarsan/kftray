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
    warn,
};
use tauri::PhysicalPosition;
use tauri::PhysicalSize;
use tauri::{
    Manager,
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
    reset_window_position,
    save_window_position,
    set_window_position,
    toggle_window_visibility,
};

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

    let tray = TrayIconBuilder::new()
        .menu(&menu)
        .icon_as_template(true)
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
                                PhysicalPosition::new(pos.x as f64, pos.y as f64)
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
                                PhysicalSize::new(size.width as f64, size.height as f64)
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
                            "Tray event captured - position: {:?}, size: {:?}",
                            physical_position, physical_size
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
    let webview_window = match window.app_handle().get_webview_window(window.label()) {
        Some(webview_window) => webview_window,
        _ => {
            error!("Failed to get webview window for label: {}", window.label());
            return;
        }
    };

    if let WindowEvent::ScaleFactorChanged { new_inner_size, .. } = event {
        if let Err(e) = webview_window.set_size(tauri::Size::Physical(*new_inner_size)) {
            warn!("Failed to set window size during scale change: {e}");
        }
        return;
    }

    info!("event: {:?}", event);
    let app_state = webview_window.state::<AppState>();

    if let WindowEvent::Focused(is_focused) = event
        && !is_focused
        && !app_state.pinned.load(Ordering::SeqCst)
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
                if let Some(main_window) = app_handle_clone.get_webview_window("main")
                    && !main_window.is_focused().unwrap_or(false)
                    && let Err(e) = webview_window_clone.hide()
                {
                    error!("Failed to hide window: {e}");
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
                _webview
                    .controller()
                    .NotifyParentWindowPositionChanged()
                    .unwrap();
            }

            #[cfg(target_os = "macos")]
            {}
        });

        let app_state = webview_window.state::<AppState>();

        #[cfg(target_os = "linux")]
        {
            app_state.positioning_active.store(false, Ordering::SeqCst);
        }

        if !app_state.positioning_active.load(Ordering::SeqCst) {
            let webview_window_clone = webview_window.clone();
            let runtime = app_state.runtime.clone();
            runtime.spawn(async move {
                sleep(Duration::from_millis(500)).await;
                save_window_position(&webview_window_clone);
            });
        }
    }

    if let WindowEvent::CloseRequested { api, .. } = event
        && !app_state.pinned.load(Ordering::SeqCst)
    {
        api.prevent_close();
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

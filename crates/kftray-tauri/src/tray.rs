use std::sync::atomic::Ordering;
use std::time::Duration;

use kftray_commons::models::window::AppState;
use kftray_commons::models::window::SaveDialogState;
use log::{
    error,
    info,
};
use tauri::{
    CustomMenuItem,
    GlobalWindowEvent,
    Manager,
    RunEvent,
    SystemTray,
    SystemTrayEvent,
    SystemTrayMenu,
    SystemTraySubmenu,
};
use tauri_plugin_positioner::Position;
use tokio::time::sleep;

use crate::commands::portforward::handle_exit_app;
use crate::window::{
    adjust_window_size_and_position,
    reset_window_position,
    save_window_position,
    set_default_position,
    set_window_position,
    toggle_window_visibility,
};

pub fn create_tray_menu() -> SystemTray {
    let quit = CustomMenuItem::new("quit".to_string(), "Quit").accelerator("CmdOrCtrl+Shift+Q");
    let open = CustomMenuItem::new("toggle".to_string(), "Toggle App");

    let set_center_position = CustomMenuItem::new("set_center_position".to_string(), "Center");
    let set_top_right_position =
        CustomMenuItem::new("set_top_right_position".to_string(), "Top Right");
    let set_bottom_right_position =
        CustomMenuItem::new("set_bottom_right_position".to_string(), "Bottom Right");
    let set_bottom_left_position =
        CustomMenuItem::new("set_bottom_left_position".to_string(), "Bottom Left");
    let set_top_left_position =
        CustomMenuItem::new("set_top_left_position".to_string(), "Top Left");

    let mut set_window_position_menu = SystemTrayMenu::new()
        .add_item(set_center_position)
        .add_item(set_top_right_position)
        .add_item(set_bottom_right_position)
        .add_item(set_bottom_left_position)
        .add_item(set_top_left_position);

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        let set_traycenter_position =
            CustomMenuItem::new("set_traycenter_position".to_string(), "System Tray Center");
        set_window_position_menu = set_window_position_menu.add_item(set_traycenter_position);
    }

    let reset_position = CustomMenuItem::new("reset_position".to_string(), "Reset Position");
    set_window_position_menu =
        set_window_position_menu.add_native_item(tauri::SystemTrayMenuItem::Separator);
    set_window_position_menu = set_window_position_menu.add_item(reset_position);

    let set_window_position_submenu =
        SystemTraySubmenu::new("Set Window Position", set_window_position_menu);

    let system_tray_menu = SystemTrayMenu::new()
        .add_item(open)
        .add_submenu(set_window_position_submenu)
        .add_item(quit);

    SystemTray::new().with_menu(system_tray_menu)
}

pub fn handle_window_event(event: GlobalWindowEvent) {
    if let tauri::WindowEvent::ScaleFactorChanged {
        scale_factor,
        new_inner_size,
        ..
    } = event.event()
    {
        let window = event.window();
        adjust_window_size_and_position(window, *scale_factor, *new_inner_size);
        if !window.is_visible().unwrap() || !window.is_focused().unwrap() {
            let window = event.window().clone();
            tauri::async_runtime::spawn(async move {
                set_default_position(&window).await;
                window.show().unwrap();
                let _ = window.set_focus();
            });
        }

        return;
    }

    info!("event: {:?}", event.event());
    let app_state = event.window().state::<AppState>();
    let is_moving = app_state.is_moving.lock().unwrap();

    if let tauri::WindowEvent::Focused(is_focused) = event.event() {
        if !is_focused && !*is_moving && !app_state.is_pinned.load(Ordering::SeqCst) {
            let app_handle = event.window().app_handle();

            if let Some(state) = app_handle.try_state::<SaveDialogState>() {
                if !state.is_open.load(Ordering::SeqCst) {
                    let window = event.window().clone();
                    let app_handle_clone = app_handle.clone();
                    let runtime = app_state.runtime.clone();
                    runtime.spawn(async move {
                        sleep(Duration::from_millis(200)).await;
                        if !app_handle_clone
                            .get_window("main")
                            .unwrap()
                            .is_focused()
                            .unwrap()
                        {
                            window.hide().unwrap()
                        }
                    });
                }
            }
        }
    }

    if let tauri::WindowEvent::Moved(_) = event.event() {
        let should_save = !*is_moving && !app_state.is_plugin_moving.load(Ordering::SeqCst);
        drop(is_moving);

        let window = event.window().clone();
        let app_handle = window.app_handle();
        let is_plugin_moving = app_state.is_plugin_moving.load(Ordering::SeqCst);

        tauri::async_runtime::spawn(async move {
            #[warn(unused_must_use)]
            let _ = window.with_webview(|_webview| {
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

            if should_save {
                info!("is_plugin_moving: {}", is_plugin_moving);

                if let Some(window) = app_handle.get_window("main") {
                    save_window_position(&window).await;
                }
            }
        });
    }

    if let tauri::WindowEvent::CloseRequested { api, .. } = event.event() {
        if !app_state.is_pinned.load(Ordering::SeqCst) {
            api.prevent_close();
            event.window().hide().unwrap();
        }
    }
}

pub fn handle_run_event(app_handle: &tauri::AppHandle, event: RunEvent) {
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

pub fn handle_system_tray_event(app: &tauri::AppHandle, event: SystemTrayEvent) {
    tauri_plugin_positioner::on_tray_event(app, &event);

    match event {
        SystemTrayEvent::LeftClick { .. } => {
            if let Some(window) = app.get_window("main") {
                toggle_window_visibility(&window);
            } else {
                error!("Main window not found on SystemTrayEvent");
            }
        }
        SystemTrayEvent::RightClick { .. } => {}
        SystemTrayEvent::DoubleClick { .. } => {}
        SystemTrayEvent::MenuItemClick { id, .. } => match id.as_str() {
            "quit" => {
                tauri::async_runtime::block_on(handle_exit_app(app.clone()));
            }
            "toggle" => {
                if let Some(window) = app.get_window("main") {
                    toggle_window_visibility(&window);
                } else {
                    error!("Main window not found on SystemTrayEvent");
                }
            }
            "reset_position" => {
                let window = app.get_window("main").unwrap();
                tauri::async_runtime::spawn(async move {
                    reset_window_position(&window).await;
                });
            }
            "set_center_position" => {
                let window = app.get_window("main").unwrap();
                set_window_position(&window, Position::Center);
            }
            "set_top_right_position" => {
                let window = app.get_window("main").unwrap();
                set_window_position(&window, Position::TopRight);
            }
            "set_bottom_right_position" => {
                let window = app.get_window("main").unwrap();
                set_window_position(&window, Position::BottomRight);
            }
            "set_bottom_left_position" => {
                let window = app.get_window("main").unwrap();
                set_window_position(&window, Position::BottomLeft);
            }
            "set_top_left_position" => {
                let window = app.get_window("main").unwrap();
                set_window_position(&window, Position::TopLeft);
            }
            #[cfg(any(target_os = "windows", target_os = "macos"))]
            "set_traycenter_position" => {
                let window = app.get_window("main").unwrap();
                set_window_position(&window, Position::TrayCenter);
            }
            _ => {}
        },
        _ => {}
    }
}

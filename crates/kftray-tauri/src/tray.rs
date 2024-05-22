use std::sync::atomic::Ordering;
use std::time::Duration;

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
use tokio::runtime::Runtime;
use tokio::time::sleep;

use crate::kubeforward::port_forward;
use crate::models::window::SaveDialogState;
use crate::window::{
    reset_window_position,
    save_window_position,
    set_window_position,
    toggle_window_visibility,
};
use crate::AppState;

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
    let app_state = event.window().state::<AppState>();
    let mut is_moving = app_state.is_moving.lock().unwrap();

    if let tauri::WindowEvent::Focused(is_focused) = event.event() {
        if !is_focused && !*is_moving {
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
        if !*is_moving {
            *is_moving = true;
            let app_handle = event.window().app_handle();

            if let Some(window) = app_handle.get_window("main") {
                save_window_position(&window);
            }

            *is_moving = false;
        }
    }

    if let tauri::WindowEvent::CloseRequested { api, .. } = event.event() {
        api.prevent_close();
        event.window().hide().unwrap();
    }
}

pub fn handle_run_event(app_handle: &tauri::AppHandle, event: RunEvent) {
    match event {
        RunEvent::ExitRequested { ref api, .. } => {
            api.prevent_exit();
            stop_all_port_forwards_and_exit(app_handle);
        }
        RunEvent::Exit => {
            stop_all_port_forwards_and_exit(app_handle);
        }
        _ => {}
    }
}

pub fn stop_all_port_forwards_and_exit(app_handle: &tauri::AppHandle) {
    let runtime = Runtime::new().expect("Failed to create a Tokio runtime");

    runtime.block_on(async {
        match port_forward::stop_all_port_forward().await {
            Ok(_) => {
                println!("Successfully stopped all port forwards.");
            }
            Err(err) => {
                eprintln!("Failed to stop port forwards: {}", err);
            }
        }
    });
    app_handle.exit(0);
}

pub fn handle_system_tray_event(app: &tauri::AppHandle, event: SystemTrayEvent) {
    tauri_plugin_positioner::on_tray_event(app, &event);

    match event {
        SystemTrayEvent::LeftClick { .. } => {
            let window = app.get_window("main").unwrap();
            if window.is_visible().unwrap() {
                window.hide().unwrap();
            } else {
                window.show().unwrap();
                window.set_focus().unwrap();
            }
        }
        SystemTrayEvent::RightClick { .. } => {}
        SystemTrayEvent::DoubleClick { .. } => {}
        SystemTrayEvent::MenuItemClick { id, .. } => match id.as_str() {
            "quit" => {
                stop_all_port_forwards_and_exit(app);
            }
            "toggle" => {
                let window = app.get_window("main").unwrap();
                toggle_window_visibility(&window);
            }
            "reset_position" => {
                let window = app.get_window("main").unwrap();
                reset_window_position(&window);
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

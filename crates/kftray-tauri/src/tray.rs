use std::sync::atomic::Ordering;

use tauri::{
    CustomMenuItem,
    GlobalWindowEvent,
    Manager,
    RunEvent,
    SystemTray,
    SystemTrayEvent,
    SystemTrayMenu,
};
use tokio::runtime::Runtime;

use crate::kubeforward::port_forward;
use crate::models::window::SaveDialogState;
use crate::window::{
    reset_window_position,
    save_window_position,
    toggle_window_visibility,
};
use crate::AppState;

pub fn create_tray_menu() -> SystemTray {
    let quit = CustomMenuItem::new("quit".to_string(), "Quit").accelerator("CmdOrCtrl+Shift+Q");
    let open = CustomMenuItem::new("toggle".to_string(), "Toggle App");
    let reset_position = CustomMenuItem::new("reset_position".to_string(), "Reset Position");

    let system_tray_menu = SystemTrayMenu::new()
        .add_item(open)
        .add_item(reset_position)
        .add_item(quit);

    SystemTray::new().with_menu(system_tray_menu)
}

pub fn handle_window_event(event: GlobalWindowEvent) {
    let app_state = event.window().state::<AppState>();
    let mut is_moving = app_state.is_moving.lock().unwrap();

    println!("Window event: {:?}", event.event());
    if let tauri::WindowEvent::Focused(is_focused) = event.event() {
        println!("Focused event: {}", is_focused);
        if !is_focused && !*is_moving {
            let app_handle = event.window().app_handle();

            if let Some(state) = app_handle.try_state::<SaveDialogState>() {
                if !state.is_open.load(Ordering::SeqCst) {
                    save_window_position(&app_handle.get_window("main").unwrap());
                    let window = event.window().clone();
                    println!("Hiding window after losing focus");
                    // Add a short delay before hiding the window
                            println!("Hiding window after delay");
                            window.hide().unwrap();
                }
            }
        }
    }

    if let tauri::WindowEvent::Moved(_) = event.event() {
        println!("Window moved event");
        if !*is_moving {
            *is_moving = true;

            let win = event.window();
            let _ = win.with_webview(|webview| {
                #[cfg(target_os = "linux")]
                {}

                #[cfg(windows)]
                unsafe {
                    // https://github.com/MicrosoftEdge/WebView2Feedback/issues/780#issuecomment-808306938
                    // https://docs.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller?view=webview2-1.0.774.44#notifyparentwindowpositionchanged
                    if let Err(e) = webview.controller().NotifyParentWindowPositionChanged() {
                        eprintln!("Failed to notify parent window position changed: {:?}", e);
                    }
                }

                #[cfg(target_os = "macos")]
                {}
            });

            let app_handle = event.window().app_handle();
            println!("Window moved, saving position");
            if let Some(window) = app_handle.get_window("main") {
                save_window_position(&window);
            } else {
                eprintln!("Failed to get main window");
            }

            *is_moving = false;
        }
    }

    if let tauri::WindowEvent::CloseRequested { api, .. } = event.event() {
        println!("Close requested event: {:?}", event.event());
        api.prevent_close();
        let app_handle = event.window().app_handle();
        let window = app_handle.get_window("main").unwrap();

        save_window_position(&window);

        println!("Hiding window after close requested");
        event.window().hide().unwrap();
    }
}

pub fn handle_run_event(app_handle: &tauri::AppHandle, event: RunEvent) {
    match event {
        RunEvent::ExitRequested { ref api, .. } => {
            println!("Exit requested event: {:?}", event);
            api.prevent_exit();
            stop_all_port_forwards_and_exit(app_handle);
        }
        RunEvent::Exit => {
            println!("Exit event: {:?}", event);
            stop_all_port_forwards_and_exit(app_handle);
        }
        _ => {}
    }
}

pub fn stop_all_port_forwards_and_exit(app_handle: &tauri::AppHandle) {
    let window = app_handle.get_window("main").unwrap();
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
    save_window_position(&window);
    app_handle.exit(0);
}

pub fn handle_system_tray_event(app: &tauri::AppHandle, event: SystemTrayEvent) {
    tauri_plugin_positioner::on_tray_event(app, &event);

    match event {
        SystemTrayEvent::LeftClick { .. } => {
            println!("System tray left click event");
            let window = app.get_window("main").unwrap();
            toggle_window_visibility(&window);
        }
        SystemTrayEvent::RightClick { .. } => {
            println!("System tray received a right click");
        }
        SystemTrayEvent::DoubleClick { .. } => {
            println!("System tray received a double click");
        }
        SystemTrayEvent::MenuItemClick { id, .. } => match id.as_str() {
            "quit" => {
                println!("System tray quit menu item click");
                stop_all_port_forwards_and_exit(app);
            }
            "toggle" => {
                println!("System tray toggle menu item click");
                let window = app.get_window("main").unwrap();
                toggle_window_visibility(&window);
            }
            "reset_position" => {
                println!("System tray reset position menu item click");
                let window = app.get_window("main").unwrap();
                reset_window_position(&window);
            }
            _ => {}
        },
        _ => {}
    }
}

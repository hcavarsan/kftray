use std::sync::atomic::{
    AtomicBool,
    AtomicU64,
    Ordering,
};
use std::time::{
    Duration,
    SystemTime,
    UNIX_EPOCH,
};
use std::sync::Mutex;
use tauri::GlobalWindowEvent;
use tauri::Manager;
use tauri::RunEvent;
use tauri::{
    CustomMenuItem,
    SystemTray,
    SystemTrayEvent,
    SystemTrayMenu,
};
use tokio::runtime::Runtime;

use crate::kubeforward::port_forward;
use crate::main;
use crate::models::window::SaveDialogState;
use crate::window::{
    reset_window_position,
    save_window_position,
    toggle_window_visibility,
};

// Atomic flag to track reset position action
static RESET_POSITION_TRIGGERED: AtomicBool = AtomicBool::new(false);
// Cooldown period after resetting the window position
const COOLDOWN_PERIOD: Duration = Duration::from_secs(1);
static LAST_RESET_TIME: AtomicU64 = AtomicU64::new(0);
// Mutex to track if the window is being moved
lazy_static::lazy_static! {
    static ref WINDOW_IS_MOVING: Mutex<bool> = Mutex::new(false);
}

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
    if let tauri::WindowEvent::Focused(is_focused) = event.event() {
        if !is_focused
            && !RESET_POSITION_TRIGGERED.load(Ordering::SeqCst)
            && !*WINDOW_IS_MOVING.lock().unwrap()
        {
            let app_handle = event.window().app_handle();

            if let Some(state) = app_handle.try_state::<SaveDialogState>() {
                if !state.is_open.load(Ordering::SeqCst) {
                    // Check if the cooldown period has passed
                    let last_reset_time = LAST_RESET_TIME.load(Ordering::SeqCst);
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs();
                    if now > last_reset_time + COOLDOWN_PERIOD.as_secs() {
                        save_window_position(&app_handle.get_window("main").unwrap());
                        // Delay hiding the window to avoid conflicts with dragging
                        std::thread::spawn({
                            let window = event.window().clone();
                            move || {
                                std::thread::sleep(Duration::from_millis(100));
                                println!("Hiding window after losing focus");
                                window.hide().unwrap();
                            }
                        });
                    }
                }
            }
        }
    }

    if let tauri::WindowEvent::Moved { .. } = event.event() {
        let app_handle = event.window().app_handle();
        println!("Window moved, saving position");
        {
            let mut is_moving = WINDOW_IS_MOVING.lock().unwrap();
            *is_moving = true;
        }
		let window = app_handle.get_window("main").unwrap();
        save_window_position(&window);
		window.set_focus().unwrap();
        {
            let mut is_moving = WINDOW_IS_MOVING.lock().unwrap();
            *is_moving = false;
        }
    }

    if let tauri::WindowEvent::CloseRequested { api, .. } = event.event() {
        if !*WINDOW_IS_MOVING.lock().unwrap() {
            api.prevent_close();
            let app_handle = event.window().app_handle();
            save_window_position(&app_handle.get_window("main").unwrap());
            println!("Hiding window after close requested");
            event.window().hide().unwrap();
        }
    }
}

pub fn handle_run_event(app_handle: &tauri::AppHandle, event: RunEvent) {
    match event {
        RunEvent::ExitRequested { api, .. } => {
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
            let window = app.get_window("main").unwrap();
            toggle_window_visibility(&window);
        }
        SystemTrayEvent::RightClick { .. } => {
            println!("system tray received a right click");
        }
        SystemTrayEvent::DoubleClick { .. } => {
            println!("system tray received a double click");
        }
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
                RESET_POSITION_TRIGGERED.store(true, Ordering::SeqCst);
                reset_window_position(&window);
                // Set the last reset time to now
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                LAST_RESET_TIME.store(now, Ordering::SeqCst);
                RESET_POSITION_TRIGGERED.store(false, Ordering::SeqCst);
            }
            _ => {}
        },
        _ => {}
    }
}

use std::sync::atomic::{
    AtomicBool,
    AtomicU64,
    Ordering,
};
use std::sync::Mutex;
use std::time::{
    Duration,
    Instant,
    SystemTime,
    UNIX_EPOCH,
};

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

// Atomic flag to track reset position action
static RESET_POSITION_TRIGGERED: AtomicBool = AtomicBool::new(false);
// Cooldown period after resetting the window position
const COOLDOWN_PERIOD: Duration = Duration::from_secs(1);
static LAST_RESET_TIME: AtomicU64 = AtomicU64::new(0);
// Mutex to track if the window is being moved
lazy_static::lazy_static! {
    static ref WINDOW_IS_MOVING: Mutex<bool> = Mutex::new(false);
}

// Atomic flag to track if the window is currently being moved
static WINDOW_IS_BEING_MOVED: AtomicBool = AtomicBool::new(false);

// Debounce duration for focus event
const DEBOUNCE_DURATION: Duration = Duration::from_millis(500);
static LAST_FOCUS_TIME: Mutex<Option<Instant>> = Mutex::new(None);

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
	println!("Window event: {:?}", event.event());
    if let tauri::WindowEvent::Focused(is_focused) = event.event() {
        if !is_focused
            && !RESET_POSITION_TRIGGERED.load(Ordering::SeqCst)
            && !WINDOW_IS_BEING_MOVED.load(Ordering::SeqCst)
			&& !matches!(event.event(), tauri::WindowEvent::Moved { .. })
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
                        // Debounce logic
                        let mut last_focus_time = LAST_FOCUS_TIME.lock().unwrap();
                        if let Some(last_time) = *last_focus_time {
                            if last_time.elapsed() < DEBOUNCE_DURATION {
								app_handle.get_window("main").unwrap().set_focus().unwrap();
                                return;
                            }
                        }
                        *last_focus_time = Some(Instant::now());

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
        let window = app_handle.get_window("main").unwrap();

        WINDOW_IS_BEING_MOVED.store(true, Ordering::SeqCst);
        save_window_position(&window);

        // Clear the moving flag after a short delay
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            println!("Window move operation completed");
            WINDOW_IS_BEING_MOVED.store(false, Ordering::SeqCst);
        });
    }

    if let tauri::WindowEvent::CloseRequested { api, .. } = event.event() {
        if !WINDOW_IS_BEING_MOVED.load(Ordering::SeqCst) {
            api.prevent_close();
            let app_handle = event.window().app_handle();
            let window = app_handle.get_window("main").unwrap();

            save_window_position(&window);

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

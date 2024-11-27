use std::sync::atomic::Ordering;

use kftray_commons::models::window::AppState;
use kftray_commons::models::window::SaveDialogState;
use log::error;
use tauri::Manager;
use tauri::State;

#[tauri::command]
pub fn open_save_dialog(state: State<SaveDialogState>) {
    state.is_open.store(true, Ordering::SeqCst);
}

#[tauri::command]
pub fn close_save_dialog(state: State<SaveDialogState>) {
    state.is_open.store(false, Ordering::SeqCst);
}

#[tauri::command]
pub fn toggle_pin_state(app_state: tauri::State<AppState>, window: tauri::Window) {
    let new_pin_state = !app_state.is_pinned.load(Ordering::SeqCst);
    app_state.is_pinned.store(new_pin_state, Ordering::SeqCst);

    if new_pin_state && !window.is_visible().unwrap_or(true) {
        let _ = window.show();
        let _ = window.set_focus();
    }

    if let Err(e) = window.set_always_on_top(new_pin_state) {
        error!("Failed to toggle pin state: {:?}", e);
    }

    let menu_title = if new_pin_state {
        "Unpin Window"
    } else {
        "Pin Window"
    };

    if let Err(e) = window
        .app_handle()
        .tray_handle()
        .get_item("pin")
        .set_title(menu_title)
    {
        error!("Failed to update menu item text: {:?}", e);
    }

    if let Err(e) = window.emit("pin-state-changed", new_pin_state) {
        error!("Failed to emit pin state event: {:?}", e);
    }
}

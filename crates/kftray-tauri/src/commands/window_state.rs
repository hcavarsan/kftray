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
    let is_pinned = app_state.is_pinned.load(Ordering::SeqCst);
    let new_pin_state = !is_pinned;
    app_state.is_pinned.store(new_pin_state, Ordering::SeqCst);

    if let Err(e) = window.set_always_on_top(new_pin_state) {
        eprintln!("Failed to toggle pin state: {:?}", e);
    }

    // Update tray menu item text
    let app_handle = window.app_handle();
    let tray_handle = app_handle.tray_handle();
    let new_text = if new_pin_state {
        "Unpin Window"
    } else {
        "Pin Window"
    };
    if let Err(e) = tray_handle.get_item("pin").set_title(new_text) {
        error!("Failed to update menu item text: {:?}", e);
    }

    // Emit event for frontend
    if let Err(e) = window.emit("pin-state-changed", new_pin_state) {
        error!("Failed to emit pin state event: {:?}", e);
    }
}

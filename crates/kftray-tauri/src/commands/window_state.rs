use std::sync::atomic::Ordering;

use kftray_commons::models::window::AppState;
use kftray_commons::models::window::SaveDialogState;
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
    app_state.is_pinned.store(!is_pinned, Ordering::SeqCst);
    window.set_always_on_top(!is_pinned).unwrap();
}

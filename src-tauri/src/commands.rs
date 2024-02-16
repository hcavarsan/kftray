use std::sync::atomic::{AtomicBool, Ordering};
use tauri::State;

pub struct SaveDialogState {
    pub is_open: AtomicBool,
}

impl Default for SaveDialogState {
    fn default() -> Self {
        SaveDialogState {
            is_open: AtomicBool::new(false),
        }
    }
}

#[tauri::command]
pub fn open_save_dialog(state: State<SaveDialogState>) {
    state.is_open.store(true, Ordering::SeqCst);
}

#[tauri::command]
pub fn close_save_dialog(state: State<SaveDialogState>) {
    state.is_open.store(false, Ordering::SeqCst);
}

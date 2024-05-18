use std::sync::atomic::AtomicBool;

//  state for the save dialog
pub struct SaveDialogState {
    pub is_open: AtomicBool,
}

//  default implementation for the SaveDialogState
impl Default for SaveDialogState {
    fn default() -> Self {
        SaveDialogState {
            is_open: AtomicBool::new(false),
        }
    }
}

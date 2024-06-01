use std::sync::atomic::AtomicBool;

use serde::{
    Deserialize,
    Serialize,
};

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

#[derive(Serialize, Deserialize, Debug)]
pub struct WindowPosition {
    pub x: i32,
    pub y: i32,
}

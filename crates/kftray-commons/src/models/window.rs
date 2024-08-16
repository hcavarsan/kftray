use std::sync::atomic::AtomicBool;
use std::sync::{
    Arc,
    Mutex,
};

use serde::{
    Deserialize,
    Serialize,
};
use tokio::runtime::Runtime;

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

#[derive(Serialize, Deserialize, Debug)]
pub struct WindowPosition {
    pub x: i32,
    pub y: i32,
}

pub struct AppState {
    pub is_moving: Arc<Mutex<bool>>,
    pub is_plugin_moving: Arc<AtomicBool>,
    pub is_pinned: Arc<AtomicBool>,
    pub runtime: Arc<Runtime>,
}

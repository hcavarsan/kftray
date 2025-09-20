use std::sync::Arc;
use std::sync::atomic::AtomicBool;

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
    pub positioning_active: Arc<AtomicBool>,
    pub pinned: Arc<AtomicBool>,
    pub runtime: Arc<Runtime>,
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::*;

    #[test]
    fn test_save_dialog_state_default() {
        let state = SaveDialogState::default();
        assert!(!state.is_open.load(Ordering::Relaxed));
    }
}

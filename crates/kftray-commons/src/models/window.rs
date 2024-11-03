//! Window state and manager
//!
//! This module provides models for the window state and manager,
//! including save dialog state, window state, and app state.

use std::sync::atomic::{
    AtomicBool,
    Ordering,
};
use std::sync::{
    Arc,
    Mutex,
};

use serde::{
    Deserialize,
    Serialize,
};
use tokio::runtime::Runtime;

#[derive(Debug)]
pub struct WindowManager {
    pub save_dialog_state: SaveDialogState,
    pub window_state: WindowState,
    pub app_state: AppState,
}

#[derive(Debug)]
pub struct SaveDialogState {
    pub is_open: AtomicBool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WindowPosition {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug)]
pub struct WindowState {
    pub position: Arc<Mutex<WindowPosition>>,
    pub is_visible: AtomicBool,
    pub is_focused: AtomicBool,
}

#[derive(Debug)]
pub struct AppState {
    pub is_moving: Arc<Mutex<bool>>,
    pub is_plugin_moving: Arc<AtomicBool>,
    pub is_pinned: Arc<AtomicBool>,
    pub runtime: Arc<Runtime>,
}

impl WindowManager {
    pub fn new(runtime: Runtime) -> Self {
        Self {
            save_dialog_state: SaveDialogState::default(),
            window_state: WindowState::default(),
            app_state: AppState::new(runtime),
        }
    }
}

impl Default for SaveDialogState {
    fn default() -> Self {
        Self {
            is_open: AtomicBool::new(false),
        }
    }
}

impl SaveDialogState {
    pub fn is_open(&self) -> bool {
        self.is_open.load(Ordering::Relaxed)
    }

    pub fn set_open(&self, value: bool) {
        self.is_open.store(value, Ordering::Relaxed);
    }
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            position: Arc::new(Mutex::new(WindowPosition { x: 0, y: 0 })),
            is_visible: AtomicBool::new(true),
            is_focused: AtomicBool::new(false),
        }
    }
}

impl WindowState {
    pub fn get_position(&self) -> WindowPosition {
        self.position.lock().unwrap().clone()
    }

    pub fn set_position(&self, x: i32, y: i32) {
        let mut pos = self.position.lock().unwrap();
        pos.x = x;
        pos.y = y;
    }

    pub fn is_visible(&self) -> bool {
        self.is_visible.load(Ordering::Relaxed)
    }

    pub fn set_visible(&self, value: bool) {
        self.is_visible.store(value, Ordering::Relaxed);
    }

    pub fn is_focused(&self) -> bool {
        self.is_focused.load(Ordering::Relaxed)
    }

    pub fn set_focused(&self, value: bool) {
        self.is_focused.store(value, Ordering::Relaxed);
    }
}

impl AppState {
    pub fn new(runtime: Runtime) -> Self {
        Self {
            is_moving: Arc::new(Mutex::new(false)),
            is_plugin_moving: Arc::new(AtomicBool::new(false)),
            is_pinned: Arc::new(AtomicBool::new(false)),
            runtime: Arc::new(runtime),
        }
    }

    pub fn set_moving(&self, value: bool) {
        *self.is_moving.lock().unwrap() = value;
    }

    pub fn is_moving(&self) -> bool {
        *self.is_moving.lock().unwrap()
    }

    pub fn set_plugin_moving(&self, value: bool) {
        self.is_plugin_moving.store(value, Ordering::Relaxed);
    }

    pub fn is_plugin_moving(&self) -> bool {
        self.is_plugin_moving.load(Ordering::Relaxed)
    }

    pub fn set_pinned(&self, value: bool) {
        self.is_pinned.store(value, Ordering::Relaxed);
    }

    pub fn is_pinned(&self) -> bool {
        self.is_pinned.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use tokio::runtime::Runtime;

    use super::*;

    #[test]
    fn test_window_state() {
        let state = WindowState::default();

        state.set_position(100, 200);
        let pos = state.get_position();
        assert_eq!(pos.x, 100);
        assert_eq!(pos.y, 200);

        state.set_visible(false);
        assert!(!state.is_visible());

        state.set_focused(true);
        assert!(state.is_focused());
    }

    #[test]
    fn test_app_state() {
        let runtime = Runtime::new().unwrap();
        let state = AppState::new(runtime);

        state.set_moving(true);
        assert!(state.is_moving());

        state.set_plugin_moving(true);
        assert!(state.is_plugin_moving());

        state.set_pinned(true);
        assert!(state.is_pinned());
    }
}

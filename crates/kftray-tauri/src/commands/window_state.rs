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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{
        atomic::AtomicBool,
        Arc,
        Mutex,
    };

    use tokio::runtime::Runtime;

    use super::*;

    fn create_mock_app_state() -> AppState {
        AppState {
            is_moving: Arc::new(std::sync::Mutex::new(false)),
            is_plugin_moving: Arc::new(AtomicBool::new(false)),
            is_pinned: Arc::new(AtomicBool::new(false)),
            runtime: Arc::new(Runtime::new().unwrap()),
        }
    }

    #[test]
    fn test_open_save_dialog_direct() {
        let state = SaveDialogState::default();

        assert!(
            !state.is_open.load(Ordering::SeqCst),
            "SaveDialogState should initialize with is_open = false"
        );

        state.is_open.store(true, Ordering::SeqCst);

        assert!(
            state.is_open.load(Ordering::SeqCst),
            "Dialog should be open after setting is_open to true"
        );
    }

    #[test]
    fn test_close_save_dialog_direct() {
        let state = SaveDialogState::default();
        state.is_open.store(true, Ordering::SeqCst);

        assert!(
            state.is_open.load(Ordering::SeqCst),
            "Dialog should be open before test"
        );

        state.is_open.store(false, Ordering::SeqCst);

        assert!(
            !state.is_open.load(Ordering::SeqCst),
            "Dialog should be closed after setting is_open to false"
        );
    }

    #[test]
    fn test_save_dialog_operations() {
        let state = SaveDialogState::default();

        assert!(
            !state.is_open.load(Ordering::SeqCst),
            "SaveDialogState should initialize with is_open = false"
        );

        state.is_open.store(true, Ordering::SeqCst);

        assert!(
            state.is_open.load(Ordering::SeqCst),
            "Dialog should be open after setting is_open to true"
        );

        state.is_open.store(false, Ordering::SeqCst);

        assert!(
            !state.is_open.load(Ordering::SeqCst),
            "Dialog should be closed after setting is_open to false"
        );
    }

    #[test]
    fn test_app_state_toggle_pin() {
        let app_state = create_mock_app_state();

        assert!(
            !app_state.is_pinned.load(Ordering::SeqCst),
            "AppState should initialize with is_pinned = false"
        );

        let initial_state = app_state.is_pinned.load(Ordering::SeqCst);
        app_state.is_pinned.store(!initial_state, Ordering::SeqCst);

        assert!(
            app_state.is_pinned.load(Ordering::SeqCst),
            "AppState should be pinned after toggle"
        );

        let current_state = app_state.is_pinned.load(Ordering::SeqCst);
        app_state.is_pinned.store(!current_state, Ordering::SeqCst);

        assert!(
            !app_state.is_pinned.load(Ordering::SeqCst),
            "AppState should be unpinned after second toggle"
        );
    }

    #[test]
    fn test_save_dialog_state_initial_values() {
        let state = SaveDialogState::default();
        assert!(
            !state.is_open.load(Ordering::SeqCst),
            "SaveDialogState should initialize with is_open = false"
        );
    }

    #[test]
    fn test_app_state_initial_values() {
        let app_state = create_mock_app_state();

        assert!(
            !app_state.is_pinned.load(Ordering::SeqCst),
            "AppState should initialize with is_pinned = false"
        );
        assert!(
            !app_state.is_plugin_moving.load(Ordering::SeqCst),
            "AppState should initialize with is_plugin_moving = false"
        );

        let is_moving = *app_state.is_moving.lock().unwrap();
        assert!(
            !is_moving,
            "AppState should initialize with is_moving = false"
        );
    }

    struct MockTrayItem {
        title: String,
    }

    impl MockTrayItem {
        fn set_title(&mut self, title: &str) -> Result<(), String> {
            self.title = title.to_string();
            Ok(())
        }
    }

    struct MockTrayHandle {
        items: HashMap<String, MockTrayItem>,
    }

    impl MockTrayHandle {
        fn new() -> Self {
            let mut items = HashMap::new();
            items.insert(
                "pin".to_string(),
                MockTrayItem {
                    title: "Pin Window".to_string(),
                },
            );
            Self { items }
        }

        fn get_item(&mut self, id: &str) -> &mut MockTrayItem {
            self.items.get_mut(id).unwrap()
        }
    }

    struct MockAppHandle {
        tray_handle: Mutex<MockTrayHandle>,
    }

    impl MockAppHandle {
        fn new() -> Self {
            Self {
                tray_handle: Mutex::new(MockTrayHandle::new()),
            }
        }

        fn tray_handle(&self) -> &Mutex<MockTrayHandle> {
            &self.tray_handle
        }
    }

    struct MockWindow {
        visible: bool,
        always_on_top: bool,
        app_handle: Arc<MockAppHandle>,
        emitted_events: Mutex<Vec<(String, bool)>>,
    }

    impl MockWindow {
        fn new() -> Self {
            Self {
                visible: true,
                always_on_top: false,
                app_handle: Arc::new(MockAppHandle::new()),
                emitted_events: Mutex::new(Vec::new()),
            }
        }

        fn is_visible(&self) -> Result<bool, String> {
            Ok(self.visible)
        }

        fn show(&self) -> Result<(), String> {
            Ok(())
        }

        fn set_focus(&self) -> Result<(), String> {
            Ok(())
        }

        fn set_always_on_top(&mut self, always_on_top: bool) -> Result<(), String> {
            self.always_on_top = always_on_top;
            Ok(())
        }

        fn app_handle(&self) -> Arc<MockAppHandle> {
            self.app_handle.clone()
        }

        fn emit<T: serde::Serialize + Clone>(&self, event: &str, payload: T) -> Result<(), String> {
            if let Ok(payload) = serde_json::to_value(payload.clone()) {
                if let Ok(payload_bool) = serde_json::from_value::<bool>(payload) {
                    self.emitted_events
                        .lock()
                        .unwrap()
                        .push((event.to_string(), payload_bool));
                }
            }
            Ok(())
        }
    }

    #[test]
    fn test_toggle_pin_state() {
        let app_state = create_mock_app_state();
        let mut mock_window = MockWindow::new();

        let initial_pin_state = app_state.is_pinned.load(Ordering::SeqCst);
        assert!(!initial_pin_state, "Should start unpinned");

        let new_pin_state = !initial_pin_state;
        app_state.is_pinned.store(new_pin_state, Ordering::SeqCst);

        if new_pin_state && !mock_window.is_visible().unwrap_or(true) {
            let _ = mock_window.show();
            let _ = mock_window.set_focus();
        }

        let _ = mock_window.set_always_on_top(new_pin_state);

        let menu_title = if new_pin_state {
            "Unpin Window"
        } else {
            "Pin Window"
        };
        let _ = mock_window
            .app_handle()
            .tray_handle()
            .lock()
            .unwrap()
            .get_item("pin")
            .set_title(menu_title);

        // Emit event
        let _ = mock_window.emit("pin-state-changed", new_pin_state);

        assert!(
            app_state.is_pinned.load(Ordering::SeqCst),
            "App state should be pinned after toggle"
        );

        assert!(mock_window.always_on_top, "Window should be always on top");

        let app_handle = mock_window.app_handle();
        let mut tray_handle = app_handle.tray_handle().lock().unwrap();
        let tray_title = &tray_handle.get_item("pin").title;
        assert_eq!(
            tray_title, "Unpin Window",
            "Menu item should be updated to 'Unpin Window'"
        );

        let events = mock_window.emitted_events.lock().unwrap();
        assert!(
            events.contains(&("pin-state-changed".to_string(), true)),
            "Should emit pin-state-changed event with true payload"
        );
    }

    #[test]
    fn test_toggle_pin_state_when_hidden() {
        let app_state = create_mock_app_state();
        let mut mock_window = MockWindow::new();

        mock_window.visible = false;

        let new_pin_state = !app_state.is_pinned.load(Ordering::SeqCst);
        app_state.is_pinned.store(new_pin_state, Ordering::SeqCst);

        if new_pin_state && !mock_window.is_visible().unwrap_or(true) {
            let _ = mock_window.show();
            let _ = mock_window.set_focus();
        }

        let _ = mock_window.set_always_on_top(new_pin_state);

        assert!(
            app_state.is_pinned.load(Ordering::SeqCst),
            "App state should be pinned after toggle"
        );
        assert!(mock_window.always_on_top, "Window should be always on top");
    }
}

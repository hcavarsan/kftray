use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use global_hotkey::{
    GlobalHotKeyEvent,
    GlobalHotKeyManager,
    hotkey::{
        Code,
        HotKey,
        Modifiers,
    },
};
use log::{
    error,
    info,
    warn,
};
use tokio::sync::{
    Mutex,
    mpsc,
    oneshot,
};

use super::{
    PlatformManager,
    ShortcutResult,
};
use crate::actions::ActionRegistry;
use crate::models::{
    ActionContext,
    ShortcutDefinition,
};

#[derive(Debug)]
enum HotkeyCommand {
    Register {
        hotkey: HotKey,
        response: oneshot::Sender<Result<(), String>>,
    },
    Unregister {
        hotkey: HotKey,
        response: oneshot::Sender<Result<(), String>>,
    },
}

pub struct WindowsPlatform {
    command_sender: mpsc::UnboundedSender<HotkeyCommand>,
    shortcuts: Arc<Mutex<HashMap<i64, HotKey>>>,
    registry: Arc<Mutex<ActionRegistry>>,
    event_loop_started: Arc<Mutex<bool>>,
}

impl WindowsPlatform {
    pub fn new(registry: Arc<Mutex<ActionRegistry>>) -> ShortcutResult<Self> {
        let (command_sender, mut command_receiver) = mpsc::unbounded_channel::<HotkeyCommand>();

        // Spawn a dedicated thread for GlobalHotKeyManager operations
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let mut manager = match GlobalHotKeyManager::new() {
                    Ok(m) => m,
                    Err(e) => {
                        error!("Failed to create GlobalHotKeyManager: {}", e);
                        return;
                    }
                };

                while let Some(command) = command_receiver.recv().await {
                    match command {
                        HotkeyCommand::Register { hotkey, response } => {
                            let result = manager.register(hotkey).map_err(|e| e.to_string());
                            let _ = response.send(result);
                        }
                        HotkeyCommand::Unregister { hotkey, response } => {
                            let result = manager.unregister(hotkey).map_err(|e| e.to_string());
                            let _ = response.send(result);
                        }
                    }
                }
            });
        });

        Ok(Self {
            command_sender,
            shortcuts: Arc::new(Mutex::new(HashMap::new())),
            registry,
            event_loop_started: Arc::new(Mutex::new(false)),
        })
    }

    fn parse_shortcut(&self, shortcut_key: &str) -> ShortcutResult<HotKey> {
        let lowercase_key = shortcut_key.to_lowercase();
        let parts: Vec<&str> = lowercase_key.split('+').collect();
        let mut modifiers = Modifiers::empty();
        let mut key_code = None;

        for part in parts {
            match part.trim() {
                "ctrl" | "control" => modifiers |= Modifiers::CONTROL,
                "alt" => modifiers |= Modifiers::ALT,
                "shift" => modifiers |= Modifiers::SHIFT,
                "super" | "cmd" | "meta" | "win" => modifiers |= Modifiers::SUPER,
                key => key_code = Some(self.parse_key(key)?),
            }
        }

        let code = key_code.ok_or_else(|| {
            crate::models::ShortcutError::InvalidShortcut(format!("No key found: {}", shortcut_key))
        })?;

        Ok(HotKey::new(Some(modifiers), code))
    }

    fn parse_key(&self, key: &str) -> ShortcutResult<Code> {
        let code = match key {
            "a" => Code::KeyA,
            "b" => Code::KeyB,
            "c" => Code::KeyC,
            "d" => Code::KeyD,
            "e" => Code::KeyE,
            "f" => Code::KeyF,
            "g" => Code::KeyG,
            "h" => Code::KeyH,
            "i" => Code::KeyI,
            "j" => Code::KeyJ,
            "k" => Code::KeyK,
            "l" => Code::KeyL,
            "m" => Code::KeyM,
            "n" => Code::KeyN,
            "o" => Code::KeyO,
            "p" => Code::KeyP,
            "q" => Code::KeyQ,
            "r" => Code::KeyR,
            "s" => Code::KeyS,
            "t" => Code::KeyT,
            "u" => Code::KeyU,
            "v" => Code::KeyV,
            "w" => Code::KeyW,
            "x" => Code::KeyX,
            "y" => Code::KeyY,
            "z" => Code::KeyZ,
            "0" => Code::Digit0,
            "1" => Code::Digit1,
            "2" => Code::Digit2,
            "3" => Code::Digit3,
            "4" => Code::Digit4,
            "5" => Code::Digit5,
            "6" => Code::Digit6,
            "7" => Code::Digit7,
            "8" => Code::Digit8,
            "9" => Code::Digit9,
            "f1" => Code::F1,
            "f2" => Code::F2,
            "f3" => Code::F3,
            "f4" => Code::F4,
            "f5" => Code::F5,
            "f6" => Code::F6,
            "f7" => Code::F7,
            "f8" => Code::F8,
            "f9" => Code::F9,
            "f10" => Code::F10,
            "f11" => Code::F11,
            "f12" => Code::F12,
            "space" => Code::Space,
            "enter" => Code::Enter,
            "escape" => Code::Escape,
            "tab" => Code::Tab,
            "backspace" => Code::Backspace,
            "delete" => Code::Delete,
            "insert" => Code::Insert,
            "home" => Code::Home,
            "end" => Code::End,
            "pageup" => Code::PageUp,
            "pagedown" => Code::PageDown,
            "up" => Code::ArrowUp,
            "down" => Code::ArrowDown,
            "left" => Code::ArrowLeft,
            "right" => Code::ArrowRight,
            _ => {
                return Err(crate::models::ShortcutError::InvalidShortcut(format!(
                    "Unknown key: {}",
                    key
                )));
            }
        };
        Ok(code)
    }
}

#[async_trait]
impl PlatformManager for WindowsPlatform {
    async fn register_shortcut(&mut self, shortcut: &ShortcutDefinition) -> ShortcutResult<()> {
        let id = shortcut
            .id
            .ok_or_else(|| crate::models::ShortcutError::Internal("ID required".to_string()))?;

        let hotkey = self.parse_shortcut(&shortcut.shortcut_key)?;

        // Send register command through channel
        let (response_tx, response_rx) = oneshot::channel();
        self.command_sender
            .send(HotkeyCommand::Register {
                hotkey,
                response: response_tx,
            })
            .map_err(|_| {
                crate::models::ShortcutError::PlatformError(
                    "Failed to send register command".to_string(),
                )
            })?;

        // Wait for response
        let result = response_rx.await.map_err(|_| {
            crate::models::ShortcutError::PlatformError(
                "Failed to receive register response".to_string(),
            )
        })?;

        result.map_err(|e| {
            crate::models::ShortcutError::PlatformError(format!("Register failed: {}", e))
        })?;

        let mut shortcuts = self.shortcuts.lock().await;
        shortcuts.insert(id, hotkey);

        {
            let mut started = self.event_loop_started.lock().await;
            if !*started {
                *started = true;
                let shortcuts_clone = self.shortcuts.clone();
                let registry_clone = self.registry.clone();
                tokio::spawn(async move {
                    info!("Windows event loop started");
                    loop {
                        if let Ok(event) = GlobalHotKeyEvent::receiver().try_recv()
                            && event.state == global_hotkey::HotKeyState::Pressed
                        {
                            let shortcut_id = {
                                let shortcuts = shortcuts_clone.lock().await;
                                shortcuts
                                    .iter()
                                    .find(|(_, hotkey)| hotkey.id() == event.id)
                                    .map(|(id, _)| *id)
                            };

                            if let Some(id) = shortcut_id {
                                let context = ActionContext::new(id);
                                if let Err(e) = registry_clone
                                    .lock()
                                    .await
                                    .execute_by_id(id, &context)
                                    .await
                                {
                                    error!("Windows shortcut execution failed: {}", e);
                                }
                            }
                        }
                        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                    }
                });
            }
        }

        info!(
            "Windows shortcut registered: {} ({})",
            shortcut.name, shortcut.shortcut_key
        );
        Ok(())
    }

    async fn unregister_shortcut(&mut self, shortcut_id: i64) -> ShortcutResult<()> {
        let mut shortcuts = self.shortcuts.lock().await;
        if let Some(hotkey) = shortcuts.remove(&shortcut_id) {
            // Send unregister command through channel
            let (response_tx, response_rx) = oneshot::channel();
            self.command_sender
                .send(HotkeyCommand::Unregister {
                    hotkey,
                    response: response_tx,
                })
                .map_err(|_| {
                    crate::models::ShortcutError::PlatformError(
                        "Failed to send unregister command".to_string(),
                    )
                })?;

            // Wait for response
            let result = response_rx.await.map_err(|_| {
                crate::models::ShortcutError::PlatformError(
                    "Failed to receive unregister response".to_string(),
                )
            })?;

            result.map_err(|e| {
                crate::models::ShortcutError::PlatformError(format!("Unregister failed: {}", e))
            })?;

            info!("Windows shortcut unregistered: {}", shortcut_id);
        } else {
            warn!("Windows shortcut not found: {}", shortcut_id);
        }
        Ok(())
    }

    async fn is_available(&self) -> bool {
        true
    }

    async fn platform_name(&self) -> &str {
        "windows"
    }
}

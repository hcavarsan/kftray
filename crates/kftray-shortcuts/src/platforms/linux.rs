use std::collections::{
    HashMap,
    HashSet,
};
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use evdev::{
    Device,
    EventType,
    Key as EvdevKey,
};
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
use tokio::sync::Mutex;

use super::{
    PlatformManager,
    ShortcutResult,
};
use crate::actions::ActionRegistry;
use crate::models::{
    ActionContext,
    ShortcutDefinition,
};

pub struct LinuxPlatform {
    implementation: LinuxImpl,
    shortcuts: Arc<Mutex<HashMap<i64, String>>>,
    registry: Arc<Mutex<ActionRegistry>>,
    event_loop_started: Arc<Mutex<bool>>,
}

enum LinuxImpl {
    GlobalHotKey(GlobalHotKeyManager, Arc<Mutex<HashMap<i64, HotKey>>>),
    Evdev,
    Fallback,
}

impl LinuxPlatform {
    pub async fn new(registry: Arc<Mutex<ActionRegistry>>) -> ShortcutResult<Self> {
        let is_wayland = Self::is_wayland();
        let can_use_evdev = Self::can_use_evdev();

        info!(
            "Linux environment detection: wayland={}, evdev={}",
            is_wayland, can_use_evdev
        );
        info!("WAYLAND_DISPLAY: {:?}", std::env::var("WAYLAND_DISPLAY"));
        info!("XDG_SESSION_TYPE: {:?}", std::env::var("XDG_SESSION_TYPE"));

        let implementation = if is_wayland && can_use_evdev {
            info!("Using evdev for Linux (Wayland)");
            LinuxImpl::Evdev
        } else if is_wayland && !can_use_evdev {
            if Self::check_input_permissions() {
                warn!(
                    "Wayland detected but no accessible input devices found - falling back to X11 compatibility mode"
                );
            } else {
                warn!(
                    "Wayland detected but user lacks input device permissions - falling back to X11 compatibility mode"
                );
                warn!("To fix: run 'sudo usermod -a -G input $USER' then logout/login");
            }
            if let Ok(manager) = GlobalHotKeyManager::new() {
                info!("Using global-hotkey for Linux (X11 compatibility)");
                LinuxImpl::GlobalHotKey(manager, Arc::new(Mutex::new(HashMap::new())))
            } else {
                warn!("GlobalHotKey also failed - shortcuts will not work");
                LinuxImpl::Fallback
            }
        } else if let Ok(manager) = GlobalHotKeyManager::new() {
            info!("Using global-hotkey for Linux (X11)");
            LinuxImpl::GlobalHotKey(manager, Arc::new(Mutex::new(HashMap::new())))
        } else if can_use_evdev {
            info!("Using evdev for Linux (fallback)");
            LinuxImpl::Evdev
        } else {
            warn!("No shortcut implementation available - shortcuts will not work");
            LinuxImpl::Fallback
        };

        Ok(Self {
            implementation,
            shortcuts: Arc::new(Mutex::new(HashMap::new())),
            registry,
            event_loop_started: Arc::new(Mutex::new(false)),
        })
    }

    fn is_wayland() -> bool {
        std::env::var("WAYLAND_DISPLAY").is_ok()
            || std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland")
    }

    fn can_use_evdev() -> bool {
        if !std::path::Path::new("/dev/input").exists() {
            info!("evdev not available: /dev/input directory not found");
            return false;
        }

        let has_permissions = Self::check_input_permissions();
        if !has_permissions {
            info!("evdev not available: user not in input group or insufficient permissions");
            return false;
        }

        let has_devices = std::fs::read_dir("/dev/input")
            .map(|entries| {
                entries.filter_map(|entry| entry.ok()).any(|entry| {
                    let path = entry.path();
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .map(|name| name.starts_with("event"))
                        .unwrap_or(false)
                        && Device::open(&path).is_ok()
                })
            })
            .unwrap_or(false);

        if !has_devices {
            info!("evdev not available: no accessible input devices found");
        }

        has_devices
    }

    fn check_input_permissions() -> bool {
        // Check if user is in input group
        if let Ok(output) = std::process::Command::new("groups").output() {
            let groups = String::from_utf8_lossy(&output.stdout);
            if groups.contains("input") {
                return true;
            }
        }

        // Fallback: try to access a device directly
        if let Ok(entries) = std::fs::read_dir("/dev/input") {
            for entry in entries.filter_map(Result::ok) {
                let path = entry.path();
                if path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.starts_with("event"))
                    .unwrap_or(false)
                {
                    // Try to open the device - this will fail if no permissions
                    if Device::open(&path).is_ok() {
                        return true;
                    }
                }
            }
        }

        false
    }

    pub fn needs_permission_fix() -> bool {
        Self::is_wayland() && !Self::check_input_permissions()
    }

    pub fn try_fix_permissions() -> Result<String, String> {
        if !Self::is_wayland() {
            return Err("Permission fix only needed on Wayland systems".to_string());
        }

        if Self::check_input_permissions() {
            return Ok("User already has input device permissions".to_string());
        }

        // Get current user
        let user = std::env::var("USER").map_err(|_| "Could not determine current user")?;

        // Try to add user to input group
        // First try with /usr/sbin/usermod (most common location)
        let output = std::process::Command::new("/usr/bin/pkexec")
            .args(["/usr/sbin/usermod", "-a", "-G", "input", &user])
            .output();

        let output = match output {
            Ok(out) => out,
            Err(_) => {
                // Fallback: try /bin/usermod (some systems have it here)
                std::process::Command::new("/usr/bin/pkexec")
                    .args(["/bin/usermod", "-a", "-G", "input", &user])
                    .output()
                    .map_err(|e| format!("Failed to execute usermod command: {}", e))?
            }
        };

        if output.status.success() {
            Ok(format!(
                "Successfully added user '{}' to input group. Please logout and login again for changes to take effect.",
                user
            ))
        } else {
            let error = String::from_utf8_lossy(&output.stderr);
            Err(format!("Failed to add user to input group: {}", error))
        }
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
                "super" | "cmd" | "meta" => modifiers |= Modifiers::SUPER,
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

    pub async fn start_event_loop(&self) -> ShortcutResult<()> {
        match &self.implementation {
            LinuxImpl::GlobalHotKey(_, hotkeys) => loop {
                if let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
                    let shortcut_id = {
                        let registered_hotkeys = hotkeys.lock().await;
                        registered_hotkeys
                            .iter()
                            .find(|(_, hotkey)| hotkey.id() == event.id)
                            .map(|(id, _)| *id)
                    };

                    if let Some(id) = shortcut_id {
                        let context = ActionContext::new(id);
                        if let Err(e) = self.registry.lock().await.execute_by_id(id, &context).await
                        {
                            error!("Linux shortcut execution failed: {}", e);
                        }
                    }
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            },
            LinuxImpl::Evdev => {
                info!("Evdev event loop not implemented - shortcuts registered but not functional");
                Ok(())
            }
            LinuxImpl::Fallback => {
                info!("Fallback implementation - shortcuts registered but not functional");
                Ok(())
            }
        }
    }

    fn evdev_monitoring_loop(
        shortcuts: Arc<Mutex<HashMap<i64, String>>>, registry: Arc<Mutex<ActionRegistry>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut devices = Self::discover_keyboard_devices()?;
        if devices.is_empty() {
            return Err("No keyboard devices found for evdev monitoring".into());
        }

        info!(
            "Found {} keyboard devices for evdev monitoring",
            devices.len()
        );

        let rt = tokio::runtime::Handle::try_current()?;
        let mut pressed_keys = HashSet::new();
        let mut last_trigger = Instant::now();

        loop {
            for device in &mut devices {
                if let Ok(events) = device.fetch_events() {
                    for event in events {
                        if event.event_type() == EventType::KEY {
                            let key_code = event.code();
                            let value = event.value();

                            match value {
                                1 => {
                                    pressed_keys.insert(key_code);
                                    if let Some(shortcut_id) = rt.block_on(
                                        Self::check_shortcut_match(&pressed_keys, &shortcuts),
                                    ) {
                                        let now = Instant::now();
                                        if now.duration_since(last_trigger)
                                            > std::time::Duration::from_millis(250)
                                        {
                                            last_trigger = now;
                                            let context = ActionContext::new(shortcut_id);
                                            if let Err(e) = rt.block_on(async {
                                                registry
                                                    .lock()
                                                    .await
                                                    .execute_by_id(shortcut_id, &context)
                                                    .await
                                            }) {
                                                error!("Evdev shortcut execution failed: {}", e);
                                            }
                                        }
                                    }
                                }
                                0 => {
                                    pressed_keys.remove(&key_code);
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    fn discover_keyboard_devices() -> Result<Vec<Device>, Box<dyn std::error::Error>> {
        let mut devices = Vec::new();
        let entries = std::fs::read_dir("/dev/input")?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if let Some(filename) = path.file_name().and_then(|n| n.to_str())
                && filename.starts_with("event")
                && let Ok(device) = Device::open(&path)
                && Self::is_keyboard_device(&device)
            {
                devices.push(device);
            }
        }

        Ok(devices)
    }

    fn is_keyboard_device(device: &Device) -> bool {
        if let Some(keys) = device.supported_keys() {
            keys.contains(EvdevKey::KEY_A) && keys.contains(EvdevKey::KEY_S)
        } else {
            false
        }
    }

    async fn check_shortcut_match(
        pressed_keys: &HashSet<u16>, shortcuts: &Arc<Mutex<HashMap<i64, String>>>,
    ) -> Option<i64> {
        let shortcuts_guard = shortcuts.lock().await;

        for (id, shortcut_key) in shortcuts_guard.iter() {
            if Self::matches_shortcut(pressed_keys, shortcut_key) {
                return Some(*id);
            }
        }

        None
    }

    fn matches_shortcut(pressed_keys: &HashSet<u16>, shortcut_key: &str) -> bool {
        let parts: Vec<&str> = shortcut_key.split('+').collect();
        let mut required_modifiers = Vec::new();
        let mut required_key = None;

        for part in parts {
            let part = part.trim().to_lowercase();
            match part.as_str() {
                "ctrl" | "control" => {
                    required_modifiers.push(vec![
                        EvdevKey::KEY_LEFTCTRL.code(),
                        EvdevKey::KEY_RIGHTCTRL.code(),
                    ]);
                }
                "shift" => {
                    required_modifiers.push(vec![
                        EvdevKey::KEY_LEFTSHIFT.code(),
                        EvdevKey::KEY_RIGHTSHIFT.code(),
                    ]);
                }
                "alt" => {
                    required_modifiers.push(vec![
                        EvdevKey::KEY_LEFTALT.code(),
                        EvdevKey::KEY_RIGHTALT.code(),
                    ]);
                }
                "super" | "cmd" | "meta" => {
                    required_modifiers.push(vec![
                        EvdevKey::KEY_LEFTMETA.code(),
                        EvdevKey::KEY_RIGHTMETA.code(),
                    ]);
                }
                key => {
                    if let Some(key_code) = Self::key_name_to_evdev(key) {
                        required_key = Some(key_code);
                    }
                }
            }
        }

        for modifier_codes in required_modifiers {
            if !modifier_codes
                .iter()
                .any(|code| pressed_keys.contains(code))
            {
                return false;
            }
        }

        if let Some(key) = required_key {
            pressed_keys.contains(&key)
        } else {
            false
        }
    }

    fn key_name_to_evdev(key: &str) -> Option<u16> {
        let evdev_key = match key {
            "a" => EvdevKey::KEY_A,
            "b" => EvdevKey::KEY_B,
            "c" => EvdevKey::KEY_C,
            "d" => EvdevKey::KEY_D,
            "e" => EvdevKey::KEY_E,
            "f" => EvdevKey::KEY_F,
            "g" => EvdevKey::KEY_G,
            "h" => EvdevKey::KEY_H,
            "i" => EvdevKey::KEY_I,
            "j" => EvdevKey::KEY_J,
            "k" => EvdevKey::KEY_K,
            "l" => EvdevKey::KEY_L,
            "m" => EvdevKey::KEY_M,
            "n" => EvdevKey::KEY_N,
            "o" => EvdevKey::KEY_O,
            "p" => EvdevKey::KEY_P,
            "q" => EvdevKey::KEY_Q,
            "r" => EvdevKey::KEY_R,
            "s" => EvdevKey::KEY_S,
            "t" => EvdevKey::KEY_T,
            "u" => EvdevKey::KEY_U,
            "v" => EvdevKey::KEY_V,
            "w" => EvdevKey::KEY_W,
            "x" => EvdevKey::KEY_X,
            "y" => EvdevKey::KEY_Y,
            "z" => EvdevKey::KEY_Z,
            "0" => EvdevKey::KEY_0,
            "1" => EvdevKey::KEY_1,
            "2" => EvdevKey::KEY_2,
            "3" => EvdevKey::KEY_3,
            "4" => EvdevKey::KEY_4,
            "5" => EvdevKey::KEY_5,
            "6" => EvdevKey::KEY_6,
            "7" => EvdevKey::KEY_7,
            "8" => EvdevKey::KEY_8,
            "9" => EvdevKey::KEY_9,
            _ => return None,
        };
        Some(evdev_key.code())
    }
}

#[async_trait]
impl PlatformManager for LinuxPlatform {
    async fn register_shortcut(&mut self, shortcut: &ShortcutDefinition) -> ShortcutResult<()> {
        let id = shortcut
            .id
            .ok_or_else(|| crate::models::ShortcutError::Internal("ID required".to_string()))?;

        let mut shortcuts = self.shortcuts.lock().await;
        shortcuts.insert(id, shortcut.shortcut_key.clone());

        let hotkey = self.parse_shortcut(&shortcut.shortcut_key)?;

        match &mut self.implementation {
            LinuxImpl::GlobalHotKey(manager, hotkeys) => {
                manager.register(hotkey).map_err(|e| {
                    crate::models::ShortcutError::PlatformError(format!("Register failed: {}", e))
                })?;

                let mut registered_hotkeys = hotkeys.lock().await;
                registered_hotkeys.insert(id, hotkey);

                {
                    let mut started = self.event_loop_started.lock().await;
                    if !*started {
                        *started = true;
                        let hotkeys_clone = hotkeys.clone();
                        let registry_clone = self.registry.clone();
                        tokio::spawn(async move {
                            info!("Linux X11 event loop started");
                            loop {
                                if let Ok(event) = GlobalHotKeyEvent::receiver().try_recv()
                                    && event.state == global_hotkey::HotKeyState::Pressed
                                {
                                    let shortcut_id = {
                                        let registered_hotkeys = hotkeys_clone.lock().await;
                                        registered_hotkeys
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
                                            error!("Linux X11 shortcut execution failed: {}", e);
                                        }
                                    }
                                }
                                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                            }
                        });
                    }
                }

                info!(
                    "Linux (X11) shortcut registered: {} ({})",
                    shortcut.name, shortcut.shortcut_key
                );
            }
            LinuxImpl::Evdev => {
                {
                    let mut started = self.event_loop_started.lock().await;
                    if !*started {
                        *started = true;
                        let shortcuts_clone = self.shortcuts.clone();
                        let registry_clone = self.registry.clone();

                        std::thread::spawn(move || {
                            info!("Starting Linux evdev monitoring for Wayland compatibility");
                            // Create a new Tokio runtime for the evdev monitoring thread
                            let rt = tokio::runtime::Runtime::new()
                                .expect("Failed to create Tokio runtime");

                            // Enter the runtime context so that Handle::try_current() works
                            let _guard = rt.enter();

                            if let Err(e) =
                                Self::evdev_monitoring_loop(shortcuts_clone, registry_clone)
                            {
                                error!("Evdev monitoring failed: {}", e);
                                error!("Linux evdev shortcuts failed - this usually means:");
                                error!("1. User not in 'input' group");
                                error!("2. No permission to access /dev/input/event* devices");
                                error!("3. Running in sandboxed environment");
                            }
                        });
                    }
                }

                info!(
                    "Linux (evdev) shortcut registered: {} ({})",
                    shortcut.name, shortcut.shortcut_key
                );
            }
            LinuxImpl::Fallback => {
                info!(
                    "Linux (fallback) shortcut registered: {} ({})",
                    shortcut.name, shortcut.shortcut_key
                );
                warn!("Fallback implementation - shortcut not functional");
            }
        }

        Ok(())
    }

    async fn unregister_shortcut(&mut self, shortcut_id: i64) -> ShortcutResult<()> {
        let mut shortcuts = self.shortcuts.lock().await;
        let shortcut_key = shortcuts.remove(&shortcut_id);

        match &mut self.implementation {
            LinuxImpl::GlobalHotKey(manager, hotkeys) => {
                let mut registered_hotkeys = hotkeys.lock().await;
                if let Some(hotkey) = registered_hotkeys.remove(&shortcut_id) {
                    manager.unregister(hotkey).map_err(|e| {
                        crate::models::ShortcutError::PlatformError(format!(
                            "Unregister failed: {}",
                            e
                        ))
                    })?;
                    info!("Linux (X11) shortcut unregistered: {}", shortcut_id);
                } else {
                    warn!("Linux (X11) shortcut not found: {}", shortcut_id);
                }
            }
            _ => {
                if let Some(key) = shortcut_key {
                    info!("Linux shortcut unregistered: {} ({})", shortcut_id, key);
                } else {
                    warn!("Linux shortcut not found: {}", shortcut_id);
                }
            }
        }

        Ok(())
    }

    async fn is_available(&self) -> bool {
        match &self.implementation {
            LinuxImpl::GlobalHotKey(_, _) => true,
            LinuxImpl::Evdev => Self::can_use_evdev(),
            LinuxImpl::Fallback => true,
        }
    }

    async fn platform_name(&self) -> &str {
        match &self.implementation {
            LinuxImpl::GlobalHotKey(_, _) => "linux-x11",
            LinuxImpl::Evdev => "linux-evdev",
            LinuxImpl::Fallback => "linux-fallback",
        }
    }
}

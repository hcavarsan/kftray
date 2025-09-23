#[cfg(target_os = "linux")]
use std::collections::{
    HashMap,
    HashSet,
};
#[cfg(target_os = "linux")]
use std::sync::{
    Arc,
    Mutex,
};
#[cfg(target_os = "linux")]
use std::thread;

use tauri_plugin_global_shortcut::{
    Code,
    Modifiers,
    Shortcut,
};

pub fn parse_shortcut_string(shortcut_str: &str) -> Option<Shortcut> {
    let parts: Vec<&str> = shortcut_str.split('+').collect();
    if parts.is_empty() {
        return None;
    }

    let mut modifiers = Modifiers::empty();
    let mut key_code = None;

    for part in parts {
        let part = part.trim();
        match part.to_lowercase().as_str() {
            // Modifiers
            "ctrl" | "control" => modifiers |= Modifiers::CONTROL,
            "shift" => modifiers |= Modifiers::SHIFT,
            "alt" => modifiers |= Modifiers::ALT,
            "cmd" | "super" => modifiers |= Modifiers::SUPER,

            // Function keys
            "f1" => key_code = Some(Code::F1),
            "f2" => key_code = Some(Code::F2),
            "f3" => key_code = Some(Code::F3),
            "f4" => key_code = Some(Code::F4),
            "f5" => key_code = Some(Code::F5),
            "f6" => key_code = Some(Code::F6),
            "f7" => key_code = Some(Code::F7),
            "f8" => key_code = Some(Code::F8),
            "f9" => key_code = Some(Code::F9),
            "f10" => key_code = Some(Code::F10),
            "f11" => key_code = Some(Code::F11),
            "f12" => key_code = Some(Code::F12),

            // Letters
            "a" => key_code = Some(Code::KeyA),
            "b" => key_code = Some(Code::KeyB),
            "c" => key_code = Some(Code::KeyC),
            "d" => key_code = Some(Code::KeyD),
            "e" => key_code = Some(Code::KeyE),
            "f" => key_code = Some(Code::KeyF),
            "g" => key_code = Some(Code::KeyG),
            "h" => key_code = Some(Code::KeyH),
            "i" => key_code = Some(Code::KeyI),
            "j" => key_code = Some(Code::KeyJ),
            "k" => key_code = Some(Code::KeyK),
            "l" => key_code = Some(Code::KeyL),
            "m" => key_code = Some(Code::KeyM),
            "n" => key_code = Some(Code::KeyN),
            "o" => key_code = Some(Code::KeyO),
            "p" => key_code = Some(Code::KeyP),
            "q" => key_code = Some(Code::KeyQ),
            "r" => key_code = Some(Code::KeyR),
            "s" => key_code = Some(Code::KeyS),
            "t" => key_code = Some(Code::KeyT),
            "u" => key_code = Some(Code::KeyU),
            "v" => key_code = Some(Code::KeyV),
            "w" => key_code = Some(Code::KeyW),
            "x" => key_code = Some(Code::KeyX),
            "y" => key_code = Some(Code::KeyY),
            "z" => key_code = Some(Code::KeyZ),

            // Numbers
            "0" => key_code = Some(Code::Digit0),
            "1" => key_code = Some(Code::Digit1),
            "2" => key_code = Some(Code::Digit2),
            "3" => key_code = Some(Code::Digit3),
            "4" => key_code = Some(Code::Digit4),
            "5" => key_code = Some(Code::Digit5),
            "6" => key_code = Some(Code::Digit6),
            "7" => key_code = Some(Code::Digit7),
            "8" => key_code = Some(Code::Digit8),
            "9" => key_code = Some(Code::Digit9),

            // Arrow keys
            "arrowup" | "up" => key_code = Some(Code::ArrowUp),
            "arrowdown" | "down" => key_code = Some(Code::ArrowDown),
            "arrowleft" | "left" => key_code = Some(Code::ArrowLeft),
            "arrowright" | "right" => key_code = Some(Code::ArrowRight),

            // Special keys
            "space" => key_code = Some(Code::Space),
            "enter" => key_code = Some(Code::Enter),
            "tab" => key_code = Some(Code::Tab),
            "escape" => key_code = Some(Code::Escape),
            "backspace" => key_code = Some(Code::Backspace),
            "delete" => key_code = Some(Code::Delete),

            _ => {}
        }
    }

    key_code.and_then(|code| {
        if modifiers.is_empty() {
            None
        } else {
            Some(Shortcut::new(Some(modifiers), code))
        }
    })
}

#[cfg(target_os = "linux")]
mod linux_shortcuts {
    use std::io;
    use std::os::unix::io::AsRawFd;

    use evdev::{
        Device,
        EventType,
        Key as EvdevKey,
    };
    use log::{
        error,
        info,
        warn,
    };
    use mio::{
        Events,
        Interest,
        Poll,
        Token,
        unix::SourceFd,
    };

    use super::*;

    type ShortcutCallback = Arc<dyn Fn() + Send + Sync>;
    type ShortcutMap = Arc<Mutex<HashMap<ShortcutCombo, ShortcutCallback>>>;

    #[derive(Clone, Debug, Hash, PartialEq, Eq)]
    pub struct ShortcutCombo {
        pub modifiers: u8,
        pub key: u16,
    }

    impl ShortcutCombo {
        pub fn from_tauri_shortcut(shortcut: &Shortcut) -> Option<Self> {
            let mut modifiers = 0u8;
            let mods = shortcut.mods;
            if mods.contains(Modifiers::CONTROL) {
                modifiers |= 1;
            }
            if mods.contains(Modifiers::SHIFT) {
                modifiers |= 2;
            }
            if mods.contains(Modifiers::ALT) {
                modifiers |= 4;
            }
            if mods.contains(Modifiers::SUPER) {
                modifiers |= 8;
            }

            let key = tauri_code_to_evdev(shortcut.key)?;
            Some(ShortcutCombo { modifiers, key })
        }
    }

    fn tauri_code_to_evdev(code: Code) -> Option<u16> {
        let evdev_key = match code {
            Code::KeyA => EvdevKey::KEY_A,
            Code::KeyB => EvdevKey::KEY_B,
            Code::KeyC => EvdevKey::KEY_C,
            Code::KeyD => EvdevKey::KEY_D,
            Code::KeyE => EvdevKey::KEY_E,
            Code::KeyF => EvdevKey::KEY_F,
            Code::KeyG => EvdevKey::KEY_G,
            Code::KeyH => EvdevKey::KEY_H,
            Code::KeyI => EvdevKey::KEY_I,
            Code::KeyJ => EvdevKey::KEY_J,
            Code::KeyK => EvdevKey::KEY_K,
            Code::KeyL => EvdevKey::KEY_L,
            Code::KeyM => EvdevKey::KEY_M,
            Code::KeyN => EvdevKey::KEY_N,
            Code::KeyO => EvdevKey::KEY_O,
            Code::KeyP => EvdevKey::KEY_P,
            Code::KeyQ => EvdevKey::KEY_Q,
            Code::KeyR => EvdevKey::KEY_R,
            Code::KeyS => EvdevKey::KEY_S,
            Code::KeyT => EvdevKey::KEY_T,
            Code::KeyU => EvdevKey::KEY_U,
            Code::KeyV => EvdevKey::KEY_V,
            Code::KeyW => EvdevKey::KEY_W,
            Code::KeyX => EvdevKey::KEY_X,
            Code::KeyY => EvdevKey::KEY_Y,
            Code::KeyZ => EvdevKey::KEY_Z,
            Code::Digit0 => EvdevKey::KEY_0,
            Code::Digit1 => EvdevKey::KEY_1,
            Code::Digit2 => EvdevKey::KEY_2,
            Code::Digit3 => EvdevKey::KEY_3,
            Code::Digit4 => EvdevKey::KEY_4,
            Code::Digit5 => EvdevKey::KEY_5,
            Code::Digit6 => EvdevKey::KEY_6,
            Code::Digit7 => EvdevKey::KEY_7,
            Code::Digit8 => EvdevKey::KEY_8,
            Code::Digit9 => EvdevKey::KEY_9,
            Code::F1 => EvdevKey::KEY_F1,
            Code::F2 => EvdevKey::KEY_F2,
            Code::F3 => EvdevKey::KEY_F3,
            Code::F4 => EvdevKey::KEY_F4,
            Code::F5 => EvdevKey::KEY_F5,
            Code::F6 => EvdevKey::KEY_F6,
            Code::F7 => EvdevKey::KEY_F7,
            Code::F8 => EvdevKey::KEY_F8,
            Code::F9 => EvdevKey::KEY_F9,
            Code::F10 => EvdevKey::KEY_F10,
            Code::F11 => EvdevKey::KEY_F11,
            Code::F12 => EvdevKey::KEY_F12,
            Code::Space => EvdevKey::KEY_SPACE,
            Code::Enter => EvdevKey::KEY_ENTER,
            Code::Tab => EvdevKey::KEY_TAB,
            Code::Escape => EvdevKey::KEY_ESC,
            Code::Backspace => EvdevKey::KEY_BACKSPACE,
            Code::Delete => EvdevKey::KEY_DELETE,
            Code::ArrowUp => EvdevKey::KEY_UP,
            Code::ArrowDown => EvdevKey::KEY_DOWN,
            Code::ArrowLeft => EvdevKey::KEY_LEFT,
            Code::ArrowRight => EvdevKey::KEY_RIGHT,
            _ => return None,
        };
        Some(evdev_key.code())
    }

    pub struct LinuxGlobalShortcutManager {
        shortcuts: ShortcutMap,
        is_running: Arc<Mutex<bool>>,
        thread_handle: Option<thread::JoinHandle<()>>,
        last_trigger: Arc<Mutex<std::time::Instant>>,
    }

    impl Default for LinuxGlobalShortcutManager {
        fn default() -> Self {
            Self::new()
        }
    }

    impl LinuxGlobalShortcutManager {
        pub fn new() -> Self {
            Self {
                shortcuts: Arc::new(Mutex::new(HashMap::new())),
                is_running: Arc::new(Mutex::new(false)),
                thread_handle: None,
                last_trigger: Arc::new(Mutex::new(std::time::Instant::now())),
            }
        }

        pub fn register_shortcut<F>(
            &mut self, shortcut_str: &str, callback: F,
        ) -> Result<(), Box<dyn std::error::Error>>
        where
            F: Fn() + Send + Sync + 'static,
        {
            if let Some(shortcut) = crate::shortcut::parse_shortcut_string(shortcut_str) {
                if let Some(combo) = ShortcutCombo::from_tauri_shortcut(&shortcut) {
                    let should_start_monitoring = {
                        let mut shortcuts = self.shortcuts.lock().unwrap();
                        shortcuts.insert(combo, Arc::new(callback));
                        !*self.is_running.lock().unwrap()
                    };

                    if should_start_monitoring {
                        self.start_monitoring()?;
                    }
                    Ok(())
                } else {
                    Err("Failed to convert shortcut to Linux format".into())
                }
            } else {
                Err("Failed to parse shortcut string".into())
            }
        }

        pub fn unregister_shortcut(
            &mut self, shortcut_str: &str,
        ) -> Result<(), Box<dyn std::error::Error>> {
            if let Some(shortcut) = crate::shortcut::parse_shortcut_string(shortcut_str) {
                if let Some(combo) = ShortcutCombo::from_tauri_shortcut(&shortcut) {
                    let mut shortcuts = self.shortcuts.lock().unwrap();
                    shortcuts.remove(&combo);
                    Ok(())
                } else {
                    Err("Failed to convert shortcut to Linux format".into())
                }
            } else {
                Err("Failed to parse shortcut string".into())
            }
        }

        fn start_monitoring(&mut self) -> Result<(), Box<dyn std::error::Error>> {
            info!("Starting Linux keyboard monitoring for global shortcuts");

            if let Ok(appimage_path) = std::env::var("APPIMAGE") {
                warn!("Running inside AppImage: {}", appimage_path);
                warn!("AppImage sandboxing may prevent access to /dev/input devices");
            }

            if let Ok(appdir) = std::env::var("APPDIR") {
                warn!("AppImage APPDIR detected: {}", appdir);
            }

            let devices = Self::discover_keyboard_devices()?;
            if devices.is_empty() {
                return Err(
                    "No accessible keyboard devices found. Check permissions or user groups."
                        .into(),
                );
            }

            info!("Found {} keyboard device(s)", devices.len());
            for (_, path) in &devices {
                info!("Keyboard device: {}", path);
            }

            *self.is_running.lock().unwrap() = true;

            let shortcuts = Arc::clone(&self.shortcuts);
            let is_running = Arc::clone(&self.is_running);
            let last_trigger = Arc::clone(&self.last_trigger);

            let handle = thread::spawn(move || {
                if let Err(e) = Self::keyboard_monitoring_loop(shortcuts, is_running, last_trigger)
                {
                    error!("Keyboard monitoring error: {}", e);
                    error!("Linux global shortcuts failed - this usually means:");
                    error!("1. User not in 'input' group");
                    error!("2. No permission to access /dev/input/event* devices");
                    error!("3. Running in sandboxed environment (AppImage/Flatpak)");
                }
            });

            self.thread_handle = Some(handle);
            info!("Linux keyboard monitoring thread started successfully");
            Ok(())
        }

        fn keyboard_monitoring_loop(
            shortcuts: ShortcutMap, is_running: Arc<Mutex<bool>>,
            last_trigger: Arc<Mutex<std::time::Instant>>,
        ) -> Result<(), Box<dyn std::error::Error>> {
            let devices = Self::discover_keyboard_devices()?;
            if devices.is_empty() {
                return Err("No keyboard devices found".into());
            }

            info!(
                "Starting keyboard monitoring loop with {} devices",
                devices.len()
            );

            let mut poll = Poll::new()?;
            let mut events = Events::with_capacity(32);
            let mut device_map = HashMap::new();
            let mut token_counter = 1;

            info!("Using non-exclusive monitoring - no virtual device needed");

            let devices_count = devices.len();
            let mut registered_count = 0;
            for (device, path) in devices {
                info!(
                    "Registering device for monitoring (non-exclusive): {}",
                    path
                );
                let token = Token(token_counter);
                let mut source_fd = SourceFd(&device.as_raw_fd());
                match poll
                    .registry()
                    .register(&mut source_fd, token, Interest::READABLE)
                {
                    Ok(_) => {
                        info!("Successfully registered device for monitoring: {}", path);
                        device_map.insert(token, (device, path));
                        token_counter += 1;
                        registered_count += 1;
                    }
                    Err(e) => {
                        error!("Failed to register device {} with poll: {}", path, e);
                        continue;
                    }
                }
            }

            if device_map.is_empty() {
                let mut error_msg =
                    "Failed to register any keyboard devices for monitoring.".to_string();

                if std::env::var("APPIMAGE").is_ok() || std::env::var("APPDIR").is_ok() {
                    error_msg.push_str(" AppImage detected: try extracting and running the binary directly, or use a system package (deb/rpm).");
                } else {
                    error_msg.push_str(" Check permissions or try running as root.");
                }

                return Err(error_msg.into());
            }

            info!(
                "Successfully registered {} out of {} keyboard devices for non-exclusive monitoring",
                registered_count, devices_count
            );

            let mut pressed_keys = HashSet::new();
            info!("Starting keyboard event monitoring loop");

            while *is_running.lock().unwrap() {
                if let Err(e) = poll.poll(&mut events, Some(std::time::Duration::from_millis(100)))
                {
                    eprintln!("Poll error: {}", e);
                    continue;
                }

                for event in &events {
                    if let Some((device, _)) = device_map.get_mut(&event.token()) {
                        match device.fetch_events() {
                            Ok(input_events) => {
                                for input_event in input_events {
                                    if input_event.event_type() == EventType::KEY {
                                        let key_code = input_event.code();
                                        let value = input_event.value();

                                        match value {
                                            1 => {
                                                pressed_keys.insert(key_code);
                                                if let Some(callback) = Self::check_shortcut_match(
                                                    &pressed_keys,
                                                    &shortcuts,
                                                ) {
                                                    let mut last = last_trigger.lock().unwrap();
                                                    let now = std::time::Instant::now();
                                                    if now.duration_since(*last)
                                                        > std::time::Duration::from_millis(250)
                                                    {
                                                        info!("Global shortcut triggered!");
                                                        *last = now;
                                                        drop(last);
                                                        callback();
                                                    }
                                                }
                                            }
                                            0 => {
                                                pressed_keys.remove(&key_code);
                                            }
                                            _ => {}
                                        }
                                    }
                                    // All events are passed through to other
                                    // applications automatically
                                    // since we're not grabbing the device
                                    // exclusively
                                }
                            }
                            Err(e) => {
                                if e.kind() == io::ErrorKind::UnexpectedEof {
                                    warn!("Device disconnected: {}", e);
                                    continue;
                                }
                                warn!("Failed to fetch events: {}", e);
                            }
                        }
                    }
                }
            }

            Ok(())
        }

        fn discover_keyboard_devices() -> Result<Vec<(Device, String)>, Box<dyn std::error::Error>>
        {
            use std::fs;
            use std::path::Path;

            let mut devices = Vec::new();
            let input_dir = Path::new("/dev/input");

            if !input_dir.exists() {
                return Err("No /dev/input directory found".into());
            }

            let entries = fs::read_dir(input_dir)?;

            for entry in entries {
                let entry = entry?;
                let path = entry.path();

                if let Some(filename) = path.file_name() {
                    if let Some(name) = filename.to_str() {
                        if name.starts_with("event") {
                            info!("Attempting to open device: {}", path.display());
                            match Device::open(&path) {
                                Ok(device) => {
                                    info!("Successfully opened device: {}", path.display());
                                    if Self::is_keyboard_device(&device) {
                                        info!("Device {} is a keyboard device", path.display());
                                        let path_str = path.to_string_lossy().to_string();
                                        devices.push((device, path_str));
                                    } else {
                                        info!("Device {} is not a keyboard device", path.display());
                                    }
                                }
                                Err(e) => {
                                    error!(
                                        "Failed to open device {}: {} (errno: {})",
                                        path.display(),
                                        e,
                                        e.raw_os_error().unwrap_or(-1)
                                    );
                                    continue;
                                }
                            }
                        }
                    }
                }
            }

            Ok(devices)
        }

        fn is_keyboard_device(device: &Device) -> bool {
            if let Some(keys) = device.supported_keys() {
                let has_basic_keys = keys.contains(EvdevKey::KEY_A)
                    && keys.contains(EvdevKey::KEY_S)
                    && keys.contains(EvdevKey::KEY_D)
                    && keys.contains(EvdevKey::KEY_F);
                return has_basic_keys;
            }
            false
        }

        fn check_shortcut_match(
            pressed_keys: &HashSet<u16>, shortcuts: &ShortcutMap,
        ) -> Option<ShortcutCallback> {
            let shortcuts = shortcuts.lock().unwrap();

            for (combo, callback) in shortcuts.iter() {
                let mut matches = true;

                if combo.modifiers & 1 != 0
                    && !pressed_keys.contains(&EvdevKey::KEY_LEFTCTRL.code())
                    && !pressed_keys.contains(&EvdevKey::KEY_RIGHTCTRL.code())
                {
                    matches = false;
                }

                if combo.modifiers & 2 != 0
                    && !pressed_keys.contains(&EvdevKey::KEY_LEFTSHIFT.code())
                    && !pressed_keys.contains(&EvdevKey::KEY_RIGHTSHIFT.code())
                {
                    matches = false;
                }

                if combo.modifiers & 4 != 0
                    && !pressed_keys.contains(&EvdevKey::KEY_LEFTALT.code())
                    && !pressed_keys.contains(&EvdevKey::KEY_RIGHTALT.code())
                {
                    matches = false;
                }

                if combo.modifiers & 8 != 0
                    && !pressed_keys.contains(&EvdevKey::KEY_LEFTMETA.code())
                    && !pressed_keys.contains(&EvdevKey::KEY_RIGHTMETA.code())
                {
                    matches = false;
                }

                if !pressed_keys.contains(&combo.key) {
                    matches = false;
                }

                if matches {
                    return Some(Arc::clone(callback));
                }
            }

            None
        }

        pub fn stop(&mut self) {
            *self.is_running.lock().unwrap() = false;

            if let Some(handle) = self.thread_handle.take() {
                let _ = handle.join();
            }
        }
    }

    impl Drop for LinuxGlobalShortcutManager {
        fn drop(&mut self) {
            self.stop();
        }
    }
}

#[cfg(target_os = "linux")]
use linux_shortcuts::LinuxGlobalShortcutManager;

pub enum GlobalShortcutManager {
    #[cfg(target_os = "linux")]
    Linux(LinuxGlobalShortcutManager),
    #[cfg(not(target_os = "linux"))]
    Tauri,
}

impl Default for GlobalShortcutManager {
    fn default() -> Self {
        Self::new()
    }
}

impl GlobalShortcutManager {
    pub fn new() -> Self {
        #[cfg(target_os = "linux")]
        {
            GlobalShortcutManager::Linux(LinuxGlobalShortcutManager::new())
        }
        #[cfg(not(target_os = "linux"))]
        {
            GlobalShortcutManager::Tauri
        }
    }

    pub fn register_shortcut<F>(
        &mut self, shortcut_str: &str, callback: F,
    ) -> Result<(), Box<dyn std::error::Error>>
    where
        F: Fn() + Send + Sync + 'static,
    {
        match self {
            #[cfg(target_os = "linux")]
            GlobalShortcutManager::Linux(manager) => {
                manager.register_shortcut(shortcut_str, callback)
            }
            #[cfg(not(target_os = "linux"))]
            GlobalShortcutManager::Tauri => {
                let _ = (shortcut_str, callback);
                Ok(())
            }
        }
    }

    pub fn unregister_shortcut(
        &mut self, shortcut_str: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            #[cfg(target_os = "linux")]
            GlobalShortcutManager::Linux(manager) => manager.unregister_shortcut(shortcut_str),
            #[cfg(not(target_os = "linux"))]
            GlobalShortcutManager::Tauri => {
                let _ = shortcut_str;
                Ok(())
            }
        }
    }
}

use std::collections::HashMap;

use crate::models::{
    ShortcutError,
    ShortcutResult,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedShortcut {
    pub modifiers: u8,
    pub key: u16,
    pub display_string: String,
}

impl ParsedShortcut {
    pub fn new(modifiers: u8, key: u16, display_string: String) -> Self {
        Self {
            modifiers,
            key,
            display_string,
        }
    }

    pub fn has_ctrl(&self) -> bool {
        self.modifiers & 1 != 0
    }

    pub fn has_shift(&self) -> bool {
        self.modifiers & 2 != 0
    }

    pub fn has_alt(&self) -> bool {
        self.modifiers & 4 != 0
    }

    pub fn has_super(&self) -> bool {
        self.modifiers & 8 != 0
    }
}

pub struct ShortcutParser {
    key_mappings: HashMap<String, u16>,
}

impl Default for ShortcutParser {
    fn default() -> Self {
        Self::new()
    }
}

impl ShortcutParser {
    pub fn new() -> Self {
        let mut key_mappings = HashMap::new();

        for (key, value) in [
            ("f1", 59),
            ("f2", 60),
            ("f3", 61),
            ("f4", 62),
            ("f5", 63),
            ("f6", 64),
            ("f7", 65),
            ("f8", 66),
            ("f9", 67),
            ("f10", 68),
            ("f11", 87),
            ("f12", 88),
            ("a", 30),
            ("b", 48),
            ("c", 46),
            ("d", 32),
            ("e", 18),
            ("f", 33),
            ("g", 34),
            ("h", 35),
            ("i", 23),
            ("j", 36),
            ("k", 37),
            ("l", 38),
            ("m", 50),
            ("n", 49),
            ("o", 24),
            ("p", 25),
            ("q", 16),
            ("r", 19),
            ("s", 31),
            ("t", 20),
            ("u", 22),
            ("v", 47),
            ("w", 17),
            ("x", 45),
            ("y", 21),
            ("z", 44),
            ("0", 11),
            ("1", 2),
            ("2", 3),
            ("3", 4),
            ("4", 5),
            ("5", 6),
            ("6", 7),
            ("7", 8),
            ("8", 9),
            ("9", 10),
            ("arrowup", 103),
            ("up", 103),
            ("arrowdown", 108),
            ("down", 108),
            ("arrowleft", 105),
            ("left", 105),
            ("arrowright", 106),
            ("right", 106),
            ("space", 57),
            ("enter", 28),
            ("tab", 15),
            ("escape", 1),
            ("backspace", 14),
            ("delete", 111),
            ("home", 102),
            ("end", 107),
            ("pageup", 104),
            ("pagedown", 109),
            ("insert", 110),
            ("print", 99),
            ("pause", 119),
            ("capslock", 58),
            ("numlock", 69),
            ("scrolllock", 70),
        ] {
            key_mappings.insert(key.to_string(), value);
        }

        Self { key_mappings }
    }

    pub fn parse(&self, shortcut_str: &str) -> ShortcutResult<ParsedShortcut> {
        let parts: Vec<&str> = shortcut_str.split('+').map(|p| p.trim()).collect();

        if parts.is_empty() {
            return Err(ShortcutError::InvalidShortcut(
                "Empty shortcut string".to_string(),
            ));
        }

        let mut modifiers = 0u8;
        let mut key_code = None;
        let mut processed_parts: Vec<String> = Vec::new();

        for part in parts {
            let part_lower = part.to_lowercase();
            match part_lower.as_str() {
                "ctrl" | "control" => {
                    modifiers |= 1;
                    processed_parts.push("Ctrl".to_string());
                }
                "shift" => {
                    modifiers |= 2;
                    processed_parts.push("Shift".to_string());
                }
                "alt" => {
                    modifiers |= 4;
                    processed_parts.push("Alt".to_string());
                }
                "cmd" | "super" | "meta" => {
                    modifiers |= 8;
                    processed_parts.push("Super".to_string());
                }
                _ => {
                    if let Some(&code) = self.key_mappings.get(&part_lower) {
                        if key_code.is_some() {
                            return Err(ShortcutError::InvalidShortcut(format!(
                                "Multiple keys specified: {}",
                                shortcut_str
                            )));
                        }
                        key_code = Some(code);
                        processed_parts.push(self.format_key_display(&part_lower));
                    } else {
                        return Err(ShortcutError::InvalidShortcut(format!(
                            "Unknown key: {}",
                            part
                        )));
                    }
                }
            }
        }

        let key = key_code
            .ok_or_else(|| ShortcutError::InvalidShortcut("No key specified".to_string()))?;

        if modifiers == 0 {
            return Err(ShortcutError::InvalidShortcut(
                "At least one modifier key is required".to_string(),
            ));
        }

        let display_string = processed_parts.join("+");

        Ok(ParsedShortcut::new(modifiers, key, display_string))
    }

    fn format_key_display(&self, key: &str) -> String {
        match key {
            "arrowup" => "Up".to_string(),
            "arrowdown" => "Down".to_string(),
            "arrowleft" => "Left".to_string(),
            "arrowright" => "Right".to_string(),
            "pageup" => "PageUp".to_string(),
            "pagedown" => "PageDown".to_string(),
            "capslock" => "CapsLock".to_string(),
            "numlock" => "NumLock".to_string(),
            "scrolllock" => "ScrollLock".to_string(),
            key if key.starts_with('f') && key.len() <= 3 => key.to_uppercase(),
            _ => {
                let mut chars: Vec<char> = key.chars().collect();
                if let Some(first_char) = chars.first_mut() {
                    *first_char = first_char.to_uppercase().next().unwrap_or(*first_char);
                }
                chars.into_iter().collect()
            }
        }
    }

    pub fn normalize_shortcut(&self, shortcut_str: &str) -> ShortcutResult<String> {
        let parsed = self.parse(shortcut_str)?;
        Ok(parsed.display_string)
    }

    pub fn validate_shortcut(&self, shortcut_str: &str) -> ShortcutResult<bool> {
        match self.parse(shortcut_str) {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}

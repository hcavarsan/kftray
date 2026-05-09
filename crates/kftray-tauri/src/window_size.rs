use tauri::{
    WebviewWindow,
    Wry,
};

const BASE_WIDTH: u32 = 450;
const BASE_HEIGHT: u32 = 500;
const MONITOR_FILL_RATIO: f32 = 0.9;

pub const SETTING_KEY: &str = "window_size_preset";

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum WindowSizePreset {
    #[default]
    Default,
    Medium,
    Large,
    ExtraLarge,
}

impl WindowSizePreset {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "default" => Some(Self::Default),
            "medium" => Some(Self::Medium),
            "large" => Some(Self::Large),
            "xl" => Some(Self::ExtraLarge),
            _ => None,
        }
    }

    pub fn as_id(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Medium => "medium",
            Self::Large => "large",
            Self::ExtraLarge => "xl",
        }
    }

    pub fn scale(self) -> f32 {
        match self {
            Self::Default => 1.0,
            Self::Medium => 1.5,
            Self::Large => 2.0,
            Self::ExtraLarge => 2.5,
        }
    }

    pub fn dimensions(self, window: &WebviewWindow<Wry>) -> (u32, u32) {
        let mut s = self.scale();
        if let Ok(Some(monitor)) = window.current_monitor() {
            let avail = monitor.size();
            if avail.width > 0 && avail.height > 0 {
                let max_w = avail.width as f32 * MONITOR_FILL_RATIO / BASE_WIDTH as f32;
                let max_h = avail.height as f32 * MONITOR_FILL_RATIO / BASE_HEIGHT as f32;
                s = s.min(max_w).min(max_h);
            }
        }
        (
            (BASE_WIDTH as f32 * s).round() as u32,
            (BASE_HEIGHT as f32 * s).round() as u32,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_ids() {
        for preset in [
            WindowSizePreset::Default,
            WindowSizePreset::Medium,
            WindowSizePreset::Large,
            WindowSizePreset::ExtraLarge,
        ] {
            assert_eq!(WindowSizePreset::from_id(preset.as_id()), Some(preset));
        }
    }

    #[test]
    fn unknown_id_is_none() {
        assert_eq!(WindowSizePreset::from_id("xxl"), None);
    }

    #[test]
    fn scales_match_ladder() {
        assert_eq!(WindowSizePreset::Default.scale(), 1.0);
        assert_eq!(WindowSizePreset::Medium.scale(), 1.5);
        assert_eq!(WindowSizePreset::Large.scale(), 2.0);
        assert_eq!(WindowSizePreset::ExtraLarge.scale(), 2.5);
    }
}

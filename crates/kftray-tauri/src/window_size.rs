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
        let available_logical = window.current_monitor().ok().flatten().map(|monitor| {
            let logical = monitor.size().to_logical::<f64>(monitor.scale_factor());
            (logical.width, logical.height)
        });
        compute_dimensions(self.scale(), available_logical)
    }
}

fn compute_dimensions(base_scale: f32, available_logical: Option<(f64, f64)>) -> (u32, u32) {
    let mut s = base_scale;
    if let Some((avail_w, avail_h)) = available_logical
        && avail_w > 0.0
        && avail_h > 0.0
    {
        let max_w = (avail_w as f32) * MONITOR_FILL_RATIO / BASE_WIDTH as f32;
        let max_h = (avail_h as f32) * MONITOR_FILL_RATIO / BASE_HEIGHT as f32;
        s = s.min(max_w).min(max_h);
    }
    (
        (BASE_WIDTH as f32 * s).round() as u32,
        (BASE_HEIGHT as f32 * s).round() as u32,
    )
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

    #[test]
    fn compute_dimensions_returns_base_when_no_monitor_info() {
        assert_eq!(compute_dimensions(1.0, None), (450, 500));
        assert_eq!(compute_dimensions(2.5, None), (1125, 1250));
    }

    #[test]
    fn compute_dimensions_zero_size_monitor_falls_back_to_base() {
        assert_eq!(compute_dimensions(1.0, Some((0.0, 0.0))), (450, 500));
        assert_eq!(compute_dimensions(1.0, Some((1920.0, 0.0))), (450, 500));
    }

    #[test]
    fn compute_dimensions_default_preset_unchanged_on_typical_displays() {
        assert_eq!(compute_dimensions(1.0, Some((1920.0, 1080.0))), (450, 500));
        assert_eq!(compute_dimensions(1.0, Some((1512.0, 982.0))), (450, 500));
        assert_eq!(compute_dimensions(1.0, Some((3840.0, 2160.0))), (450, 500));
        assert_eq!(compute_dimensions(1.0, Some((1366.0, 768.0))), (450, 500));
    }

    #[test]
    fn compute_dimensions_clamps_when_monitor_too_small() {
        let (w, h) = compute_dimensions(1.0, Some((600.0, 400.0)));
        assert_eq!((w, h), (324, 360));
    }

    #[test]
    fn compute_dimensions_xl_preset_clamps_to_monitor_height() {
        let unclamped = compute_dimensions(2.5, None);
        assert_eq!(unclamped, (1125, 1250));

        let (w, h) = compute_dimensions(2.5, Some((1512.0, 982.0)));
        assert_eq!((w, h), (795, 884));
        assert!((w as f32) < 1125.0 && (h as f32) < 1250.0);
    }

    #[test]
    fn compute_dimensions_uses_full_scale_when_monitor_is_large_enough() {
        assert_eq!(compute_dimensions(1.5, Some((3840.0, 2160.0))), (675, 750));
        assert_eq!(compute_dimensions(2.0, Some((3840.0, 2160.0))), (900, 1000));
    }
}

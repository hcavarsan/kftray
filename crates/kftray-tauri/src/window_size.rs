use std::sync::atomic::{
    AtomicU64,
    Ordering,
};
use std::time::Duration;

use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_commons::utils::settings::{
    delete_setting_with_mode,
    get_setting,
    set_setting,
};
use log::warn;

const SETTING_KEY: &str = "window_size";
const LEGACY_PRESET_KEY: &str = "window_size_preset";
const BASE_WIDTH: f64 = 450.0;
const BASE_HEIGHT: f64 = 500.0;
const SAVE_DELAY: Duration = Duration::from_millis(500);

static PENDING_SAVE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowSize {
    pub width: u32,
    pub height: u32,
}

impl WindowSize {
    fn parse(value: &str) -> Option<Self> {
        let (width, height) = value.split_once('x')?;
        let size = Self {
            width: width.parse().ok()?,
            height: height.parse().ok()?,
        };
        (size.width > 0 && size.height > 0).then_some(size)
    }

    fn from_legacy_preset(id: &str) -> Option<Self> {
        let scale = match id {
            "xs" => 0.5,
            "small" => 0.75,
            "default" => 1.0,
            "medium" => 1.5,
            "large" => 2.0,
            "xl" => 2.5,
            _ => return None,
        };
        Some(Self {
            width: (BASE_WIDTH * scale).round() as u32,
            height: (BASE_HEIGHT * scale).round() as u32,
        })
    }

    fn to_setting(self) -> String {
        format!("{}x{}", self.width, self.height)
    }

    pub fn fit_within(self, available: Option<(f64, f64)>) -> Self {
        match available {
            Some((width, height)) if width > 0.0 && height > 0.0 => Self {
                width: self.width.min(width.floor() as u32),
                height: self.height.min(height.floor() as u32),
            },
            _ => self,
        }
    }
}

pub async fn load() -> Option<WindowSize> {
    if let Ok(Some(value)) = get_setting(SETTING_KEY).await {
        return WindowSize::parse(&value);
    }
    let legacy = get_setting(LEGACY_PRESET_KEY).await.ok().flatten()?;
    let size = WindowSize::from_legacy_preset(&legacy)?;
    match set_setting(SETTING_KEY, &size.to_setting()).await {
        Ok(()) => {
            if let Err(e) = delete_setting_with_mode(LEGACY_PRESET_KEY, DatabaseMode::File).await {
                warn!("Failed to remove legacy window size preset: {e}");
            }
        }
        Err(e) => warn!("Failed to migrate window size preset {legacy}: {e}"),
    }
    Some(size)
}

pub fn save_after_resize(runtime: &tokio::runtime::Runtime, size: WindowSize) {
    let generation = PENDING_SAVE.fetch_add(1, Ordering::SeqCst) + 1;
    runtime.spawn(async move {
        tokio::time::sleep(SAVE_DELAY).await;
        if PENDING_SAVE.load(Ordering::SeqCst) != generation {
            return;
        }
        if let Err(e) = set_setting(SETTING_KEY, &size.to_setting()).await {
            warn!("Failed to save window size: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_saved_sizes() {
        assert_eq!(
            WindowSize::parse("675x750"),
            Some(WindowSize {
                width: 675,
                height: 750
            })
        );
        assert_eq!(WindowSize::parse("0x750"), None);
        assert_eq!(WindowSize::parse("675"), None);
        assert_eq!(WindowSize::parse("axb"), None);
    }

    #[test]
    fn saved_size_round_trips() {
        let size = WindowSize {
            width: 1200,
            height: 80,
        };
        assert_eq!(WindowSize::parse(&size.to_setting()), Some(size));
    }

    #[test]
    fn legacy_presets_keep_their_size() {
        let size = |id| WindowSize::from_legacy_preset(id).map(|s| (s.width, s.height));
        assert_eq!(size("xs"), Some((225, 250)));
        assert_eq!(size("small"), Some((338, 375)));
        assert_eq!(size("default"), Some((450, 500)));
        assert_eq!(size("medium"), Some((675, 750)));
        assert_eq!(size("large"), Some((900, 1000)));
        assert_eq!(size("xl"), Some((1125, 1250)));
        assert_eq!(size("xxl"), None);
    }

    #[test]
    fn fits_within_the_monitor() {
        let size = WindowSize {
            width: 1125,
            height: 1250,
        };
        assert_eq!(
            size.fit_within(Some((1512.0, 982.0))),
            WindowSize {
                width: 1125,
                height: 982
            }
        );
        assert_eq!(size.fit_within(None), size);
        assert_eq!(size.fit_within(Some((0.0, 0.0))), size);
    }
}

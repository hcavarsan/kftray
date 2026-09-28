use std::sync::Mutex;
use std::sync::atomic::Ordering;

use kftray_commons::models::window::AppState;
use log::{
    info,
    warn,
};
use serde::{
    Deserialize,
    Serialize,
};
use tauri::{
    AppHandle,
    Emitter,
    Manager,
    Wry,
};

pub const APP_MODE_SETTING_KEY: &str = "app_mode";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppMode {
    #[default]
    Tray,
    Window,
}

impl AppMode {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "tray" => Some(Self::Tray),
            "window" => Some(Self::Window),
            _ => None,
        }
    }

    pub fn as_id(self) -> &'static str {
        match self {
            Self::Tray => "tray",
            Self::Window => "window",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TrayMode {
    Tray,
    Window,
}

#[derive(Clone, Copy, Debug, Default)]
struct ModeInputs {
    preference: AppMode,
    tray_unavailable: bool,
}

impl ModeInputs {
    fn mode(self) -> TrayMode {
        if self.preference == AppMode::Window || self.tray_unavailable {
            TrayMode::Window
        } else {
            TrayMode::Tray
        }
    }
}

#[derive(Default)]
pub struct TrayModeState {
    inputs: Mutex<ModeInputs>,
}

impl TrayModeState {
    pub fn mode(&self) -> TrayMode {
        self.inputs().mode()
    }

    pub fn preference(&self) -> AppMode {
        self.inputs().preference
    }

    fn inputs(&self) -> ModeInputs {
        *self.inputs.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn update(&self, change: impl FnOnce(&mut ModeInputs)) -> Option<TrayMode> {
        let mut inputs = self.inputs.lock().unwrap_or_else(|e| e.into_inner());
        let before = inputs.mode();
        change(&mut inputs);
        let after = inputs.mode();
        (before != after).then_some(after)
    }
}

pub fn current(app: &AppHandle<Wry>) -> TrayMode {
    app.try_state::<TrayModeState>()
        .map_or(TrayMode::Tray, |state| state.mode())
}

pub fn preference(app: &AppHandle<Wry>) -> AppMode {
    app.try_state::<TrayModeState>()
        .map_or(AppMode::Tray, |state| state.preference())
}

pub fn set_preference(app: &AppHandle<Wry>, preference: AppMode) {
    let state = app.state::<TrayModeState>();
    let changed = state.preference() != preference;
    let switched = state.update(|inputs| inputs.preference = preference);
    if changed {
        crate::tray::set_tray_icon_visible(app, preference == AppMode::Tray);
    }
    if let Some(mode) = switched {
        switch_to(app, mode);
    }
}

#[cfg(target_os = "linux")]
pub fn set_tray_available(app: &AppHandle<Wry>, available: bool) {
    if !available {
        info!(
            "No system tray available. On GNOME, install the AppIndicator extension to use tray mode"
        );
    }
    if let Some(mode) = app
        .state::<TrayModeState>()
        .update(|inputs| inputs.tray_unavailable = !available)
    {
        switch_to(app, mode);
    }
}

pub async fn load_preference(app: &AppHandle<Wry>) {
    match kftray_commons::utils::settings::get_setting(APP_MODE_SETTING_KEY).await {
        Ok(Some(value)) => match AppMode::from_id(&value) {
            Some(preference) => set_preference(app, preference),
            None => warn!("Ignoring unknown app mode setting: {value}"),
        },
        Ok(None) => {}
        Err(e) => warn!("Failed to load app mode setting: {e}"),
    }
}

fn switch_to(app: &AppHandle<Wry>, mode: TrayMode) {
    info!("Switching kftray to {mode:?} mode");

    #[cfg(target_os = "macos")]
    {
        let policy = match mode {
            TrayMode::Tray => tauri::ActivationPolicy::Accessory,
            TrayMode::Window => tauri::ActivationPolicy::Regular,
        };
        if let Err(e) = app.set_activation_policy(policy) {
            warn!("Failed to set activation policy: {e}");
        }
    }

    if let Some(window) = app.get_webview_window("main") {
        if let Err(e) = window.set_skip_taskbar(mode == TrayMode::Tray) {
            warn!("Failed to update taskbar visibility: {e}");
        }
        if let Err(e) = window.set_resizable(mode == TrayMode::Window) {
            warn!("Failed to update window resizability: {e}");
        }
        match mode {
            TrayMode::Window => crate::window::show_centered_main_window(&window),
            TrayMode::Tray => {
                crate::window::forget_window_position(&window);
                let pinned = window.state::<AppState>().pinned.load(Ordering::SeqCst);
                if !pinned && let Err(e) = window.hide() {
                    warn!("Failed to hide window: {e}");
                }
            }
        }
    }

    if let Err(e) = app.emit("tray-mode-changed", mode) {
        warn!("Failed to emit tray mode event: {e}");
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WindowSnapshot {
    pub visible: bool,
    pub minimized: bool,
    pub focused: bool,
    pub pinned: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToggleAction {
    Show,
    Hide,
    Minimize,
    Keep,
}

pub fn toggle_action(mode: TrayMode, window: WindowSnapshot) -> ToggleAction {
    if !window.visible || window.minimized {
        return ToggleAction::Show;
    }

    match mode {
        TrayMode::Tray if window.pinned => ToggleAction::Keep,
        TrayMode::Tray => ToggleAction::Hide,
        TrayMode::Window if !window.focused => ToggleAction::Show,
        TrayMode::Window if window.pinned => ToggleAction::Keep,
        TrayMode::Window => ToggleAction::Minimize,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visible(focused: bool, pinned: bool) -> WindowSnapshot {
        WindowSnapshot {
            visible: true,
            minimized: false,
            focused,
            pinned,
        }
    }

    #[test]
    fn hidden_or_minimized_window_is_shown_in_both_modes() {
        let hidden = WindowSnapshot::default();
        let minimized = WindowSnapshot {
            visible: true,
            minimized: true,
            focused: false,
            pinned: true,
        };

        for mode in [TrayMode::Tray, TrayMode::Window] {
            assert_eq!(toggle_action(mode, hidden), ToggleAction::Show);
            assert_eq!(toggle_action(mode, minimized), ToggleAction::Show);
        }
    }

    #[test]
    fn tray_mode_hides_visible_window_unless_pinned() {
        assert_eq!(
            toggle_action(TrayMode::Tray, visible(false, false)),
            ToggleAction::Hide
        );
        assert_eq!(
            toggle_action(TrayMode::Tray, visible(true, false)),
            ToggleAction::Hide
        );
        assert_eq!(
            toggle_action(TrayMode::Tray, visible(true, true)),
            ToggleAction::Keep
        );
    }

    #[test]
    fn window_mode_raises_background_window_and_minimizes_focused_one() {
        assert_eq!(
            toggle_action(TrayMode::Window, visible(false, false)),
            ToggleAction::Show
        );
        assert_eq!(
            toggle_action(TrayMode::Window, visible(false, true)),
            ToggleAction::Show
        );
        assert_eq!(
            toggle_action(TrayMode::Window, visible(true, false)),
            ToggleAction::Minimize
        );
        assert_eq!(
            toggle_action(TrayMode::Window, visible(true, true)),
            ToggleAction::Keep
        );
    }

    #[test]
    fn window_mode_wins_when_preferred_or_tray_is_missing() {
        let cases = [
            (AppMode::Tray, false, TrayMode::Tray),
            (AppMode::Tray, true, TrayMode::Window),
            (AppMode::Window, false, TrayMode::Window),
            (AppMode::Window, true, TrayMode::Window),
        ];

        for (preference, tray_unavailable, expected) in cases {
            let inputs = ModeInputs {
                preference,
                tray_unavailable,
            };
            assert_eq!(inputs.mode(), expected, "{inputs:?}");
        }
    }

    #[test]
    fn update_reports_only_effective_mode_changes() {
        let state = TrayModeState::default();
        assert_eq!(state.mode(), TrayMode::Tray);

        assert_eq!(
            state.update(|i| i.tray_unavailable = true),
            Some(TrayMode::Window)
        );
        assert_eq!(state.update(|i| i.preference = AppMode::Window), None);
        assert_eq!(state.update(|i| i.tray_unavailable = false), None);
        assert_eq!(
            state.update(|i| i.preference = AppMode::Tray),
            Some(TrayMode::Tray)
        );
    }

    #[test]
    fn app_mode_ids_round_trip() {
        for mode in [AppMode::Tray, AppMode::Window] {
            assert_eq!(AppMode::from_id(mode.as_id()), Some(mode));
        }
        assert_eq!(AppMode::from_id("auto"), None);
    }
}

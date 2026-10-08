use anyhow::Result;

use crate::app::Desktop;
use crate::ui::TestId;

pub struct SettingsPage<'a> {
    desktop: &'a Desktop,
}

impl<'a> SettingsPage<'a> {
    pub(crate) fn new(desktop: &'a Desktop) -> Self {
        Self { desktop }
    }

    pub async fn open(self) -> Result<Self> {
        self.desktop.click(&TestId::ConfigMenu.by()).await?;
        self.desktop.click(&TestId::SettingsButton.by()).await?;
        let limit = self.desktop.timeouts().ui;
        self.desktop
            .wait_visible(&TestId::SettingsTelemetry.by(), limit)
            .await?;
        Ok(self)
    }

    pub async fn crash_reports_enabled(&self) -> Result<bool> {
        let state = self
            .desktop
            .attr(&TestId::SettingsTelemetry.by(), "data-state")
            .await?;
        Ok(state.as_deref() == Some("checked"))
    }

    pub async fn toggle_crash_reports(&self) -> Result<()> {
        self.desktop.click(&TestId::SettingsTelemetry.by()).await
    }

    pub async fn save(&self) -> Result<()> {
        self.desktop.click(&TestId::SettingsSave.by()).await?;
        let limit = self.desktop.timeouts().ui;
        self.desktop
            .wait_absent(&TestId::SettingsDialog.by(), limit)
            .await
    }
}

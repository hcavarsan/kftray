use anyhow::Result;
use thirtyfour::By;

use crate::app::Desktop;
use crate::ui::TestId;

const CRASH_REPORTS: &str = "crashReports";

pub struct ConsentPage<'a> {
    desktop: &'a Desktop,
}

impl<'a> ConsentPage<'a> {
    pub(crate) fn new(desktop: &'a Desktop) -> Self {
        Self { desktop }
    }

    pub async fn wait_visible(&self) -> Result<()> {
        let limit = self.desktop.timeouts().ui;
        self.desktop
            .wait_visible(&TestId::ConsentDialog.by(), limit)
            .await
    }

    pub async fn is_visible(&self) -> Result<bool> {
        self.desktop.is_visible(&TestId::ConsentDialog.by()).await
    }

    /// Crash reports are pre-checked in the dialog, so accepting is just
    /// confirming the default and saving.
    pub async fn accept_crash_reports(&self) -> Result<()> {
        let option = By::Css(format!(
            "{}[data-id='{CRASH_REPORTS}']",
            TestId::ConsentOption.css()
        ));
        let limit = self.desktop.timeouts().ui;
        self.desktop
            .wait_attr(&option, "data-state", "checked", limit)
            .await?;
        self.answer(TestId::ConsentSave).await
    }

    pub async fn decline(&self) -> Result<()> {
        self.answer(TestId::ConsentDecline).await
    }

    async fn answer(&self, button: TestId) -> Result<()> {
        self.desktop.click(&button.by()).await?;
        let limit = self.desktop.timeouts().ui;
        self.desktop
            .wait_absent(&TestId::ConsentDialog.by(), limit)
            .await
    }
}

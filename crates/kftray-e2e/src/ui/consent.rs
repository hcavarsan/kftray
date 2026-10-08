use anyhow::Result;

use crate::app::Desktop;
use crate::ui::TestId;

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

    pub async fn allow(&self) -> Result<()> {
        self.answer(TestId::ConsentAllow).await
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

use anyhow::Result;
use thirtyfour::By;

use crate::app::Desktop;
use crate::ui::TestId;

pub struct MainPage<'a> {
    desktop: &'a Desktop,
}

impl<'a> MainPage<'a> {
    pub(crate) fn new(desktop: &'a Desktop) -> Self {
        Self { desktop }
    }

    pub fn row(&self, alias: &str) -> ConfigRow<'a> {
        ConfigRow {
            desktop: self.desktop,
            selector: format!("{}[data-alias='{alias}']", TestId::ConfigRow.css()),
        }
    }

    pub async fn aliases(&self) -> Result<Vec<String>> {
        self.desktop
            .attrs(&TestId::ConfigRow.by(), "data-alias")
            .await
    }

    pub async fn wait_rows(&self, count: usize) -> Result<()> {
        let limit = self.desktop.timeouts().ui;
        self.desktop
            .wait_count(&TestId::ConfigRow.by(), count, limit)
            .await
    }

    pub async fn expand_groups(&self) -> Result<()> {
        let button = TestId::ExpandGroups.by();
        if self.desktop.attr(&button, "data-state").await?.as_deref() == Some("collapsed") {
            self.desktop.click(&button).await?;
        }
        Ok(())
    }

    pub async fn import_file(&self) -> Result<()> {
        self.desktop.click(&TestId::ConfigMenu.by()).await?;
        self.desktop.click(&TestId::MenuImport.by()).await
    }

    pub async fn export_file(&self) -> Result<()> {
        self.desktop.click(&TestId::ConfigMenu.by()).await?;
        self.desktop.click(&TestId::MenuExport.by()).await
    }
}

pub struct ConfigRow<'a> {
    desktop: &'a Desktop,
    selector: String,
}

impl ConfigRow<'_> {
    pub async fn start(&self) -> Result<()> {
        self.toggle_from("stopped").await
    }

    pub async fn stop(&self) -> Result<()> {
        self.toggle_from("running").await
    }

    pub async fn wait_present(&self) -> Result<()> {
        let limit = self.desktop.timeouts().ui;
        self.desktop.wait_present(&self.by(), limit).await
    }

    pub async fn wait_running(&self) -> Result<()> {
        self.wait_state("running").await
    }

    pub async fn wait_stopped(&self) -> Result<()> {
        self.wait_state("stopped").await
    }

    /// Waits for the warning a row shows when connections to the pod fail.
    pub async fn wait_connection_error(&self) -> Result<()> {
        let limit = self.desktop.timeouts().forward;
        let warning = By::Css(format!("{} {}", self.selector, TestId::ConfigError.css()));
        self.desktop.wait_present(&warning, limit).await
    }

    async fn toggle_from(&self, state: &str) -> Result<()> {
        let limit = self.desktop.timeouts().ui;
        self.desktop
            .wait_attr(&self.by(), "data-state", state, limit)
            .await?;
        self.desktop.main().expand_groups().await?;
        let toggle = By::Css(format!("{} {}", self.selector, TestId::ConfigToggle.css()));
        self.desktop.click(&toggle).await
    }

    async fn wait_state(&self, state: &str) -> Result<()> {
        let limit = self.desktop.timeouts().forward;
        self.desktop
            .wait_attr(&self.by(), "data-state", state, limit)
            .await
    }

    fn by(&self) -> By {
        By::Css(self.selector.clone())
    }
}

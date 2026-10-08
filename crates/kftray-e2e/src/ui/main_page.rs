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

    pub async fn expand_groups(&self) -> Result<()> {
        let button = TestId::ExpandGroups.by();
        if self.desktop.attr(&button, "data-state").await?.as_deref() == Some("collapsed") {
            self.desktop.click(&button).await?;
        }
        Ok(())
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

    pub async fn wait_running(&self) -> Result<()> {
        self.wait_state("running").await
    }

    pub async fn wait_stopped(&self) -> Result<()> {
        self.wait_state("stopped").await
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

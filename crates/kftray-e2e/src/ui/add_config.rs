use anyhow::Result;
use thirtyfour::{
    By,
    Key,
};

use crate::app::Desktop;
use crate::ui::TestId;

pub struct AddConfigPage<'a> {
    desktop: &'a Desktop,
}

impl<'a> AddConfigPage<'a> {
    pub(crate) fn new(desktop: &'a Desktop) -> Self {
        Self { desktop }
    }

    pub async fn open(self) -> Result<Self> {
        self.desktop.click(&TestId::AddConfigButton.by()).await?;
        let limit = self.desktop.timeouts().ui;
        self.desktop
            .wait_visible(&TestId::AddConfigDialog.by(), limit)
            .await?;
        Ok(self)
    }

    pub async fn alias(&self, alias: &str) -> Result<()> {
        self.type_into(TestId::ConfigAlias, alias).await
    }

    pub async fn context(&self, context: &str) -> Result<()> {
        self.choose(TestId::ConfigContext, context).await
    }

    pub async fn workload_type(&self, label: &str) -> Result<()> {
        self.choose(TestId::ConfigWorkloadType, label).await
    }

    pub async fn namespace(&self, namespace: &str) -> Result<()> {
        self.choose(TestId::ConfigNamespace, namespace).await
    }

    pub async fn service(&self, service: &str) -> Result<()> {
        self.choose(TestId::ConfigService, service).await
    }

    pub async fn protocol(&self, label: &str) -> Result<()> {
        self.choose(TestId::ConfigProtocol, label).await
    }

    pub async fn remote_port(&self, port: u16) -> Result<()> {
        self.choose(TestId::ConfigRemotePort, &port.to_string())
            .await
    }

    pub async fn local_port(&self, port: u16) -> Result<()> {
        self.type_into(TestId::ConfigLocalPort, &port.to_string())
            .await
    }

    pub async fn save(&self) -> Result<()> {
        self.desktop.click(&TestId::ConfigSave.by()).await?;
        let limit = self.desktop.timeouts().ui;
        self.desktop
            .wait_absent(&TestId::AddConfigDialog.by(), limit)
            .await
    }

    async fn type_into(&self, field: TestId, text: &str) -> Result<()> {
        let input = self.desktop.find(&input_of(field)).await?;
        input.send_keys(text).await?;
        Ok(())
    }

    async fn choose(&self, field: TestId, text: &str) -> Result<()> {
        let input = self.desktop.find(&input_of(field)).await?;
        input.send_keys(text).await?;
        let option = By::Css(format!("{} [role='option']", field.css()));
        let limit = self.desktop.timeouts().ui;
        self.desktop.wait_visible(&option, limit).await?;
        self.desktop.press(Key::Enter).await
    }
}

fn input_of(field: TestId) -> By {
    By::Css(format!("{} input", field.css()))
}

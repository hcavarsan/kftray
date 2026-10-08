use std::process::Stdio;
use std::time::Duration;

use anyhow::{
    Context,
    Result,
    bail,
    ensure,
};
use serde_json::{
    Value,
    json,
};
use thirtyfour::error::{
    WebDriverErrorInner,
    WebDriverResult,
};
use thirtyfour::{
    By,
    Capabilities,
    Key,
    WebDriver,
    WebElement,
};
use tokio::process::{
    Child,
    Command,
};
use tokio::time::timeout;

use crate::app::Profile;
use crate::harness::wait::{
    wait_for,
    wait_for_port,
};
use crate::harness::{
    TestEnv,
    Timeouts,
    free_port,
};
use crate::ui::{
    AddConfigPage,
    ConsentPage,
    MainPage,
    SettingsPage,
    TestId,
};

pub struct Desktop {
    child: Child,
    pub(crate) driver: WebDriver,
    profile: Profile,
    env: TestEnv,
}

impl Desktop {
    pub async fn launch(env: &TestEnv, profile: Profile) -> Result<Self> {
        let port = free_port()?;
        let mut child = Command::new(&env.kftray)
            .envs(profile.env(env, Some(port)))
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("spawn {}", env.kftray.display()))?;
        tokio::select! {
            ready = wait_for_port(port, env.timeouts.startup) => ready.context(
                "kftray did not open its WebDriver port; was it built with `--features e2e`?",
            )?,
            status = child.wait() => bail!(
                "kftray exited with {} before opening its WebDriver port; another kftray \
                 is probably running and took over, stop it with `pkill -x kftray`",
                status.context("wait for kftray")?
            ),
        }
        let driver = WebDriver::new(format!("http://127.0.0.1:{port}"), Capabilities::new())
            .await
            .context("open a WebDriver session on kftray")?;
        let desktop = Self {
            child,
            driver,
            profile,
            env: env.clone(),
        };
        desktop.wait_window_ready().await?;
        Ok(desktop)
    }

    pub async fn launch_ready(env: &TestEnv) -> Result<Self> {
        let desktop = Self::launch(env, Profile::new()?).await?;
        desktop.consent().decline().await?;
        Ok(desktop)
    }

    pub async fn relaunch(self) -> Result<Self> {
        let env = self.env.clone();
        let profile = self.profile.clone();
        self.quit().await?;
        Self::launch(&env, profile).await
    }

    pub async fn quit(mut self) -> Result<()> {
        self.invoke::<Value>("stop_all_port_forward_cmd", json!({}))
            .await?;
        let dispatch = self.invoke_detached("handle_exit_app", json!({})).await;
        let limit = self.env.timeouts.exit;
        let Ok(status) = timeout(limit, self.child.wait()).await else {
            dispatch?;
            bail!("kftray did not exit within {limit:?}");
        };
        let status = status.context("wait for kftray")?;
        ensure!(status.success(), "kftray exited with {status}");
        Ok(())
    }

    pub fn timeouts(&self) -> Timeouts {
        self.env.timeouts
    }

    pub fn main(&self) -> MainPage<'_> {
        MainPage::new(self)
    }

    pub fn consent(&self) -> ConsentPage<'_> {
        ConsentPage::new(self)
    }

    pub fn settings(&self) -> SettingsPage<'_> {
        SettingsPage::new(self)
    }

    pub fn add_config(&self) -> AddConfigPage<'_> {
        AddConfigPage::new(self)
    }

    async fn wait_window_ready(&self) -> Result<()> {
        let limit = self.env.timeouts.startup;
        self.wait_attr(&TestId::MainView.by(), "data-state", "ready", limit)
            .await
    }

    pub(crate) async fn wait_visible(&self, by: &By, limit: Duration) -> Result<()> {
        let what = format!("{} to be visible", describe(by));
        wait_for(&self.driver, &self.env.artifacts, &what, limit, || async {
            Ok(self.first_visible(by).await?.is_some())
        })
        .await
    }

    pub(crate) async fn wait_absent(&self, by: &By, limit: Duration) -> Result<()> {
        let what = format!("{} to disappear", describe(by));
        wait_for(&self.driver, &self.env.artifacts, &what, limit, || async {
            Ok(self.first_visible(by).await?.is_none())
        })
        .await
    }

    pub(crate) async fn wait_present(&self, by: &By, limit: Duration) -> Result<()> {
        let what = format!("{} to exist", describe(by));
        wait_for(&self.driver, &self.env.artifacts, &what, limit, || async {
            Ok(!self.driver.find_all(by.clone()).await?.is_empty())
        })
        .await
    }

    pub(crate) async fn wait_count(&self, by: &By, count: usize, limit: Duration) -> Result<()> {
        let what = format!("{count} of {}", describe(by));
        wait_for(&self.driver, &self.env.artifacts, &what, limit, || async {
            Ok(self.driver.find_all(by.clone()).await?.len() == count)
        })
        .await
    }

    pub(crate) async fn wait_attr(
        &self, by: &By, name: &str, value: &str, limit: Duration,
    ) -> Result<()> {
        let what = format!("{} to have {name}={value}", describe(by));
        wait_for(&self.driver, &self.env.artifacts, &what, limit, || async {
            Ok(self.attr(by, name).await?.as_deref() == Some(value))
        })
        .await
    }

    pub(crate) async fn is_visible(&self, by: &By) -> Result<bool> {
        Ok(self.first_visible(by).await?.is_some())
    }

    pub(crate) async fn attr(&self, by: &By, name: &str) -> Result<Option<String>> {
        for element in self.driver.find_all(by.clone()).await? {
            if let Some(value) = settle(element.attr(name).await)? {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }

    pub(crate) async fn find(&self, by: &By) -> Result<WebElement> {
        self.wait_visible(by, self.env.timeouts.ui).await?;
        self.first_visible(by)
            .await?
            .with_context(|| format!("{} vanished after it became visible", describe(by)))
    }

    pub(crate) async fn attrs(&self, by: &By, name: &str) -> Result<Vec<String>> {
        let mut values = Vec::new();
        for element in self.driver.find_all(by.clone()).await? {
            if let Some(value) = settle(element.attr(name).await)? {
                values.push(value);
            }
        }
        Ok(values)
    }

    pub(crate) async fn click(&self, by: &By) -> Result<()> {
        let what = format!("{} to accept a click", describe(by));
        wait_for(
            &self.driver,
            &self.env.artifacts,
            &what,
            self.env.timeouts.ui,
            || async {
                let Some(element) = self.first_visible(by).await? else {
                    return Ok(false);
                };
                settle(element.click().await.map(|()| true))
            },
        )
        .await
    }

    pub(crate) async fn press(&self, key: Key) -> Result<()> {
        let key = char::from(key);
        self.driver
            .action_chain()
            .key_down(key)
            .key_up(key)
            .perform()
            .await
            .with_context(|| format!("press {key:?}"))
    }

    async fn first_visible(&self, by: &By) -> Result<Option<WebElement>> {
        for element in self.driver.find_all(by.clone()).await? {
            if settle(element.is_displayed().await)? {
                return Ok(Some(element));
            }
        }
        Ok(None)
    }
}

impl Drop for Desktop {
    fn drop(&mut self) {
        let _ = self.driver.clone().leak();
        if let Some(log) = self.profile.log_file() {
            let _ = std::fs::copy(log, self.env.artifacts.join("kftray.log"));
        }
    }
}

fn settle<T: Default>(result: WebDriverResult<T>) -> Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(error)
            if matches!(
                error.as_inner(),
                WebDriverErrorInner::StaleElementReference(_)
                    | WebDriverErrorInner::NoSuchElement(_)
                    | WebDriverErrorInner::ElementClickIntercepted(_)
                    | WebDriverErrorInner::ElementNotInteractable(_)
            ) =>
        {
            Ok(T::default())
        }
        Err(error) => Err(error.into()),
    }
}

fn describe(by: &By) -> String {
    by.to_string()
}

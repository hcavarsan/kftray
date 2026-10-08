use anyhow::Result;
use kftray_e2e::app::{
    Desktop,
    Profile,
};
use kftray_e2e::harness::TestEnv;

#[tokio::test]
async fn shows_the_consent_dialog_on_first_start() -> Result<()> {
    let env = TestEnv::from_env()?;
    let app = Desktop::launch(&env, Profile::new()?).await?;

    app.consent().wait_visible().await?;

    app.consent().decline().await?;
    app.quit().await
}

#[tokio::test]
async fn accepting_turns_crash_reports_on_in_settings() -> Result<()> {
    let env = TestEnv::from_env()?;
    let app = Desktop::launch(&env, Profile::new()?).await?;
    app.consent().allow().await?;

    let settings = app.settings().open().await?;

    assert!(settings.crash_reports_enabled().await?);
    app.quit().await
}

#[tokio::test]
async fn declining_is_remembered_after_restart() -> Result<()> {
    let env = TestEnv::from_env()?;
    let app = Desktop::launch(&env, Profile::new()?).await?;
    app.consent().decline().await?;

    let app = app.relaunch().await?;

    assert!(!app.consent().is_visible().await?);
    app.quit().await
}

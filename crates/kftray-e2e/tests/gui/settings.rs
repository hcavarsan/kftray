use anyhow::Result;
use kftray_e2e::app::Desktop;
use kftray_e2e::harness::TestEnv;

#[tokio::test]
async fn toggling_crash_reports_persists_after_restart() -> Result<()> {
    let env = TestEnv::from_env()?;
    let app = Desktop::launch_ready(&env).await?;
    let settings = app.settings().open().await?;

    settings.toggle_crash_reports().await?;
    settings.save().await?;
    let app = app.relaunch().await?;

    assert!(app.settings().open().await?.crash_reports_enabled().await?);
    app.quit().await
}

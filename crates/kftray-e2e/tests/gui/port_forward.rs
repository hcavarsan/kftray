use anyhow::Result;
use kftray_e2e::app::Desktop;
use kftray_e2e::fixtures::{
    self,
    Fixture,
};
use kftray_e2e::harness::TestEnv;
use kftray_e2e::probe;
use rstest::rstest;

#[rstest]
#[case::service_http(fixtures::service_http())]
#[case::pod_http(fixtures::pod_http())]
#[case::service_udp(fixtures::service_udp())]
#[case::proxy_http(fixtures::proxy_http())]
#[tokio::test]
async fn starting_a_forward_serves_traffic(#[case] fixture: Fixture) -> Result<()> {
    let env = TestEnv::from_env()?;
    let app = Desktop::launch_ready(&env).await?;
    app.import_configs(&[&fixture.config]).await?;
    let row = app.main().row(fixture.alias());

    row.start().await?;
    row.wait_running().await?;

    fixture
        .expect
        .verify(fixture.local_port(), env.timeouts.forward)
        .await?;
    app.quit().await
}

#[rstest]
#[case::service_http(fixtures::service_http())]
#[case::pod_http(fixtures::pod_http())]
#[case::proxy_http(fixtures::proxy_http())]
#[tokio::test]
async fn stopping_a_forward_closes_the_port(#[case] fixture: Fixture) -> Result<()> {
    let env = TestEnv::from_env()?;
    let app = Desktop::launch_ready(&env).await?;
    app.import_configs(&[&fixture.config]).await?;
    let row = app.main().row(fixture.alias());
    row.start().await?;
    row.wait_running().await?;

    row.stop().await?;
    row.wait_stopped().await?;

    probe::wait_closed(fixture.local_port(), env.timeouts.ui).await?;
    app.quit().await
}

#[rstest]
#[case::service_http(fixtures::service_http())]
#[tokio::test]
async fn a_running_forward_resumes_after_restart(#[case] fixture: Fixture) -> Result<()> {
    let env = TestEnv::from_env()?;
    let app = Desktop::launch_ready(&env).await?;
    app.import_configs(&[&fixture.config]).await?;
    let row = app.main().row(fixture.alias());
    row.start().await?;
    row.wait_running().await?;

    let app = app.relaunch_after_kill().await?;

    fixture
        .expect
        .verify(fixture.local_port(), env.timeouts.forward)
        .await?;
    app.quit().await
}

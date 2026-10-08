use anyhow::Result;
use kftray_e2e::app::Kftui;
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
#[tokio::test]
async fn auto_start_serves_traffic(#[case] fixture: Fixture) -> Result<()> {
    let env = TestEnv::from_env()?;
    let kftui = Kftui::launch(&env, &[&fixture.config]).await?;

    fixture
        .expect
        .verify(fixture.local_port(), env.timeouts.forward)
        .await?;

    kftui.terminate(env.timeouts.exit).await?;
    Ok(())
}

#[rstest]
#[case::service_http(fixtures::service_http())]
#[tokio::test]
async fn terminating_closes_the_port(#[case] fixture: Fixture) -> Result<()> {
    let env = TestEnv::from_env()?;
    let kftui = Kftui::launch(&env, &[&fixture.config]).await?;
    fixture
        .expect
        .verify(fixture.local_port(), env.timeouts.forward)
        .await?;

    kftui.terminate(env.timeouts.exit).await?;

    probe::wait_closed(fixture.local_port(), env.timeouts.ui).await
}

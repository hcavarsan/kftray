use anyhow::Result;
use kftray_commons::models::config_model::Config;
use kftray_e2e::app::Desktop;
use kftray_e2e::fixtures;
use kftray_e2e::harness::TestEnv;
use kftray_e2e::probe;
use serde_json::json;

#[tokio::test]
async fn importing_a_file_lists_every_config() -> Result<()> {
    let env = TestEnv::from_env()?;
    let app = Desktop::launch_ready(&env).await?;
    let service = fixtures::service_http();
    let pod = fixtures::pod_http();
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("configs.json");
    std::fs::write(&file, serde_json::to_vec(&[&service.config, &pod.config])?)?;
    app.mock("plugin:dialog|open", json!(file)).await?;

    app.main().import_file().await?;
    app.main().wait_rows(2).await?;

    assert_eq!(app.main().aliases().await?, [pod.alias(), service.alias()]);
    app.quit().await
}

#[tokio::test]
async fn exporting_matches_what_was_imported() -> Result<()> {
    let env = TestEnv::from_env()?;
    let app = Desktop::launch_ready(&env).await?;
    let service = fixtures::service_http();
    let pod = fixtures::pod_http();
    app.import_configs(&[&service.config, &pod.config]).await?;
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("exported.json");
    app.mock("plugin:dialog|save", json!(file)).await?;

    app.main().export_file().await?;
    let exported: Vec<Config> =
        serde_json::from_str(&probe::read_written_file(&file, env.timeouts.ui).await?)?;

    assert_eq!(
        exported,
        [
            service.config.prepare_for_export(),
            pod.config.prepare_for_export()
        ]
    );
    app.quit().await
}

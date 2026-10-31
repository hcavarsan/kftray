use anyhow::Result;
use kftray_e2e::app::Desktop;
use kftray_e2e::harness::gcp_bastion::{
    ECHO_HOST,
    INSTANCE,
    PROJECT,
    ZONE,
};
use kftray_e2e::harness::workload::{
    NAMESPACE,
    Workload,
};
use kftray_e2e::harness::{
    TestEnv,
    free_port,
};

#[tokio::test]
async fn saving_the_form_adds_a_row() -> Result<()> {
    let env = TestEnv::from_env()?;
    let app = Desktop::launch_ready(&env).await?;
    let form = app.add_config().open().await?;

    form.alias("form-http").await?;
    form.context("default").await?;
    form.workload_type("Service").await?;
    form.namespace(NAMESPACE).await?;
    form.service(Workload::EchoHttp.name()).await?;
    form.protocol("TCP").await?;
    form.remote_port(Workload::EchoHttp.port()).await?;
    form.local_port(free_port()?).await?;
    form.save().await?;

    app.main().row("form-http").wait_present().await?;
    app.quit().await
}

#[tokio::test]
async fn saving_an_ssh_forward_form_adds_a_row() -> Result<()> {
    let env = TestEnv::from_env()?;
    let app = Desktop::launch_ready(&env).await?;
    let form = app.add_config().open().await?;

    form.alias("form-ssh").await?;
    form.workload_type("SSH port forward").await?;
    form.ssh_provider("GCP Bastion (IAP)").await?;
    form.gcp_project(PROJECT).await?;
    form.gcp_zone(ZONE).await?;
    form.gcp_instance(INSTANCE).await?;
    form.ssh_remote_address(ECHO_HOST).await?;
    form.ssh_local_port(free_port()?).await?;
    form.ssh_remote_port(Workload::EchoHttp.port()).await?;
    form.save().await?;

    app.main().row("form-ssh").wait_present().await?;
    app.quit().await
}

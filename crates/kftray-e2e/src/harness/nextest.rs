use std::process::ExitStatus;

use anyhow::{
    Context,
    Result,
};
use tokio::process::Command;

use crate::harness::env::TestEnv;
use crate::harness::workspace_root;

pub const CONFIG_FILE: &str = ".cargo/nextest.toml";

pub async fn run(env: &TestEnv, display: Option<&str>, filter: Option<&str>) -> Result<ExitStatus> {
    let mut command = Command::new("cargo");
    command
        .current_dir(workspace_root())
        .args(["nextest", "run", "--config-file", CONFIG_FILE])
        .args(["--profile", "e2e", "--package", "kftray-e2e"])
        .envs(env.vars())
        .kill_on_drop(true);
    if let Some(display) = display {
        command.env("DISPLAY", display);
    }
    if let Some(filter) = filter {
        command.args(["-E", filter]);
    }
    command
        .status()
        .await
        .context("run cargo nextest; install it with `cargo install cargo-nextest`")
}

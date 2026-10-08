use std::env::consts::EXE_SUFFIX;
use std::path::PathBuf;

use anyhow::{
    Context,
    Result,
    ensure,
};
use tokio::process::Command;

use crate::harness::coverage::Coverage;
use crate::harness::workspace_root;

const PNPM: &str = if cfg!(windows) { "pnpm.cmd" } else { "pnpm" };

#[derive(Clone, Debug)]
pub struct Binaries {
    pub kftray: PathBuf,
    pub kftui: PathBuf,
}

impl Binaries {
    pub fn in_target() -> Self {
        let debug = workspace_root().join("target").join("debug");
        Self {
            kftray: debug.join(format!("kftray{EXE_SUFFIX}")),
            kftui: debug.join(format!("kftui{EXE_SUFFIX}")),
        }
    }

    pub fn existing() -> Result<Self> {
        let binaries = Self::in_target();
        for path in [&binaries.kftray, &binaries.kftui] {
            ensure!(
                path.is_file(),
                "{} is missing; run `cargo run -p kftray-e2e -- build` first",
                path.display()
            );
        }
        Ok(binaries)
    }
}

pub async fn all(coverage: Option<&Coverage>) -> Result<Binaries> {
    let root = workspace_root();
    let mut app = Command::new(PNPM);
    app.current_dir(&root).args([
        "exec",
        "tauri",
        "build",
        "--debug",
        "--no-bundle",
        "--features",
        "e2e",
        "--config",
        "crates/kftray-tauri/tauri.e2e.conf.json",
    ]);
    let mut kftui = Command::new("cargo");
    kftui.current_dir(&root).args(["build", "-p", "kftui"]);
    if let Some(coverage) = coverage {
        app.envs(coverage.vars());
        kftui.envs(coverage.vars());
    }
    run(&mut app)
        .await
        .context("build kftray with the e2e feature")?;
    run(&mut kftui).await.context("build kftui")?;
    Ok(Binaries::in_target())
}

async fn run(command: &mut Command) -> Result<()> {
    let status = command
        .kill_on_drop(true)
        .status()
        .await
        .with_context(|| format!("spawn {command:?}"))?;
    ensure!(status.success(), "{command:?} exited with {status}");
    Ok(())
}

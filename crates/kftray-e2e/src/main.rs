use anyhow::{
    Context,
    Result,
    bail,
    ensure,
};
use clap::Parser;
use kftray_e2e::harness::build::{
    self,
    Binaries,
};
use kftray_e2e::harness::cluster::Cluster;
use kftray_e2e::harness::display::Display;
use kftray_e2e::harness::workload::Workload;
use kftray_e2e::harness::{
    TestEnv,
    Timeouts,
    nextest,
    workspace_root,
};

#[derive(Parser)]
#[command(
    name = "kftray-e2e",
    about = "Build, provision and run the kftray end-to-end suite"
)]
enum Command {
    Run {
        #[arg(
            long,
            help = "nextest filter expression, for example 'test(first_run)'"
        )]
        filter: Option<String>,
        #[arg(long, help = "reuse target/debug/kftray and target/debug/kftui")]
        skip_build: bool,
    },
    Up {
        #[arg(long, help = "reuse target/debug/kftray and target/debug/kftui")]
        skip_build: bool,
    },
    Build,
}

struct Environment {
    env: TestEnv,
    display: Display,
    _cluster: Cluster,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("kftray_e2e=info"))
        .init();
    match Command::parse() {
        Command::Build => build::all().await.map(drop),
        Command::Run { filter, skip_build } => run(filter.as_deref(), skip_build).await,
        Command::Up { skip_build } => up(skip_build).await,
    }
}

async fn run(filter: Option<&str>, skip_build: bool) -> Result<()> {
    let environment = prepare(skip_build).await?;
    let status = tokio::select! {
        status = nextest::run(&environment.env, environment.display.name.as_deref(), filter) => status?,
        _ = tokio::signal::ctrl_c() => bail!("interrupted"),
    };
    ensure!(status.success(), "e2e tests failed");
    Ok(())
}

async fn up(skip_build: bool) -> Result<()> {
    let environment = prepare(skip_build).await?;
    for (name, value) in environment.env.vars() {
        println!("export {name}={}", value.display());
    }
    if let Some(display) = &environment.display.name {
        println!("export DISPLAY={display}");
    }
    println!(
        "run tests with: cargo nextest run --config-file {} --profile e2e -E 'test(name)'",
        nextest::CONFIG_FILE
    );
    println!("environment is up; press Ctrl-C to tear it down");
    tokio::signal::ctrl_c().await.context("wait for Ctrl-C")?;
    Ok(())
}

async fn prepare(skip_build: bool) -> Result<Environment> {
    let out = workspace_root().join("target").join("e2e");
    let artifacts = out.join("artifacts");
    if artifacts.exists() {
        std::fs::remove_dir_all(&artifacts)
            .with_context(|| format!("clear {}", artifacts.display()))?;
    }
    let timeouts = Timeouts::scaled_from_env()?;
    let display = Display::ensure(timeouts.startup).await?;
    let (binaries, cluster) = tokio::try_join!(
        build_or_reuse(skip_build),
        Cluster::start(Workload::all(), &out),
    )?;
    let env = TestEnv::new(binaries, &cluster, artifacts)?;
    Ok(Environment {
        env,
        display,
        _cluster: cluster,
    })
}

async fn build_or_reuse(skip_build: bool) -> Result<Binaries> {
    if skip_build {
        Binaries::existing()
    } else {
        build::all().await
    }
}

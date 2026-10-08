use std::ffi::OsStr;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{
    Context,
    Result,
};

use crate::harness::build::Binaries;
use crate::harness::cluster::Cluster;

pub const POLL_INTERVAL: Duration = Duration::from_millis(200);
pub const CLUSTER_READY: Duration = Duration::from_secs(180);

const APP: &str = "KFTRAY_E2E_APP";
const KFTUI: &str = "KFTRAY_E2E_KFTUI";
const KUBECONFIG: &str = "KUBECONFIG";
const ARTIFACTS: &str = "KFTRAY_E2E_ARTIFACTS";
const TIMEOUT_SCALE: &str = "KFTRAY_E2E_TIMEOUT_SCALE";

#[derive(Clone, Debug)]
pub struct TestEnv {
    pub kftray: PathBuf,
    pub kftui: PathBuf,
    pub kubeconfig: PathBuf,
    pub artifacts: PathBuf,
    pub timeouts: Timeouts,
}

#[derive(Clone, Copy, Debug)]
pub struct Timeouts {
    pub startup: Duration,
    pub ui: Duration,
    pub forward: Duration,
    pub exit: Duration,
}

impl Timeouts {
    pub fn scaled_from_env() -> Result<Self> {
        let scale = match std::env::var(TIMEOUT_SCALE) {
            Ok(value) => value
                .parse::<f64>()
                .ok()
                .filter(|scale| scale.is_finite() && *scale > 0.0)
                .with_context(|| format!("{TIMEOUT_SCALE}={value:?} is not a positive number"))?,
            Err(_) => 1.0,
        };
        Ok(Self {
            startup: Duration::from_secs(30).mul_f64(scale),
            ui: Duration::from_secs(10).mul_f64(scale),
            forward: Duration::from_secs(60).mul_f64(scale),
            exit: Duration::from_secs(10).mul_f64(scale),
        })
    }
}

impl TestEnv {
    pub fn new(binaries: Binaries, cluster: &Cluster, artifacts: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&artifacts)
            .with_context(|| format!("create {}", artifacts.display()))?;
        Ok(Self {
            kftray: binaries.kftray,
            kftui: binaries.kftui,
            kubeconfig: cluster.kubeconfig.clone(),
            artifacts,
            timeouts: Timeouts::scaled_from_env()?,
        })
    }

    pub fn from_env() -> Result<Self> {
        let var = |name: &str| {
            std::env::var_os(name).map(PathBuf::from).with_context(|| {
                format!("{name} is not set; run `cargo run -p kftray-e2e -- run` or `-- up`")
            })
        };
        Ok(Self {
            kftray: var(APP)?,
            kftui: var(KFTUI)?,
            kubeconfig: var(KUBECONFIG)?,
            artifacts: var(ARTIFACTS)?,
            timeouts: Timeouts::scaled_from_env()?,
        })
    }

    pub fn vars(&self) -> Vec<(&'static str, &OsStr)> {
        vec![
            (APP, self.kftray.as_os_str()),
            (KFTUI, self.kftui.as_os_str()),
            (KUBECONFIG, self.kubeconfig.as_os_str()),
            (ARTIFACTS, self.artifacts.as_os_str()),
        ]
    }
}

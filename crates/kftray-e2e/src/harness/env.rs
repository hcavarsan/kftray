use std::ffi::OsStr;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{
    Context,
    Result,
    anyhow,
};

use crate::harness::build::Binaries;
use crate::harness::cluster::Cluster;
use crate::harness::gcp_bastion::GcpBastion;

pub const POLL_INTERVAL: Duration = Duration::from_millis(200);
pub const CLUSTER_READY: Duration = Duration::from_secs(180);

const APP: &str = "KFTRAY_E2E_APP";
const KFTUI: &str = "KFTRAY_E2E_KFTUI";
const KUBECONFIG: &str = "KUBECONFIG";
const ARTIFACTS: &str = "KFTRAY_E2E_ARTIFACTS";
const GCP_ENDPOINTS: &str = "KFTRAY_E2E_GCP_ENDPOINTS";
const SSH_KEY: &str = "KFTRAY_E2E_SSH_KEY";
const TIMEOUT_SCALE: &str = "KFTRAY_E2E_TIMEOUT_SCALE";

#[derive(Clone, Debug)]
pub struct TestEnv {
    pub kftray: PathBuf,
    pub kftui: PathBuf,
    pub kubeconfig: PathBuf,
    pub artifacts: PathBuf,
    pub gcp_endpoints: String,
    pub ssh_key: PathBuf,
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
    pub fn new(
        binaries: Binaries, cluster: &Cluster, bastion: &GcpBastion, artifacts: PathBuf,
    ) -> Result<Self> {
        std::fs::create_dir_all(&artifacts)
            .with_context(|| format!("create {}", artifacts.display()))?;
        Ok(Self {
            kftray: binaries.kftray,
            kftui: binaries.kftui,
            kubeconfig: cluster.kubeconfig.clone(),
            artifacts,
            gcp_endpoints: bastion.endpoints_json()?,
            ssh_key: bastion.ssh_key().to_path_buf(),
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
            gcp_endpoints: var(GCP_ENDPOINTS)?
                .into_os_string()
                .into_string()
                .map_err(|_| anyhow!("{GCP_ENDPOINTS} is not valid UTF-8"))?,
            ssh_key: var(SSH_KEY)?,
            timeouts: Timeouts::scaled_from_env()?,
        })
    }

    pub fn vars(&self) -> Vec<(&'static str, &OsStr)> {
        vec![
            (APP, self.kftray.as_os_str()),
            (KFTUI, self.kftui.as_os_str()),
            (KUBECONFIG, self.kubeconfig.as_os_str()),
            (ARTIFACTS, self.artifacts.as_os_str()),
            (GCP_ENDPOINTS, OsStr::new(&self.gcp_endpoints)),
            (SSH_KEY, self.ssh_key.as_os_str()),
        ]
    }
}

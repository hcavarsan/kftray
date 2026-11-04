use std::ffi::OsString;
use std::path::{
    Path,
    PathBuf,
};
use std::sync::Arc;

use anyhow::{
    Context,
    Result,
};
use kftray_ssh::consts::KEY_RELATIVE_PATH;
use tempfile::TempDir;

use crate::harness::TestEnv;
use crate::harness::gcp_bastion::TOKEN;

#[derive(Clone)]
pub struct Profile {
    dir: Arc<TempDir>,
}

impl Profile {
    pub fn new() -> Result<Self> {
        let dir = tempfile::Builder::new()
            .prefix("kftray-e2e-")
            .tempdir()
            .context("create a temporary KFTRAY_CONFIG")?;
        Ok(Self { dir: Arc::new(dir) })
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn env(&self, env: &TestEnv, webdriver_port: Option<u16>) -> Vec<(&'static str, OsString)> {
        let mut vars = vec![
            ("KFTRAY_CONFIG", self.path().into()),
            ("KUBECONFIG", env.kubeconfig.as_os_str().into()),
            ("DO_NOT_TRACK", "1".into()),
            ("KFTRAY_TEST_MODE", "1".into()),
            ("WEBKIT_DISABLE_COMPOSITING_MODE", "1".into()),
            ("WEBKIT_DISABLE_DMABUF_RENDERER", "1".into()),
            ("RUST_LOG", "info".into()),
            ("KFTRAY_GCP_ENDPOINTS", env.gcp_endpoints.as_str().into()),
            ("KFTRAY_GCP_STATIC_TOKEN", TOKEN.into()),
        ];
        if let Some(port) = webdriver_port {
            vars.push(("TAURI_WEBDRIVER_PORT", port.to_string().into()));
        }
        vars
    }

    pub fn seed_ssh_key(&self, key: &Path) -> Result<()> {
        let target = self.path().join(KEY_RELATIVE_PATH);
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        }
        std::fs::copy(key, &target)
            .with_context(|| format!("copy {} to {}", key.display(), target.display()))?;
        #[cfg(unix)]
        std::fs::set_permissions(&target, std::os::unix::fs::PermissionsExt::from_mode(0o600))
            .with_context(|| format!("restrict {}", target.display()))?;
        Ok(())
    }

    pub fn log_file(&self) -> Option<PathBuf> {
        std::fs::read_dir(self.path())
            .ok()?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("kftray_") && name.ends_with(".log"))
            })
            .max()
    }
}

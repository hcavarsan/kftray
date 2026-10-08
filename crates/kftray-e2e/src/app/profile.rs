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
use tempfile::TempDir;

use crate::harness::TestEnv;

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
        ];
        if let Some(port) = webdriver_port {
            vars.push(("TAURI_WEBDRIVER_PORT", port.to_string().into()));
        }
        vars
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

use std::process::{
    ExitStatus,
    Stdio,
};
use std::time::Duration;

use anyhow::{
    Context,
    Result,
};
use kftray_commons::models::config_model::Config;
use tempfile::NamedTempFile;
use tokio::process::{
    Child,
    Command,
};
use tokio::time::timeout;

use crate::app::Profile;
use crate::harness::TestEnv;

pub struct Kftui {
    child: Child,
    _profile: Profile,
    _configs: NamedTempFile,
}

impl Kftui {
    pub async fn launch(env: &TestEnv, configs: &[&Config]) -> Result<Self> {
        let profile = Profile::new()?;
        let mut file = tempfile::Builder::new()
            .prefix("kftui-configs-")
            .suffix(".json")
            .tempfile()
            .context("create the kftui config file")?;
        serde_json::to_writer(&mut file, configs).context("write the kftui config file")?;
        let child = Command::new(&env.kftui)
            .arg("--configs-path")
            .arg(file.path())
            .args(["--auto-start", "--non-interactive", "--no-update-check"])
            .envs(profile.env(env, None))
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("spawn {}", env.kftui.display()))?;
        Ok(Self {
            child,
            _profile: profile,
            _configs: file,
        })
    }

    pub async fn terminate(mut self, limit: Duration) -> Result<ExitStatus> {
        self.signal_terminate()?;
        timeout(limit, self.child.wait())
            .await
            .with_context(|| format!("kftui did not exit within {limit:?}"))?
            .context("wait for kftui")
    }

    #[cfg(unix)]
    fn signal_terminate(&mut self) -> Result<()> {
        let pid = self.child.id().context("kftui already exited")?;
        let pid = libc::pid_t::try_from(pid).context("kftui pid out of range")?;
        if unsafe { libc::kill(pid, libc::SIGTERM) } != 0 {
            return Err(std::io::Error::last_os_error()).context("send SIGTERM to kftui");
        }
        Ok(())
    }

    #[cfg(not(unix))]
    fn signal_terminate(&mut self) -> Result<()> {
        self.child.start_kill().context("stop kftui")
    }
}

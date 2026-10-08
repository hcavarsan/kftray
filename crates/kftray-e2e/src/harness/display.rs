use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{
    Context,
    Result,
};
use tokio::process::{
    Child,
    Command,
};

use crate::harness::wait::poll;

pub struct Display {
    _xvfb: Option<Child>,
    pub name: Option<String>,
}

impl Display {
    pub async fn ensure(limit: Duration) -> Result<Self> {
        if !cfg!(target_os = "linux") || std::env::var_os("DISPLAY").is_some() {
            return Ok(Self {
                _xvfb: None,
                name: None,
            });
        }
        let number = free_display_number()?;
        let name = format!(":{number}");
        let child = Command::new("Xvfb")
            .args([
                name.as_str(),
                "-screen",
                "0",
                "1280x800x24",
                "-nolisten",
                "tcp",
            ])
            .stdout(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("start Xvfb; install the xvfb package")?;
        let socket = format!("/tmp/.X11-unix/X{number}");
        poll(limit, || async {
            Path::new(&socket).exists().then_some(())
        })
        .await
        .with_context(|| format!("Xvfb did not open display {name} within {limit:?}"))?;
        log::info!("started Xvfb on {name}");
        Ok(Self {
            _xvfb: Some(child),
            name: Some(name),
        })
    }
}

fn free_display_number() -> Result<u16> {
    (99..199)
        .find(|number| {
            !Path::new(&format!("/tmp/.X{number}-lock")).exists()
                && !Path::new(&format!("/tmp/.X11-unix/X{number}")).exists()
        })
        .context("no free X display number between :99 and :198")
}

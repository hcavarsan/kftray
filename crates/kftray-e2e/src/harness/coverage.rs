use std::path::{
    Path,
    PathBuf,
};

use anyhow::{
    Context,
    Result,
    ensure,
};
use tokio::process::Command;

use crate::harness::workspace_root;

const TARGET_DIR_VAR: &str = "CARGO_LLVM_COV_TARGET_DIR";
const IGNORED_FILES: &str = r"crates[/\\]kftray-e2e[/\\]";
const CACHEDIR_TAG: &str = "Signature: 8a477f597d28d172789f06886806bc55\n\
                            # This file is a cache directory tag created by cargo.\n\
                            # For information about cache directory tags see https://bford.info/cachedir/\n";

pub struct Coverage {
    vars: Vec<(String, String)>,
}

impl Coverage {
    pub async fn prepare() -> Result<Self> {
        let output = Command::new("cargo")
            .current_dir(workspace_root())
            .args(["llvm-cov", "show-env"])
            .kill_on_drop(true)
            .output()
            .await
            .context(
                "run cargo llvm-cov show-env; install it with `cargo install cargo-llvm-cov` and \
                 `rustup component add llvm-tools`",
            )?;
        ensure!(
            output.status.success(),
            "cargo llvm-cov show-env failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).context("decode cargo llvm-cov show-env")?;
        let coverage = Self {
            vars: parse_env(&stdout)?,
        };
        coverage.clean().await?;
        coverage.clear_profiles()?;
        Ok(coverage)
    }

    pub async fn clean(&self) -> Result<()> {
        self.tag_target_dir()?;
        let status = Command::new("cargo")
            .current_dir(workspace_root())
            .args(["llvm-cov", "clean", "--workspace"])
            .envs(self.vars())
            .kill_on_drop(true)
            .status()
            .await
            .context("run cargo llvm-cov clean --workspace")?;
        ensure!(
            status.success(),
            "cargo llvm-cov clean --workspace exited with {status}"
        );
        Ok(())
    }

    pub fn vars(&self) -> impl Iterator<Item = (&str, &str)> {
        self.vars
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }

    pub async fn report(&self, lcov: &Path) -> Result<()> {
        let status = Command::new("cargo")
            .current_dir(workspace_root())
            .args(["llvm-cov", "report", "--lcov", "--output-path"])
            .arg(lcov)
            .args(["--ignore-filename-regex", IGNORED_FILES])
            .envs(self.vars())
            .kill_on_drop(true)
            .status()
            .await
            .context("run cargo llvm-cov report")?;
        ensure!(
            status.success(),
            "cargo llvm-cov report exited with {status}"
        );
        log::info!("e2e coverage written to {}", lcov.display());
        Ok(())
    }

    fn target_dir(&self) -> Result<PathBuf> {
        self.vars
            .iter()
            .find(|(name, _)| name == TARGET_DIR_VAR)
            .map(|(_, value)| PathBuf::from(value))
            .with_context(|| format!("cargo llvm-cov show-env did not set {TARGET_DIR_VAR}"))
    }

    fn tag_target_dir(&self) -> Result<()> {
        let dir = self.target_dir()?;
        let tag = dir.join("CACHEDIR.TAG");
        if dir.is_dir() && !tag.exists() {
            std::fs::write(&tag, CACHEDIR_TAG)
                .with_context(|| format!("write {}", tag.display()))?;
        }
        Ok(())
    }

    fn clear_profiles(&self) -> Result<()> {
        let dir = self.target_dir()?;
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error).with_context(|| format!("read {}", dir.display())),
        };
        for entry in entries {
            let path = entry
                .with_context(|| format!("read {}", dir.display()))?
                .path();
            if path
                .extension()
                .is_some_and(|extension| extension == "profraw")
            {
                std::fs::remove_file(&path)
                    .with_context(|| format!("remove {}", path.display()))?;
            }
        }
        Ok(())
    }
}

fn parse_env(output: &str) -> Result<Vec<(String, String)>> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (name, value) = line
                .split_once('=')
                .with_context(|| format!("unexpected cargo llvm-cov show-env line {line:?}"))?;
            let value = value
                .strip_prefix('\'')
                .and_then(|quoted| quoted.strip_suffix('\''))
                .unwrap_or(value);
            ensure!(
                !value.contains('\''),
                "cannot read the quoted value of {name} from cargo llvm-cov show-env"
            );
            Ok((name.to_owned(), value.to_owned()))
        })
        .collect()
}

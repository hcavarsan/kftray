use std::env;

use anyhow::Result;
use log::info;
#[cfg(unix)]
use log::warn;
use tokio::sync::OnceCell;

use super::KubeConnection;
use super::config::{
    create_config_with_context,
    get_kubeconfig_paths_from_option,
    merge_kubeconfigs,
};
use super::connection::create_client_with_config;

static PATH_INIT: OnceCell<()> = OnceCell::const_new();

async fn init_path() {
    PATH_INIT
        .get_or_init(|| async {
            #[cfg(unix)]
            let resolved_path = {
                let current = env::var("PATH").unwrap_or_default();
                tokio::task::spawn_blocking({
                    let current = current.clone();
                    move || match shell_path() {
                        Some(p) => {
                            info!("init_path: using shell PATH");
                            merge_paths(&p, &current)
                        }
                        None => {
                            info!("init_path: using fallback paths");
                            with_fallback(&current)
                        }
                    }
                })
                .await
                .unwrap_or_else(|e| {
                    warn!("init_path: spawn_blocking failed ({e}), falling back to current PATH");
                    with_fallback(&current)
                })
            };

            // Windows GUI apps may not inherit PATH correctly from parent process.
            // Re-setting forces std::process::Command to use current values.
            #[cfg(windows)]
            let path_vars: Vec<(&'static str, String)> = ["PATH", "PATHEXT"]
                .into_iter()
                .filter_map(|var| env::var(var).ok().map(|val| (var, val)))
                .collect();

            unsafe {
                env::remove_var("PYTHONHOME");
                env::remove_var("PYTHONPATH");

                #[cfg(windows)]
                for (var, val) in &path_vars {
                    env::set_var(var, val);
                }

                #[cfg(unix)]
                env::set_var("PATH", &resolved_path);
            }
        })
        .await;
}

#[cfg(unix)]
fn shell_path() -> Option<String> {
    use std::path::Path;

    let home = env::var("HOME").ok()?;

    let current_shell = env::var("SHELL").ok();
    let shells_to_try = [
        current_shell.as_deref(),
        Some("/opt/homebrew/bin/fish"),
        Some("/usr/local/bin/fish"),
        Some("/bin/zsh"),
        Some("/bin/bash"),
    ];

    let mut collected = Vec::new();

    for (index, candidate) in shells_to_try.iter().enumerate() {
        let Some(shell) = *candidate else {
            continue;
        };
        if shells_to_try[..index].contains(candidate) || !Path::new(shell).exists() {
            continue;
        }
        if let Some(path) = try_shell_path(shell, &home) {
            info!("shell_path: {} returned {} chars", shell, path.len());
            collected.push(path);
        }
    }

    let merged = merge_shell_paths(&collected);
    if merged.is_none() {
        warn!("shell_path: no shell returned valid PATH");
    }
    merged
}

/// Merges PATH strings collected from every shell that answered, in the
/// order they were tried, deduping entries while preserving each shell's
/// first occurrence. Kept separate from `shell_path` so the merge logic is
/// testable without spawning real shells.
#[cfg(unix)]
fn merge_shell_paths(paths: &[String]) -> Option<String> {
    paths.iter().fold(None, |acc, path| {
        Some(match acc {
            Some(acc) => merge_paths(&acc, path),
            None => path.clone(),
        })
    })
}

#[cfg(unix)]
fn try_shell_path(shell: &str, home: &str) -> Option<String> {
    use std::io::Read;
    use std::process::{
        Command,
        Stdio,
    };
    use std::time::{
        Duration,
        Instant,
    };

    use log::warn;

    let is_fish = shell.ends_with("/fish");
    let cmd = if is_fish {
        "string join : $PATH"
    } else {
        "echo $PATH"
    };

    let mut child = Command::new(shell)
        .args(["-lc", cmd])
        .env("DISABLE_AUTO_UPDATE", "true")
        .current_dir(home)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;

    let timeout = Duration::from_secs(3);
    let start = Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = String::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = out.read_to_string(&mut stdout);
                }

                if !status.success() {
                    return None;
                }

                let path = stdout.trim().to_string();
                return (!path.is_empty() && path.contains('/')).then_some(path);
            }
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                warn!("try_shell_path: {} timed out", shell);
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => return None,
        }
    }
}

#[cfg(unix)]
fn merge_paths(shell: &str, current: &str) -> String {
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    shell
        .split(':')
        .chain(current.split(':'))
        .filter(|p| !p.is_empty() && seen.insert(*p))
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(unix)]
fn with_fallback(current: &str) -> String {
    use std::collections::HashSet;
    use std::path::Path;

    let existing: HashSet<_> = current.split(':').collect();
    let paths = fallback_paths();
    let new: Vec<_> = paths
        .iter()
        .filter(|p| !existing.contains(p.as_str()) && Path::new(p).exists())
        .map(String::as_str)
        .collect();

    if new.is_empty() {
        current.to_string()
    } else {
        format!("{}:{current}", new.join(":"))
    }
}

#[cfg(target_os = "macos")]
fn fallback_paths() -> Vec<String> {
    let h = env::var("HOME").unwrap_or_default();
    vec![
        "/usr/local/bin".into(),
        "/opt/homebrew/bin".into(),
        "/opt/homebrew/sbin".into(),
        format!("{h}/.local/bin"),
        "/usr/local/google-cloud-sdk/bin".into(),
        format!("{h}/google-cloud-sdk/bin"),
        "/opt/homebrew/Caskroom/google-cloud-sdk/latest/google-cloud-sdk/bin".into(),
        format!("{h}/.local/share/mise/shims"),
        format!("{h}/.asdf/shims"),
        format!("{h}/.asdf/bin"),
        format!("{h}/.nix-profile/bin"),
        "/nix/var/nix/profiles/default/bin".into(),
        format!("{h}/.cargo/bin"),
        format!("{h}/.volta/bin"),
        format!("{h}/.deno/bin"),
        format!("{h}/.bun/bin"),
        format!("{h}/go/bin"),
        format!("{h}/.krew/bin"),
    ]
}

#[cfg(all(unix, not(target_os = "macos")))]
fn fallback_paths() -> Vec<String> {
    let h = env::var("HOME").unwrap_or_default();
    vec![
        "/usr/local/bin".into(),
        "/snap/bin".into(),
        "/home/linuxbrew/.linuxbrew/bin".into(),
        format!("{h}/.linuxbrew/bin"),
        format!("{h}/.local/bin"),
        "/usr/local/google-cloud-sdk/bin".into(),
        format!("{h}/google-cloud-sdk/bin"),
        format!("{h}/.local/share/mise/shims"),
        format!("{h}/.asdf/shims"),
        format!("{h}/.asdf/bin"),
        format!("{h}/.nix-profile/bin"),
        "/nix/var/nix/profiles/default/bin".into(),
        format!("{h}/.cargo/bin"),
        format!("{h}/.volta/bin"),
        format!("{h}/.deno/bin"),
        format!("{h}/.bun/bin"),
        format!("{h}/go/bin"),
        format!("{h}/.krew/bin"),
    ]
}

fn env_debug_info() -> String {
    let path = env::var("PATH")
        .map(|p| {
            if p.len() > 80 {
                format!("{}...", &p[..80])
            } else {
                p
            }
        })
        .unwrap_or_else(|_| "<not set>".into());
    let home = env::var("HOME").unwrap_or_else(|_| "<not set>".into());
    let kubeconfig = env::var("KUBECONFIG").unwrap_or_else(|_| "<not set>".into());

    format!("PATH={path} | HOME={home} | KUBECONFIG={kubeconfig}")
}

pub async fn create_client_with_specific_context(
    kubeconfig: Option<String>, context_name: &str,
) -> Result<KubeConnection> {
    init_path().await;

    let kubeconfig_paths = get_kubeconfig_paths_from_option(kubeconfig)?;
    let (merged_kubeconfig, mut errors) = merge_kubeconfigs(&kubeconfig_paths)?;

    match create_config_with_context(&merged_kubeconfig, context_name).await {
        Ok(config) => match create_client_with_config(&config).await {
            Ok(client) => {
                info!("Created new client for context: {context_name}");
                return Ok(KubeConnection {
                    client,
                    cluster_url: config.cluster_url,
                });
            }
            Err(e) => {
                errors.push(format!(
                    "Connection failed for context '{context_name}': {e}"
                ));
            }
        },
        Err(e) => {
            errors.push(format!("Config error for context '{context_name}': {e}"));
        }
    }

    Err(anyhow::anyhow!(
        "Failed to create Kubernetes client.\n\
         Errors:\n{}\n\
         Environment: {}",
        errors
            .iter()
            .map(|e| format!("  • {e}"))
            .collect::<Vec<_>>()
            .join("\n"),
        env_debug_info()
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn merge_shell_paths_merges_and_dedupes_in_shell_order() {
        let paths = vec![
            "/opt/homebrew/bin:/usr/bin".to_string(),
            "/usr/local/bin:/usr/bin".to_string(),
            "/bin/zsh/extra:/opt/homebrew/bin".to_string(),
        ];

        let merged = merge_shell_paths(&paths).expect("at least one shell answered");

        assert_eq!(
            merged,
            "/opt/homebrew/bin:/usr/bin:/usr/local/bin:/bin/zsh/extra"
        );
    }

    #[test]
    fn merge_shell_paths_returns_none_when_no_shell_answered() {
        assert_eq!(merge_shell_paths(&[]), None);
    }

    #[test]
    fn merge_shell_paths_single_shell_is_unchanged() {
        let paths = vec!["/usr/local/bin:/usr/bin:/bin".to_string()];
        assert_eq!(
            merge_shell_paths(&paths),
            Some("/usr/local/bin:/usr/bin:/bin".to_string())
        );
    }

    #[test]
    fn merge_paths_prefers_shell_entries_and_appends_new_process_entries() {
        let merged = merge_paths("/opt/homebrew/bin:/usr/bin", "/usr/bin:/usr/local/sbin");
        assert_eq!(merged, "/opt/homebrew/bin:/usr/bin:/usr/local/sbin");
    }
}

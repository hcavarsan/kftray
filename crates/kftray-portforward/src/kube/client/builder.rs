use std::env;
use std::sync::Once;

use anyhow::Result;
use kube::Client;
use kube::config::Kubeconfig;
use log::info;

use super::config::{
    create_config_with_context,
    get_kubeconfig_paths_from_option,
    merge_kubeconfigs,
};
use super::connection::create_client_with_config;

static PATH_INIT: Once = Once::new();

fn init_path() {
    PATH_INIT.call_once(|| {
        unsafe {
            env::remove_var("PYTHONHOME");
            env::remove_var("PYTHONPATH");
        }

        // Windows GUI apps may not inherit PATH correctly from parent process.
        // Re-setting forces std::process::Command to use current values.
        #[cfg(windows)]
        for var in ["PATH", "PATHEXT"] {
            if let Ok(val) = env::var(var) {
                unsafe { env::set_var(var, &val) };
            }
        }

        #[cfg(unix)]
        {
            let current = env::var("PATH").unwrap_or_default();
            let resolved = shell_path()
                .map(|p| merge_paths(&p, &current))
                .unwrap_or_else(|| with_fallback(&current));
            unsafe { env::set_var("PATH", &resolved) };
        }
    });
}

#[cfg(unix)]
fn shell_path() -> Option<String> {
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

    let shell = env::var("SHELL").ok()?;
    let home = env::var("HOME").ok()?;
    let is_fish = shell.ends_with("/fish");

    let cmd = if is_fish {
        "string join : $PATH"
    } else {
        "echo $PATH"
    };

    let mut child = Command::new(&shell)
        .args(["-lc", cmd])
        .env("DISABLE_AUTO_UPDATE", "true")
        .current_dir(&home)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;

    let timeout = Duration::from_secs(5);
    let start = Instant::now();

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = String::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = out.read_to_string(&mut stdout);
                }

                if !status.success() {
                    let mut stderr = String::new();
                    if let Some(mut err) = child.stderr.take() {
                        let _ = err.read_to_string(&mut stderr);
                    }
                    warn!("shell_path failed: {}", stderr.trim());
                    return None;
                }

                let path = stdout.trim().to_string();
                return (!path.is_empty() && path.contains('/')).then_some(path);
            }
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                warn!("shell_path timed out");
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
    kubeconfig: Option<String>, context_name: Option<&str>,
) -> Result<(Option<Client>, Option<Kubeconfig>, Vec<String>)> {
    init_path();

    let kubeconfig_paths = get_kubeconfig_paths_from_option(kubeconfig)?;
    let (merged_kubeconfig, all_contexts, mut errors) = merge_kubeconfigs(&kubeconfig_paths)?;

    if let Some(context_name) = context_name {
        match create_config_with_context(&merged_kubeconfig, context_name).await {
            Ok(config) => match create_client_with_config(&config).await {
                Some(client) => {
                    info!("Created new client for context: {context_name}");
                    return Ok((Some(client), Some(merged_kubeconfig), all_contexts));
                }
                _ => {
                    errors.push(format!("Connection failed for context '{context_name}'"));
                }
            },
            Err(e) => {
                errors.push(format!("Config error for context '{context_name}': {e}"));
            }
        }
    } else {
        info!("No specific context provided, returning all available contexts.");
        return Ok((None, None, all_contexts));
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

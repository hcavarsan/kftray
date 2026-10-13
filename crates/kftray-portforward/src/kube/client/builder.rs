use std::env;

use anyhow::Result;
#[cfg(unix)]
use log::warn;
use log::{
    error,
    info,
};
use tokio::sync::OnceCell;

use super::KubeConnection;
use super::config::{
    create_config_with_context,
    get_kubeconfig_paths_from_option,
    merge_kubeconfigs,
};
use super::connection::create_client_with_config;

static PATH_INIT: OnceCell<()> = OnceCell::const_new();

/// Resolves and applies the process-wide `PATH` (and, on Windows,
/// `PATHEXT`) this crate's Kubernetes client creation relies on, exactly
/// once for the process. `env::set_var`/`env::remove_var` are unsafe
/// because mutating the process environment while another thread reads it
/// (`std::env::var`, a spawned `Command` inheriting env, an exec-credential
/// plugin) is undefined behaviour; `init_path` is only ever called lazily
/// from [`create_client_with_specific_context`], which can happen at an
/// arbitrary point once the async runtime already has other tasks running.
/// Callers that can run this during their own single-threaded startup —
/// before spawning the runtime's other workers — should call this instead
/// and await it there, so the one-time mutation happens with nothing else
/// racing it.
pub async fn warm_up_path_env() {
    init_path().await;
}

async fn init_path() {
    PATH_INIT
        .get_or_init(|| async {
            #[cfg(unix)]
            let resolved_path = {
                let current = env::var("PATH").unwrap_or_default();
                tokio::task::spawn_blocking({
                    let current = current.clone();
                    move || {
                        let shell = shell_path();
                        info!("init_path: shell PATH found: {}", shell.is_some());
                        with_fallback(
                            &merge_paths(shell.as_deref().unwrap_or_default(), &current),
                            &fallback_paths(),
                        )
                    }
                })
                .await
                .unwrap_or_else(|e| {
                    warn!("init_path: spawn_blocking failed ({e}), falling back to current PATH");
                    with_fallback(&current, &fallback_paths())
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

/// Login shells may print a motd or `echo` from their rc files before our
/// command runs, so the PATH is wrapped in markers and parsed out of stdout.
#[cfg(unix)]
const PATH_MARKER: &str = "__KFTRAY_PATH__";

#[cfg(unix)]
fn extract_marked_path(stdout: &str) -> Option<String> {
    let (_, rest) = stdout.split_once(PATH_MARKER)?;
    let (path, _) = rest.split_once(PATH_MARKER)?;
    let path = path.trim();
    (!path.is_empty()).then(|| path.to_string())
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

    let cmd = if shell.ends_with("/fish") {
        format!("printf '{PATH_MARKER}%s{PATH_MARKER}' (string join : $PATH)")
    } else {
        format!("printf '{PATH_MARKER}%s{PATH_MARKER}' \"$PATH\"")
    };

    let mut child = Command::new(shell)
        .args(["-lc", &cmd])
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

                let path = extract_marked_path(&stdout)?;
                return path.contains('/').then_some(path);
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
fn with_fallback(path: &str, fallbacks: &[String]) -> String {
    use std::collections::HashSet;
    use std::path::Path;

    let mut seen: HashSet<&str> = path.split(':').collect();
    let extra = fallbacks
        .iter()
        .filter(|p| Path::new(p).exists() && seen.insert(p.as_str()))
        .map(String::as_str);

    path.split(':')
        .filter(|p| !p.is_empty())
        .chain(extra)
        .collect::<Vec<_>>()
        .join(":")
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
                format!("{}...", &p[..p.floor_char_boundary(80)])
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

    // The environment goes to the local log only: in the returned error its
    // `KUBECONFIG=` text made every client failure look like a kubeconfig problem.
    error!(
        "Failed to create Kubernetes client for context '{context_name}'. Environment: {}",
        env_debug_info()
    );

    Err(anyhow::anyhow!(
        "Failed to create Kubernetes client.\n\
         Errors:\n{}",
        errors
            .iter()
            .map(|e| format!("  • {e}"))
            .collect::<Vec<_>>()
            .join("\n")
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

    #[test]
    fn shell_startup_chatter_does_not_leak_into_the_path() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let shell = dir.path().join("noisy-shell");
        std::fs::write(
            &shell,
            "#!/bin/sh\necho 'Welcome back /to/the/motd'\nPATH=/opt/tools/bin:/usr/bin\neval \"$2\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();

        let path = try_shell_path(shell.to_str().unwrap(), dir.path().to_str().unwrap());

        assert_eq!(path.as_deref(), Some("/opt/tools/bin:/usr/bin"));
    }

    #[test]
    fn fallback_dirs_that_exist_are_appended_after_the_shell_path() {
        let dir = tempfile::tempdir().unwrap();
        let present = dir.path().join("present");
        let in_shell = dir.path().join("in-shell");
        std::fs::create_dir(&present).unwrap();
        std::fs::create_dir(&in_shell).unwrap();
        let missing = dir.path().join("missing");
        let fallbacks = [&missing, &present, &in_shell].map(|p| p.to_str().unwrap().to_string());
        let shell = format!("/usr/bin:{}", in_shell.display());

        let merged = with_fallback(&merge_paths(&shell, "/bin"), &fallbacks);

        assert_eq!(merged, format!("{shell}:/bin:{}", present.display()));
    }

    #[test]
    fn unicode_path_does_not_panic_in_env_debug_info() {
        let path = format!("{}x/home/joão/.local/bin:/usr/bin", "/x".repeat(35));
        assert!(!path.is_char_boundary(80));
        let _guard = kftray_commons::test_utils::EnvVarGuard::set("PATH", &path);

        let expected = format!("PATH={}... |", &path[..79]);
        assert!(env_debug_info().contains(&expected));
    }
}

#[cfg(all(test, unix))]
mod client_failure_tests {
    use kftray_telemetry::{
        FailureKind,
        classify,
    };

    use super::*;

    #[tokio::test]
    async fn unreachable_api_server_classifies_as_connection() {
        crate::ssl::ensure_crypto_provider_installed();
        init_path().await;
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let dir = tempfile::tempdir().unwrap();
        let kubeconfig = dir.path().join("config");
        std::fs::write(
            &kubeconfig,
            format!(
                "apiVersion: v1\nkind: Config\n\
                 clusters:\n- name: c\n  cluster:\n    server: http://127.0.0.1:{port}\n\
                 contexts:\n- name: ctx\n  context:\n    cluster: c\n    user: u\n\
                 users:\n- name: u\n  user:\n    token: t\n\
                 current-context: ctx\n"
            ),
        )
        .unwrap();
        let _guard = kftray_commons::test_utils::EnvVarGuard::set("KUBECONFIG", "/tmp/x");

        let Err(error) = create_client_with_specific_context(
            Some(kubeconfig.to_string_lossy().into_owned()),
            "ctx",
        )
        .await
        else {
            panic!("a closed port must fail");
        };

        let text = error.to_string();
        assert!(!text.contains("Environment:"), "{text}");
        assert_eq!(classify(&text), FailureKind::Connection, "{text}");
    }
}

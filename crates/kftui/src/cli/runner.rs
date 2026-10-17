use std::collections::HashSet;
use std::sync::Mutex;

use futures::stream::{
    self,
    StreamExt,
};
use kftray_commons::models::config_model::Config;
use kftray_commons::utils::config::{
    get_config_with_mode,
    read_configs_with_mode,
};
use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_commons::utils::settings::{
    get_app_settings_with_mode,
    get_ssl_enabled_with_mode,
    set_ssl_auto_regenerate_with_mode,
    set_ssl_ca_auto_install_with_mode,
    set_ssl_enabled_with_mode,
};
use kftray_portforward::kube::stop_port_forward_with_mode;
use kftray_portforward::ssl::CertificateManager;
use log::{
    info,
    warn,
};
use tokio::signal;

use crate::cli::args::Cli;
use crate::cli::output;
use crate::cli::watch::DiscoveryWatch;
use crate::core;

pub struct PortForwardRunner;

impl PortForwardRunner {
    pub async fn auto_start_port_forwards(
        cli: &Cli, mode: DatabaseMode, imported_config_ids: Vec<i64>,
    ) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
        let config_ids = Self::get_config_ids_to_start(cli, mode, imported_config_ids).await?;

        if config_ids.is_empty() {
            if cli.ssl && !cli.non_interactive {
                Self::ensure_ssl_for_saved_configs(mode).await?;
            }
            return Ok(Vec::new());
        }

        let successful_config_ids = Self::start_port_forwards(cli, mode, config_ids).await?;
        Ok(successful_config_ids)
    }

    pub async fn ensure_ssl_for_saved_configs(
        mode: DatabaseMode,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let config_ids: Vec<i64> = read_configs_with_mode(mode)
            .await?
            .into_iter()
            .filter_map(|config| config.id)
            .collect();
        Self::ensure_ssl_setup_with_configs(&config_ids, mode, true).await
    }

    /// Returns the exit code of the command given after `--`, when one ran.
    pub async fn run_non_interactive_mode(
        cli: &Cli, mode: DatabaseMode, imported_config_ids: Vec<i64>,
    ) -> Result<Option<i32>, Box<dyn std::error::Error>> {
        if cli.auto_start {
            let successful_config_ids =
                Self::auto_start_port_forwards(cli, mode, imported_config_ids).await?;
            Self::handle_auto_start_non_interactive(cli, mode, successful_config_ids).await
        } else {
            Self::handle_save_only_non_interactive(cli, mode)
                .await
                .map(|()| None)
        }
    }

    async fn get_config_ids_to_start(
        cli: &Cli, mode: DatabaseMode, imported_config_ids: Vec<i64>,
    ) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
        if cli.has_config_source() && cli.filters.is_empty() {
            return Ok(imported_config_ids);
        }

        let configs = read_configs_with_mode(mode).await.map_err(|e| {
            output::error!("Error: Failed to read configurations: {e}");
            e
        })?;
        Ok(Self::select_config_ids(cli, &configs, imported_config_ids))
    }

    /// Ids of `configs` matching `--filter`, restricted to the imported ones
    /// when a config source was given.
    pub(crate) fn select_config_ids(
        cli: &Cli, configs: &[Config], imported_config_ids: Vec<i64>,
    ) -> Vec<i64> {
        let imported: HashSet<i64> = imported_config_ids.into_iter().collect();
        cli.filter_view()
            .filter(configs)
            .filter_map(|config| config.id)
            .filter(|id| !cli.has_config_source() || imported.contains(id))
            .collect()
    }

    async fn start_port_forwards(
        cli: &Cli, mode: DatabaseMode, config_ids: Vec<i64>,
    ) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
        if cli.ssl {
            Self::ensure_ssl_setup_with_configs(&config_ids, mode, true).await?;
        }

        if cli.non_interactive {
            output::status!("Starting {} port forward(s)", config_ids.len());
        }

        let (successful_config_ids, failed) = Self::start_each(cli, mode, config_ids, |_| {}).await;

        if failed > 0 {
            output::warning!("Warning: {failed} port forward(s) failed to start");
            if cli.non_interactive {
                if successful_config_ids.is_empty() && !cli.may_start_empty() {
                    output::error!(
                        "Error: All port forwards failed to start in non-interactive mode"
                    );
                    output::error!("Check the errors above for details");
                    Self::reconcile_before_exit(mode).await;
                    return Err("all port forwards failed to start in non-interactive mode".into());
                } else {
                    output::warning!(
                        "Note: {} port forward(s) started successfully",
                        successful_config_ids.len()
                    );
                }
            }
        }

        Ok(successful_config_ids)
    }

    /// Starts the configs a `--watch` round added in non-interactive mode. Each
    /// id goes into `started` as soon as its forward runs, so a shutdown that
    /// stops waiting for this batch still stops the forwards it started.
    /// A failure here never ends the run.
    pub(crate) async fn start_discovered(
        cli: &Cli, mode: DatabaseMode, config_ids: Vec<i64>, started: &Mutex<Vec<i64>>,
    ) {
        output::status!("Starting {} discovered port forward(s)", config_ids.len());

        let (successful_config_ids, failed) = Self::start_each(cli, mode, config_ids, |id| {
            if let Ok(mut started) = started.lock() {
                started.push(id);
            }
        })
        .await;
        if failed > 0 {
            output::warning!("Warning: {failed} port forward(s) failed to start");
        }

        if let Ok(started) = Self::get_configs_by_ids(mode, successful_config_ids).await {
            for config in &started {
                Self::print_configuration(config);
            }
        }
    }

    /// Returns the ids that started and how many failed. `on_started` gets
    /// each id as soon as its start succeeds.
    async fn start_each(
        cli: &Cli, mode: DatabaseMode, config_ids: Vec<i64>, mut on_started: impl FnMut(i64),
    ) -> (Vec<i64>, usize) {
        let ssl_settings_enabled = get_ssl_enabled_with_mode(mode).await.unwrap_or(false);
        let ssl_override = cli.ssl && !ssl_settings_enabled;
        let mut tasks = stream::iter(config_ids)
            .map(|config_id| async move {
                match core::port_forward::start_port_forward(config_id, mode, ssl_override).await {
                    Ok(()) => Some(config_id),
                    Err(error) => {
                        output::error!(
                            "Error: Failed to start port forward for config {config_id}: {error}"
                        );
                        None
                    }
                }
            })
            .buffer_unordered(16);

        let mut successful_config_ids = Vec::new();
        let mut failed = 0;
        while let Some(result) = tasks.next().await {
            match result {
                Some(config_id) => {
                    on_started(config_id);
                    successful_config_ids.push(config_id);
                }
                None => failed += 1,
            }
        }

        (successful_config_ids, failed)
    }

    async fn handle_auto_start_non_interactive(
        cli: &Cli, mode: DatabaseMode, successful_config_ids: Vec<i64>,
    ) -> Result<Option<i32>, Box<dyn std::error::Error>> {
        let started_configs = Self::get_configs_by_ids(mode, successful_config_ids).await?;

        if started_configs.is_empty() && !cli.may_start_empty() {
            output::error!("Error: No configurations found");
            output::error!("Ensure config source contains valid port forward configurations");
            Self::reconcile_before_exit(mode).await;
            return Err("no configurations found to run in non-interactive mode".into());
        }

        if cli.runs_command() {
            Self::print_active_configurations(&started_configs);
        } else if started_configs.is_empty() {
            output::status!("No port forwards started yet");
            output::status!("\nWatching for newly annotated services (press Ctrl+C to stop)");
        } else {
            Self::print_active_configurations(&started_configs);
            output::status!("\nRunning in non-interactive mode (press Ctrl+C to stop)");
            output::status!("Keeping port forwards active");
        }
        let discovery = cli.watch.then(|| DiscoveryWatch::spawn(cli.clone(), mode));
        if cli.runs_command() {
            return Self::run_command(&cli.command, started_configs, discovery, mode)
                .await
                .map(Some);
        }
        Self::wait_for_shutdown_signal(started_configs, discovery, mode).await?;
        Ok(None)
    }

    /// Runs the command given after `--` while the forwards are up, then
    /// stops them. Returns the command's exit code, or 127 or 126 the way a
    /// shell does when the command can't start. A command that exits 0 still
    /// fails the run when the stop leaves work behind.
    pub(crate) async fn run_command(
        command: &[String], configs: Vec<Config>, discovery: Option<DiscoveryWatch>,
        mode: DatabaseMode,
    ) -> Result<i32, Box<dyn std::error::Error>> {
        let (program, args) = command.split_first().ok_or("no command given after --")?;
        output::status!("\nRunning {}", command.join(" "));
        let mut child = tokio::process::Command::new(program);
        child.args(args);
        let (code, interrupted) = match Self::run_child(child).await {
            Ok(result) => result,
            Err(error) => {
                output::error!("Error: Failed to run {program}: {error}");
                let code = if error.kind() == std::io::ErrorKind::NotFound {
                    127
                } else {
                    126
                };
                (code, false)
            }
        };

        // As with --non-interactive, the first signal asks for the teardown
        // and only the next one aborts it. A signal during the command
        // counts as the first.
        output::status!("\nStopping port forwards");
        let stopped = Self::stop_unless_interrupted(
            Self::stop_all_started(configs, discovery, mode),
            Self::interrupts(if interrupted { 1 } else { 2 }),
        )
        .await;
        match stopped {
            Err(error) if code == 0 => Err(error),
            _ => Ok(code),
        }
    }

    /// Ctrl+C in the terminal reaches the command too, so kftui keeps waiting
    /// and lets the command decide when to exit. A SIGTERM sent to kftui
    /// alone goes on to the command. The handlers go in before the spawn, so
    /// an early Ctrl+C can't kill kftui and leave the forwards behind.
    /// Returns the exit code and whether kftui got a signal meanwhile. Fails
    /// only when the command can't start.
    async fn run_child(mut command: tokio::process::Command) -> std::io::Result<(i32, bool)> {
        let mut interrupted = false;
        #[cfg(unix)]
        let status = {
            let mut sigint = signal::unix::signal(signal::unix::SignalKind::interrupt())
                .map_err(|error| warn!("Failed to install SIGINT handler: {error}"))
                .ok();
            let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
                .map_err(|error| warn!("Failed to install SIGTERM handler: {error}"))
                .ok();
            let mut child = command.spawn()?;
            loop {
                tokio::select! {
                    status = child.wait() => break status,
                    _ = Self::recv_signal(&mut sigint) => interrupted = true,
                    _ = Self::recv_signal(&mut sigterm) => {
                        interrupted = true;
                        if let Some(pid) = child.id() {
                            // SAFETY: kill only sends a signal to the pid of a
                            // child this process has not reaped yet.
                            unsafe {
                                libc::kill(pid as libc::pid_t, libc::SIGTERM);
                            }
                        }
                    }
                }
            }
        };
        #[cfg(windows)]
        let status = {
            let mut ctrl_c = signal::windows::ctrl_c()
                .map_err(|error| warn!("Failed to install Ctrl+C handler: {error}"))
                .ok();
            let mut child = command.spawn()?;
            loop {
                tokio::select! {
                    status = child.wait() => break status,
                    received = async {
                        match ctrl_c.as_mut() {
                            Some(ctrl_c) => ctrl_c.recv().await,
                            None => std::future::pending().await,
                        }
                    } => {
                        if received.is_none() {
                            break child.wait().await;
                        }
                        interrupted = true;
                    }
                }
            }
        };

        let code = match status {
            Ok(status) => Self::exit_code(status),
            Err(error) => {
                output::error!("Error: Failed to wait for the command: {error}");
                1
            }
        };
        Ok((code, interrupted))
    }

    /// A command killed by a signal exits with 128 plus the signal number,
    /// the way a shell reports it.
    fn exit_code(status: std::process::ExitStatus) -> i32 {
        #[cfg(unix)]
        if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&status) {
            return 128 + signal;
        }
        status.code().unwrap_or(1)
    }

    #[cfg(unix)]
    async fn recv_signal(signal: &mut Option<signal::unix::Signal>) {
        if let Some(signal) = signal
            && signal.recv().await.is_some()
        {
            return;
        }
        std::future::pending::<()>().await
    }

    /// Resolves after `count` Ctrl+C or SIGTERM signals. A signal that
    /// arrived before the call does not count.
    async fn interrupts(count: usize) {
        #[cfg(unix)]
        {
            let mut sigint = signal::unix::signal(signal::unix::SignalKind::interrupt()).ok();
            let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate()).ok();
            for _ in 0..count {
                tokio::select! {
                    _ = Self::recv_signal(&mut sigint) => {}
                    _ = Self::recv_signal(&mut sigterm) => {}
                }
            }
        }
        #[cfg(windows)]
        {
            let mut ctrl_c = signal::windows::ctrl_c().ok();
            for _ in 0..count {
                let received = match ctrl_c.as_mut() {
                    Some(ctrl_c) => ctrl_c.recv().await,
                    None => None,
                };
                if received.is_none() {
                    std::future::pending::<()>().await
                }
            }
        }
    }

    /// Races the stop against a second interrupt instead of letting tokio's
    /// still-installed handlers swallow it: a user stuck on a stalled
    /// recovery lock or cluster delete can then abort short of SIGKILL.
    async fn stop_unless_interrupted(
        stop: impl Future<Output = Result<(), Box<dyn std::error::Error>>>,
        interrupt: impl Future<Output = ()>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        tokio::select! {
            result = stop => result,
            _ = interrupt => {
                output::warning!(
                    "Warning: a second interrupt was received; exiting without waiting \
                     for the stop to finish. Stops already dispatched keep running \
                     detached in the background."
                );
                Err("shutdown interrupted by a second signal before stopping finished".into())
            }
        }
    }

    /// Reconciles anything a start abandoned before an early, non-interactive
    /// exit — the same reconcile pass the interactive shutdown paths always
    /// run before terminating, so exiting here still reaps a relay
    /// Deployment, an address claim, or a hosts entry a partial start left
    /// behind.
    async fn reconcile_before_exit(mode: DatabaseMode) {
        let (still_owed, cleanup_result) = crate::core::port_forward::reconcile_shutdown_cleanup(
            mode,
            &HashSet::new(),
            crate::core::port_forward::CLEANUP_RECONCILE_TIMEOUT,
        )
        .await;
        if !still_owed.is_empty() {
            output::warning!(
                "Warning: cleanup for configuration(s) {still_owed:?} did not complete; they \
                 stay marked running and are retried on the next stop"
            );
        }
        if let Err(error) = cleanup_result {
            output::warning!("Warning: failed to clean up configuration states: {error}");
        }
    }

    async fn handle_save_only_non_interactive(
        cli: &Cli, mode: DatabaseMode,
    ) -> Result<(), Box<dyn std::error::Error>> {
        output::status!("Configurations processed");
        if !cli.watch {
            return Ok(());
        }

        output::status!("\nWatching for newly annotated services (press Ctrl+C to stop)");
        let discovery = DiscoveryWatch::spawn(cli.clone(), mode);
        Self::wait_for_shutdown_signal(Vec::new(), Some(discovery), mode).await
    }

    async fn get_configs_by_ids(
        mode: DatabaseMode, config_ids: Vec<i64>,
    ) -> Result<Vec<Config>, Box<dyn std::error::Error>> {
        let all_configs = read_configs_with_mode(mode).await.map_err(|e| {
            output::error!("Error: Failed to read configurations: {e}");
            Box::new(std::io::Error::other(e)) as Box<dyn std::error::Error>
        })?;

        let target_ids: HashSet<i64> = config_ids.into_iter().collect();
        Ok(all_configs
            .into_iter()
            .filter(|config| target_ids.contains(&config.id.unwrap_or(-1)))
            .collect())
    }

    fn print_active_configurations(configs: &[Config]) {
        output::status!("Port forwards started");
        output::status!("Active configurations: {}", configs.len());

        for config in configs {
            Self::print_configuration(config);
        }
    }

    fn print_configuration(config: &Config) {
        let alias = config.alias.as_deref().unwrap_or("unnamed");
        let local_port = config.local_port.unwrap_or(0);
        let remote_port = config.remote_port.unwrap_or(0);
        let namespace = &config.namespace;
        let service_or_target = match config.workload_type.as_deref() {
            Some("proxy") => config.remote_address.as_deref().unwrap_or("unknown"),
            Some("pod") => config.target.as_deref().unwrap_or("unknown"),
            Some("service") => config.service.as_deref().unwrap_or("unknown"),
            _ => config.service.as_deref().unwrap_or("unknown"),
        };

        output::status!(
            "  {alias} {local_port}:{remote_port} -> {service_or_target}:{remote_port} ({namespace})"
        );
    }

    /// Fails when teardown left something behind, so a script running this
    /// mode can tell an incomplete shutdown from a clean one by the exit code.
    async fn wait_for_shutdown_signal(
        configs: Vec<Config>, discovery: Option<DiscoveryWatch>, mode: DatabaseMode,
    ) -> Result<(), Box<dyn std::error::Error>> {
        #[cfg(unix)]
        {
            let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate())
                .map_err(|error| warn!("Failed to install SIGTERM handler: {error}"))
                .ok();

            tokio::select! {
                result = signal::ctrl_c() => {
                    if let Err(error) = result {
                        warn!("Failed to install Ctrl+C handler: {error}");
                        match sigterm.as_mut() {
                            Some(sigterm) => {
                                output::warning!(
                                    "Warning: failed to install a Ctrl+C handler ({error}); \
                                     waiting for SIGTERM instead"
                                );
                                sigterm.recv().await;
                            }
                            None => {
                                output::error!(
                                    "Error: no shutdown signal handler could be installed; \
                                     stopping port forwards and exiting"
                                );
                                Self::stop_all_started(configs, discovery, mode).await?;
                                return Err(
                                    "no shutdown signal handler could be installed".into()
                                );
                            }
                        }
                    }
                }
                _ = async {
                    match sigterm.as_mut() {
                        Some(sigterm) => {
                            sigterm.recv().await;
                        }
                        None => std::future::pending::<()>().await,
                    }
                } => {}
            }
            output::status!("\nStopping port forwards");

            Self::stop_unless_interrupted(Self::stop_all_started(configs, discovery, mode), async {
                match sigterm.as_mut() {
                    Some(sigterm) => {
                        tokio::select! {
                            _ = signal::ctrl_c() => {}
                            _ = sigterm.recv() => {}
                        }
                    }
                    None => {
                        let _ = signal::ctrl_c().await;
                    }
                }
            })
            .await
        }
        #[cfg(not(unix))]
        {
            if let Err(error) = signal::ctrl_c().await {
                warn!("Failed to install Ctrl+C handler: {error}");
                output::error!(
                    "Error: no shutdown signal handler could be installed; stopping port \
                     forwards and exiting"
                );
                Self::stop_all_started(configs, discovery, mode).await?;
                return Err("no shutdown signal handler could be installed".into());
            }
            output::status!("\nStopping port forwards");

            Self::stop_unless_interrupted(Self::stop_all_started(configs, discovery, mode), async {
                let _ = signal::ctrl_c().await;
            })
            .await
        }
    }

    /// Stops the `--watch` rounds first, so the forwards they started are
    /// stopped together with the ones started at launch.
    async fn stop_all_started(
        mut configs: Vec<Config>, discovery: Option<DiscoveryWatch>, mode: DatabaseMode,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(discovery) = discovery {
            let started_ids = discovery
                .stop(crate::core::port_forward::CLEANUP_RECONCILE_TIMEOUT)
                .await;
            if !started_ids.is_empty()
                && let Ok(started) = Self::get_configs_by_ids(mode, started_ids).await
            {
                configs.extend(started);
            }
        }
        Self::stop_all_port_forwards(&configs, mode).await
    }

    async fn stop_all_port_forwards(
        configs: &[Config], mode: DatabaseMode,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let dispatched_ids: HashSet<i64> = configs.iter().filter_map(|config| config.id).collect();

        // Every stop is spawned onto the runtime right here, before any
        // waiting begins: a config past whatever `drain_stop_tasks` awaits
        // concurrently is not left queued behind a lazily polled stream, so
        // it starts stopping immediately instead of only once an earlier
        // stop's join future happens to be polled.
        let handles: Vec<(i64, tokio::task::JoinHandle<Result<(), String>>)> = dispatched_ids
            .iter()
            .copied()
            .map(|config_id| {
                let handle = tokio::spawn(async move {
                    Self::stop_single_port_forward(config_id, mode)
                        .await
                        .map_err(|error| format!("Config {config_id}: {error}"))
                });
                (config_id, handle)
            })
            .collect();

        // Both phases below (this drain and the reconcile pass further down)
        // share one overall deadline instead of each getting its own full
        // `CLEANUP_RECONCILE_TIMEOUT`: a caller bounded by a supervisor's own
        // stop timeout (systemd, Docker, Kubernetes) needs the total wait
        // bounded too, not doubled.
        let deadline_at =
            tokio::time::Instant::now() + crate::core::port_forward::CLEANUP_RECONCILE_TIMEOUT;
        let (results, completed_ids, drained_ok) = Self::drain_stop_tasks(
            handles,
            deadline_at.saturating_duration_since(tokio::time::Instant::now()),
        )
        .await;

        let mut stop_errors = Vec::new();
        let mut stopped_count = 0;
        for (_, result) in results {
            match result {
                Ok(()) => stopped_count += 1,
                Err(e) => stop_errors.push(e),
            }
        }

        output::status!("Stopped {stopped_count} port forward(s)");

        let mut failures: Vec<String> = Vec::new();
        if stop_errors.is_empty() && drained_ok {
            output::status!("Port forwards stopped");
        } else if !stop_errors.is_empty() {
            output::warning!("Warning: Some port forwards failed to stop properly");
            for error in &stop_errors {
                output::warning!("  {error}");
            }
            failures.extend(stop_errors);
        }

        let unfinished_stop_ids: HashSet<i64> =
            dispatched_ids.difference(&completed_ids).copied().collect();
        if !drained_ok {
            let still_owed: Vec<i64> = unfinished_stop_ids.iter().copied().collect();
            let message = format!(
                "stop for configuration(s) {still_owed:?} did not finish within the shutdown \
                 budget; they stay marked running and are retried on the next stop"
            );
            output::warning!("Warning: {message}");
            failures.push(message);
        }

        // The cleanup registry lives only in this process, so anything a failed
        // stop left outstanding has to be retried before it exits. Both
        // interactive exits do this; without it a transient delete failure on
        // Ctrl+C leaks a relay Deployment with nothing left to remove it. Ids
        // still mid-stop above are excluded here too: they are still running
        // detached and hold their per-config recovery lock, so reconciling or
        // re-stopping them here would only contend for it.
        let (still_owed, cleanup_result) = crate::core::port_forward::reconcile_shutdown_cleanup(
            mode,
            &unfinished_stop_ids,
            deadline_at.saturating_duration_since(tokio::time::Instant::now()),
        )
        .await;
        if !still_owed.is_empty() {
            let message = format!(
                "cleanup for configuration(s) {still_owed:?} did not complete; they stay \
                 marked running and are retried on the next stop"
            );
            output::warning!("Warning: {message}");
            failures.push(message);
        }

        if let Err(error) = cleanup_result {
            output::warning!("Warning: failed to clean up configuration states: {error}");
            failures.push(format!("failed to clean up configuration states: {error}"));
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(format!("shutdown left work outstanding: {}", failures.join("; ")).into())
        }
    }

    async fn stop_single_port_forward(
        config_id: i64, mode: DatabaseMode,
    ) -> Result<(), Box<dyn std::error::Error>> {
        stop_port_forward_with_mode(config_id.to_string(), mode)
            .await
            .map(|_| ())
            .map_err(|e| Box::new(std::io::Error::other(e)) as Box<dyn std::error::Error>)
    }

    /// Awaits every already-spawned handle, each bounded by whatever of
    /// `deadline` remains after the ones before it. Every handle was spawned
    /// by the caller (`stop_all_port_forwards`) before this function was
    /// ever called, so a stop still running when its slice of the deadline
    /// elapses keeps running to completion detached in the background
    /// instead of being cancelled mid-cleanup (which could leave a relay or
    /// an address claim half released): the `tokio::time::timeout` here only
    /// drops the future joining it, not the spawned task itself. The caller
    /// learns which ids never reported back
    /// (`dispatched_ids.difference(&completed_ids)`) and excludes them from
    /// cleanup work that would otherwise contend for their per-config
    /// recovery lock.
    pub(crate) async fn drain_stop_tasks(
        handles: Vec<(i64, tokio::task::JoinHandle<Result<(), String>>)>,
        deadline: std::time::Duration,
    ) -> (Vec<(i64, Result<(), String>)>, HashSet<i64>, bool) {
        let mut results = Vec::new();
        let mut completed_ids: HashSet<i64> = HashSet::new();
        let deadline_at = tokio::time::Instant::now() + deadline;
        let mut drained_ok = true;
        for (id, handle) in handles {
            let remaining = deadline_at.saturating_duration_since(tokio::time::Instant::now());
            match tokio::time::timeout(remaining, handle).await {
                Ok(Ok(result)) => {
                    completed_ids.insert(id);
                    results.push((id, result));
                }
                Ok(Err(join_error)) => {
                    completed_ids.insert(id);
                    results.push((
                        id,
                        Err(format!("Config {id}: stop task panicked: {join_error}")),
                    ));
                }
                Err(_) => {
                    drained_ok = false;
                }
            }
        }
        (results, completed_ids, drained_ok)
    }

    /// `announce` prints the outcome outside the TUI. The TUI passes `false`,
    /// since output there would land on top of the drawn screen.
    pub(crate) async fn ensure_ssl_setup_with_configs(
        config_ids: &[i64], mode: DatabaseMode, announce: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let ssl_already_enabled = get_ssl_enabled_with_mode(mode).await.unwrap_or(false);

        if ssl_already_enabled {
            let needs_cert_regeneration =
                Self::check_cert_needs_regeneration(config_ids, mode).await?;
            if !needs_cert_regeneration {
                info!("SSL already enabled with valid certificate for current configs");
                return Ok(());
            }
            info!("SSL enabled but certificate needs regeneration for new configs");
        } else {
            info!("Enabling SSL for CLI session");
        }

        set_ssl_enabled_with_mode(true, mode)
            .await
            .map_err(|e| format!("Failed to enable SSL: {}", e))?;
        set_ssl_ca_auto_install_with_mode(true, mode)
            .await
            .map_err(|e| format!("Failed to set CA auto install: {}", e))?;
        set_ssl_auto_regenerate_with_mode(true, mode)
            .await
            .map_err(|e| format!("Failed to set SSL auto regenerate: {}", e))?;

        let settings = get_app_settings_with_mode(mode)
            .await
            .map_err(|e| format!("Failed to get app settings: {}", e))?;
        let cert_manager = CertificateManager::new(&settings)?;

        let cert_result = match mode {
            DatabaseMode::File => cert_manager
                .regenerate_certificate_for_all_configs()
                .await
                .map(|_| ()),
            DatabaseMode::Memory => {
                let current_configs = Self::get_current_configs(config_ids, mode).await?;
                Self::regenerate_certificate_for_memory_configs(&cert_manager, &current_configs)
                    .await
            }
        };

        match cert_result {
            Ok(_) => {
                info!("Generated SSL certificates for all configs");

                match cert_manager.ensure_ca_installed_and_trusted().await {
                    Ok(_) => {
                        info!("CA certificate installed and trusted");
                        if announce {
                            output::status!("SSL/HTTPS enabled for port forwarding");
                            output::status!("CA certificate installed and trusted in system");
                            output::status!("You may need to restart your browser to take effect");

                            let example_url = Self::get_example_https_url(config_ids, mode).await;
                            output::status!(
                                "Access your services via HTTPS (e.g., {})",
                                example_url
                            );
                        }
                    }
                    Err(e) => {
                        warn!("Failed to install CA certificate: {}", e);
                        if announce {
                            output::warning!("SSL enabled but CA certificate installation failed");
                            output::warning!(
                                "You may need to manually trust the certificate in your browser"
                            );
                        }
                    }
                }
            }
            Err(e) => {
                warn!("Failed to generate SSL certificates: {}", e);
                return Err(format!("Failed to generate SSL certificates: {}", e).into());
            }
        }

        Ok(())
    }

    async fn check_cert_needs_regeneration(
        config_ids: &[i64], mode: DatabaseMode,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        let settings = get_app_settings_with_mode(mode)
            .await
            .map_err(|e| format!("Failed to get app settings: {}", e))?;
        let cert_manager = CertificateManager::new(&settings)?;

        let current_aliases: std::collections::HashSet<String> = {
            let mut aliases = std::collections::HashSet::new();
            for &config_id in config_ids {
                if let Ok(config) = get_config_with_mode(config_id, mode).await
                    && let Some(alias) = config.alias
                {
                    aliases.insert(alias);
                }
            }
            aliases
        };

        if current_aliases.is_empty() {
            return Ok(false);
        }

        match cert_manager.list_all_certificates().await {
            Ok(cert_infos) => {
                if cert_infos.is_empty() {
                    return Ok(true);
                }

                let existing_domains: std::collections::HashSet<String> = cert_infos
                    .into_iter()
                    .map(|cert_info| cert_info.domain)
                    .collect();

                for alias in &current_aliases {
                    if !existing_domains.contains(alias) {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Err(_) => Ok(true),
        }
    }

    async fn get_current_configs(
        config_ids: &[i64], mode: DatabaseMode,
    ) -> Result<Vec<Config>, Box<dyn std::error::Error>> {
        let mut configs = Vec::new();
        for &config_id in config_ids {
            if let Ok(config) = get_config_with_mode(config_id, mode).await {
                configs.push(config);
            }
        }
        Ok(configs)
    }

    async fn regenerate_certificate_for_memory_configs(
        cert_manager: &CertificateManager, configs: &[Config],
    ) -> Result<(), anyhow::Error> {
        let aliases: Vec<String> = configs
            .iter()
            .filter_map(|config| config.alias.clone())
            .collect();

        if aliases.is_empty() {
            return Ok(());
        }

        for alias in aliases {
            cert_manager.regenerate_certificate(&alias).await?;
        }

        let mut domains = CertificateManager::collect_all_domains_from_configs().await?;
        domains.extend(CertificateManager::domains_for_configs(configs));
        domains.sort();
        domains.dedup();
        cert_manager.regenerate_global_certificate(&domains).await?;
        Ok(())
    }

    async fn get_example_https_url(config_ids: &[i64], mode: DatabaseMode) -> String {
        for &config_id in config_ids {
            if let Ok(config) = get_config_with_mode(config_id, mode).await
                && let (Some(alias), Some(local_port)) = (config.alias, config.local_port)
            {
                return format!("https://{}:{}", alias, local_port);
            }
        }
        "https://your-service:port".to_string()
    }
}

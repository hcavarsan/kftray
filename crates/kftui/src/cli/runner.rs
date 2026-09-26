use std::collections::HashSet;

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
    get_app_settings,
    get_ssl_enabled,
    set_ssl_auto_regenerate,
    set_ssl_ca_auto_install,
    set_ssl_enabled,
};
use kftray_portforward::kube::stop_port_forward_with_mode;
use kftray_portforward::ssl::CertificateManager;
use log::{
    info,
    warn,
};
use tokio::signal;

use crate::cli::args::Cli;
use crate::core;

pub struct PortForwardRunner;

impl PortForwardRunner {
    pub async fn auto_start_port_forwards(
        cli: &Cli, mode: DatabaseMode, imported_config_ids: Vec<i64>,
    ) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
        let config_ids = Self::get_config_ids_to_start(cli, mode, imported_config_ids).await?;

        if config_ids.is_empty() {
            return Ok(Vec::new());
        }

        let successful_config_ids = Self::start_port_forwards(cli, mode, config_ids).await?;
        Ok(successful_config_ids)
    }

    pub async fn run_non_interactive_mode(
        cli: &Cli, mode: DatabaseMode, imported_config_ids: Vec<i64>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if cli.auto_start {
            let successful_config_ids =
                Self::auto_start_port_forwards(cli, mode, imported_config_ids).await?;
            Self::handle_auto_start_non_interactive(cli, mode, successful_config_ids).await
        } else {
            Self::handle_save_only_non_interactive().await
        }
    }

    async fn get_config_ids_to_start(
        cli: &Cli, mode: DatabaseMode, imported_config_ids: Vec<i64>,
    ) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
        if cli.has_config_source() && cli.filters.is_empty() {
            return Ok(imported_config_ids);
        }

        let configs = read_configs_with_mode(mode).await.map_err(|e| {
            eprintln!("Error: Failed to read configurations: {e}");
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
            Self::ensure_ssl_setup_with_configs(&config_ids, mode).await?;
        }

        if cli.non_interactive {
            println!("Starting {} port forward(s)", config_ids.len());
        }

        let ssl_settings_enabled = get_ssl_enabled().await.unwrap_or(false);
        let ssl_override = cli.ssl && !ssl_settings_enabled;
        let mut errors = Vec::new();
        let mut tasks = stream::iter(config_ids)
            .map(|config_id| async move {
                match core::port_forward::start_port_forward(config_id, mode, ssl_override).await {
                    Ok(()) => Ok(config_id),
                    Err(error) => {
                        eprintln!(
                            "Error: Failed to start port forward for config {config_id}: {error}"
                        );
                        Err((config_id, error))
                    }
                }
            })
            .buffer_unordered(16);

        let mut successful_config_ids = Vec::new();
        while let Some(result) = tasks.next().await {
            match result {
                Ok(config_id) => successful_config_ids.push(config_id),
                Err((config_id, e)) => errors.push((config_id, e)),
            }
        }

        if !errors.is_empty() {
            eprintln!("Warning: {} port forward(s) failed to start", errors.len());
            if cli.non_interactive {
                if successful_config_ids.is_empty() {
                    eprintln!("Error: All port forwards failed to start in non-interactive mode");
                    eprintln!("Check the errors above for details");
                    Self::reconcile_before_exit(mode).await;
                    return Err("all port forwards failed to start in non-interactive mode".into());
                } else {
                    eprintln!(
                        "Note: {} port forward(s) started successfully",
                        successful_config_ids.len()
                    );
                }
            }
        }

        Ok(successful_config_ids)
    }

    async fn handle_auto_start_non_interactive(
        _cli: &Cli, mode: DatabaseMode, successful_config_ids: Vec<i64>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let started_configs = Self::get_configs_by_ids(mode, successful_config_ids).await?;

        if started_configs.is_empty() {
            eprintln!("Error: No configurations found");
            eprintln!("Ensure config source contains valid port forward configurations");
            Self::reconcile_before_exit(mode).await;
            return Err("no configurations found to run in non-interactive mode".into());
        }

        Self::print_active_configurations(&started_configs);
        Self::wait_for_shutdown_signal(&started_configs, mode).await?;
        Ok(())
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
            eprintln!(
                "Warning: cleanup for configuration(s) {still_owed:?} did not complete; they \
                 stay marked running and are retried on the next stop"
            );
        }
        if let Err(error) = cleanup_result {
            eprintln!("Warning: failed to clean up configuration states: {error}");
        }
    }

    async fn handle_save_only_non_interactive() -> Result<(), Box<dyn std::error::Error>> {
        println!("Configurations processed");
        Ok(())
    }

    async fn get_configs_by_ids(
        mode: DatabaseMode, config_ids: Vec<i64>,
    ) -> Result<Vec<Config>, Box<dyn std::error::Error>> {
        let all_configs = read_configs_with_mode(mode).await.map_err(|e| {
            eprintln!("Error: Failed to read configurations: {e}");
            Box::new(std::io::Error::other(e)) as Box<dyn std::error::Error>
        })?;

        let target_ids: HashSet<i64> = config_ids.into_iter().collect();
        Ok(all_configs
            .into_iter()
            .filter(|config| target_ids.contains(&config.id.unwrap_or(-1)))
            .collect())
    }

    fn print_active_configurations(configs: &[Config]) {
        println!("Port forwards started");
        println!("Active configurations: {}", configs.len());

        for config in configs {
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

            println!(
                "  {alias} {local_port}:{remote_port} -> {service_or_target}:{remote_port} ({namespace})"
            );
        }

        println!("\nRunning in non-interactive mode (press Ctrl+C to stop)");
        println!("Keeping port forwards active");
    }

    /// Fails when teardown left something behind, so a script running this
    /// mode can tell an incomplete shutdown from a clean one by the exit code.
    async fn wait_for_shutdown_signal(
        configs: &[Config], mode: DatabaseMode,
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
                                eprintln!(
                                    "Warning: failed to install a Ctrl+C handler ({error}); \
                                     waiting for SIGTERM instead"
                                );
                                sigterm.recv().await;
                            }
                            None => {
                                eprintln!(
                                    "Error: no shutdown signal handler could be installed; \
                                     stopping port forwards and exiting"
                                );
                                Self::stop_all_port_forwards(configs, mode).await?;
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
            println!("\nStopping port forwards");

            // A second signal during the stop below is raced against it
            // instead of being silently swallowed by tokio's still-installed
            // handlers: without this, a user stuck waiting on a stalled
            // recovery lock or cluster delete has no way to abort short of
            // SIGKILL.
            let stop = Self::stop_all_port_forwards(configs, mode);
            tokio::pin!(stop);
            tokio::select! {
                result = &mut stop => result,
                _ = async {
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
                } => {
                    eprintln!(
                        "Warning: a second interrupt was received; exiting without waiting \
                         for the stop to finish. Stops already dispatched keep running \
                         detached in the background."
                    );
                    Err("shutdown interrupted by a second signal before stopping finished".into())
                }
            }
        }
        #[cfg(not(unix))]
        {
            if let Err(error) = signal::ctrl_c().await {
                warn!("Failed to install Ctrl+C handler: {error}");
                eprintln!(
                    "Error: no shutdown signal handler could be installed; stopping port \
                     forwards and exiting"
                );
                Self::stop_all_port_forwards(configs, mode).await?;
                return Err("no shutdown signal handler could be installed".into());
            }
            println!("\nStopping port forwards");

            let stop = Self::stop_all_port_forwards(configs, mode);
            tokio::pin!(stop);
            tokio::select! {
                result = &mut stop => result,
                _ = signal::ctrl_c() => {
                    eprintln!(
                        "Warning: a second interrupt was received; exiting without waiting \
                         for the stop to finish. Stops already dispatched keep running \
                         detached in the background."
                    );
                    Err("shutdown interrupted by a second signal before stopping finished".into())
                }
            }
        }
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

        println!("Stopped {stopped_count} port forward(s)");

        let mut failures: Vec<String> = Vec::new();
        if stop_errors.is_empty() && drained_ok {
            println!("Port forwards stopped");
        } else if !stop_errors.is_empty() {
            eprintln!("Warning: Some port forwards failed to stop properly");
            for error in &stop_errors {
                eprintln!("  {error}");
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
            eprintln!("Warning: {message}");
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
            eprintln!("Warning: {message}");
            failures.push(message);
        }

        if let Err(error) = cleanup_result {
            eprintln!("Warning: failed to clean up configuration states: {error}");
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

    async fn ensure_ssl_setup_with_configs(
        config_ids: &[i64], mode: DatabaseMode,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let ssl_already_enabled = get_ssl_enabled().await.unwrap_or(false);

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

        set_ssl_enabled(true)
            .await
            .map_err(|e| format!("Failed to enable SSL: {}", e))?;
        set_ssl_ca_auto_install(true)
            .await
            .map_err(|e| format!("Failed to set CA auto install: {}", e))?;
        set_ssl_auto_regenerate(true)
            .await
            .map_err(|e| format!("Failed to set SSL auto regenerate: {}", e))?;

        let settings = get_app_settings()
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
                        println!("SSL/HTTPS enabled for port forwarding");
                        println!("CA certificate installed and trusted in system");
                        println!("You may need to restart your browser to take effect");

                        let example_url = Self::get_example_https_url(config_ids, mode).await;
                        println!("Access your services via HTTPS (e.g., {})", example_url);
                    }
                    Err(e) => {
                        warn!("Failed to install CA certificate: {}", e);
                        println!("SSL enabled but CA certificate installation failed");
                        println!("You may need to manually trust the certificate in your browser");
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
        let settings = get_app_settings()
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

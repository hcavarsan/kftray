use std::collections::HashSet;
use std::time::Duration;

use kftray_commons::models::config_model::Config;
use kftray_commons::utils::config::get_config_with_mode;
use kftray_commons::utils::config_state::cleanup_current_process_config_states_with_mode;
use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_portforward::kube::{
    deploy_and_forward_pod_with_mode,
    reconcile_pending_cleanup,
    start_port_forward_with_mode as kube_start_port_forward,
    stop_all_port_forward_with_deadline,
    stop_port_forward_with_mode,
};
use log::error;

use crate::tui::input::App;

/// How long exit waits for cleanup targets that have not settled yet.
///
/// Covers the backend's uncertainty window with room for the retries inside it:
/// a create abandoned on the way out can still be applied, and the registry
/// tracking it does not survive the process.
pub(crate) const CLEANUP_RECONCILE_TIMEOUT: Duration =
    kftray_portforward::kube::UNCERTAIN_CREATE_WINDOW.saturating_mul(2);

pub async fn start_port_forwarding(config: Config, mode: DatabaseMode) -> Result<(), String> {
    start_port_forwarding_with_ssl(config, mode, false).await
}

pub async fn start_port_forwarding_with_ssl(
    config: Config, mode: DatabaseMode, ssl_override: bool,
) -> Result<(), String> {
    let result = match config.workload_type.as_deref() {
        Some("proxy") => {
            deploy_and_forward_pod_with_mode(vec![config.clone()], mode, ssl_override).await
        }
        Some("expose") => {
            kube_start_port_forward(vec![config.clone()], "tcp", mode, ssl_override).await
        }
        Some("service") | Some("pod") | None => {
            if config.workload_type.is_none() && config.service.is_none() {
                return Err(
                    "Config is missing a workload type and has no service name to fall back on"
                        .to_string(),
                );
            }
            match config.protocol.as_str() {
                "tcp" => {
                    kube_start_port_forward(vec![config.clone()], "tcp", mode, ssl_override).await
                }
                "udp" => {
                    deploy_and_forward_pod_with_mode(vec![config.clone()], mode, ssl_override).await
                }
                protocol => return Err(format!("Unsupported protocol: {protocol}")),
            }
        }
        workload => return Err(format!("Unsupported workload type: {workload:?}")),
    };

    let responses = match result {
        Ok(responses) => responses,
        Err(e) => {
            error!("Failed to start port forward: {e:?}");
            return Err(format!("Failed to start port forward: {e:?}"));
        }
    };
    // Every caller above dispatches a single-config batch (one `Config`
    // wrapped in a one-element `vec!`), so collapsing a mixed-batch outcome
    // into a single `Err` below cannot lose a partial success today. If this
    // is ever called with a real multi-config batch, this catches it before
    // the collapsed error starts silently hiding forwards that did start.
    debug_assert_eq!(
        responses.len(),
        1,
        "start_port_forwarding_with_ssl only ever dispatches a single-config batch"
    );
    if let Err(message) = kftray_commons::models::response::batch_failure(&responses) {
        error!("Failed to start port forward: {message}");
        return Err(format!("Failed to start port forward: {message}"));
    }

    Ok(())
}

pub async fn stop_port_forwarding(config: Config, mode: DatabaseMode) -> Result<(), String> {
    let config_id = config.id.ok_or("Config has no ID")?;
    stop_port_forward_with_mode(config_id.to_string(), mode)
        .await
        .map(|_| ())
        .map_err(|error| format!("Failed to stop port forward: {error}"))
}

/// Stops every forward this process owns, then reconciles anything a create
/// left abandoned on the way out. Used by `run_tui`'s single shutdown path
/// regardless of why the event loop ended (user quit, Ctrl+C, menu exit, or
/// an error), so there is exactly one place that knows how to wind a running
/// process down. Returns whatever failed, for the caller to report and turn
/// into an exit code. In-session failures that were already shown to the
/// user (or logged) are not included here: `app.finish_forwarding` already
/// logs and prints them, and folding them in would make an otherwise clean
/// shutdown exit non-zero and print them a second time.
pub async fn shutdown_port_forwarding(app: &mut App, mode: DatabaseMode) -> Vec<String> {
    log::debug!("Stopping all port forwards in mode: {mode:?}...");

    let mut failures: Vec<String> = Vec::new();
    let (_shutdown_reports, detached_ids) = app.finish_forwarding().await;

    // Ids kftui just detached are excluded: a start or stop still running in
    // the background holds the per-config recovery lock, so re-stopping or
    // reconciling them here would only contend for the same lock instead of
    // finishing sooner. Each stop below runs as its own spawned task bounded
    // by the same deadline; one still running past it keeps running detached
    // in the background instead of being cancelled mid-cleanup, and its id
    // comes back as unfinished instead of being silently dropped.
    //
    // Both phases below share one overall deadline instead of each getting
    // its own full `CLEANUP_RECONCILE_TIMEOUT`: a caller bounded by a
    // supervisor's own stop timeout (systemd, Docker, Kubernetes) needs the
    // total wait bounded too, not doubled.
    let deadline_at = tokio::time::Instant::now() + CLEANUP_RECONCILE_TIMEOUT;
    let mut unfinished_ids: HashSet<i64> = detached_ids.clone();
    match stop_all_port_forward_with_deadline(
        mode,
        &detached_ids,
        deadline_at.saturating_duration_since(tokio::time::Instant::now()),
    )
    .await
    {
        Ok((responses, unfinished)) => {
            for response in responses {
                if response.status != 0 {
                    let message = format!("Error stopping port forward: {:?}", response.stderr);
                    error!("{message}");
                    eprintln!("{message}");
                    failures.push(message);
                }
            }
            if !unfinished.is_empty() {
                let message = format!(
                    "Stopping port forward(s) {unfinished:?} did not finish within the \
                     shutdown budget; they stay marked running and are retried on the next \
                     stop"
                );
                error!("{message}");
                eprintln!("{message}");
                failures.push(message);
                unfinished_ids.extend(unfinished);
            }
        }
        Err(error) => {
            let message = format!("Failed to stop port forwards: {error}");
            error!("{message}");
            eprintln!("{message}");
            failures.push(message);
        }
    }

    // A create abandoned on the way out can surface after that first pass, and
    // the registry that tracks it lives only in this process. Ids still
    // mid-stop above are excluded here too: they are running detached and
    // still hold their per-config recovery lock.
    let (still_owed, cleanup_result) = reconcile_shutdown_cleanup(
        mode,
        &unfinished_ids,
        deadline_at.saturating_duration_since(tokio::time::Instant::now()),
    )
    .await;

    if !still_owed.is_empty() {
        let message = format!(
            "Cleanup for configuration(s) {still_owed:?} did not complete; they stay marked \
             running and are retried on the next stop"
        );
        error!("{message}");
        eprintln!("{message}");
        failures.push(message);
    }
    if let Err(error) = cleanup_result {
        let message = format!("Failed to clean up configuration states: {error}");
        error!("{message}");
        eprintln!("{message}");
        failures.push(message);
    }

    log::debug!("Port forwarding shutdown complete");
    failures
}

/// Reconciles any create still abandoned on the way out, then marks every
/// configuration this process was running as stopped, except the ones still
/// owed: those keep their `is_running` state so the next run's stop-all
/// enumerates and retries them, instead of leaving them behind a dead pid.
/// `exclude` skips ids a caller already knows are still being handled by a
/// detached task holding their per-config recovery lock; those ids are
/// folded into the database update too, so their `is_running` state is left
/// alone until the detached task finishes, but they are not reported back as
/// a reconciliation failure since nothing was attempted for them here.
/// Returns the config ids reconciliation itself could not settle within
/// `deadline`, alongside the cleanup outcome, so each shutdown path can
/// report and choose its own exit code independently.
pub async fn reconcile_shutdown_cleanup(
    mode: DatabaseMode, exclude: &HashSet<i64>, deadline: Duration,
) -> (Vec<i64>, Result<(), String>) {
    let reconciled_owed = reconcile_pending_cleanup(mode, deadline, exclude).await;
    let mut state_cleanup_ids: HashSet<i64> = reconciled_owed.iter().copied().collect();
    state_cleanup_ids.extend(exclude.iter().copied());
    let state_cleanup_ids: Vec<i64> = state_cleanup_ids.into_iter().collect();
    let cleanup_result =
        cleanup_current_process_config_states_with_mode(mode, &state_cleanup_ids).await;
    (reconciled_owed, cleanup_result)
}

pub async fn start_port_forward(
    config_id: i64, mode: DatabaseMode, ssl_override: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = get_config_with_mode(config_id, mode).await?;

    start_port_forwarding_with_ssl(config, mode, ssl_override)
        .await
        .map_err(|error| Box::new(std::io::Error::other(error)) as Box<dyn std::error::Error>)
}

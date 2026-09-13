use std::io::Write;

use crossterm::{
    cursor::Show,
    execute,
    terminal::{
        LeaveAlternateScreen,
        disable_raw_mode,
    },
};
use kftray_commons::models::config_model::Config;
use kftray_commons::utils::config::get_config_with_mode;
use kftray_commons::utils::config_state::cleanup_current_process_config_states_with_mode;
use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_portforward::kube::{
    deploy_and_forward_pod_with_mode,
    start_port_forward_with_mode as kube_start_port_forward,
    stop_all_port_forward_with_mode,
    stop_port_forward_with_mode,
};
use log::error;

use crate::tui::input::App;

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
        Some("service") | Some("pod") => match config.protocol.as_str() {
            "tcp" => kube_start_port_forward(vec![config.clone()], "tcp", mode, ssl_override).await,
            "udp" => {
                deploy_and_forward_pod_with_mode(vec![config.clone()], mode, ssl_override).await
            }
            protocol => return Err(format!("Unsupported protocol: {protocol}")),
        },
        workload => return Err(format!("Unsupported workload type: {workload:?}")),
    };

    let responses = match result {
        Ok(responses) => responses,
        Err(e) => {
            error!("Failed to start port forward: {e:?}");
            return Err(format!("Failed to start port forward: {e:?}"));
        }
    };
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

pub async fn stop_all_port_forward_and_exit(app: &mut App, mode: DatabaseMode) {
    log::debug!("Stopping all port forwards in mode: {mode:?}...");

    // Restore the terminal first: draining in-flight operations and deleting
    // cluster resources are both unbounded from here, and neither should hold
    // the shell in raw mode.
    let _ = disable_raw_mode();
    let _ = execute!(std::io::stdout(), LeaveAlternateScreen, Show);
    let _ = std::io::stdout().flush();

    app.finish_forwarding().await;
    // Reported on stderr and through the exit code: the alternate screen is
    // already gone by the time this runs, so nothing drawn here would be seen.
    let mut failed = false;
    match tokio::time::timeout(
        crate::tui::app::CLEANUP_RECONCILE_TIMEOUT,
        stop_all_port_forward_with_mode(mode),
    )
    .await
    .unwrap_or_else(|_| Err("shutdown budget elapsed".to_owned()))
    {
        Ok(responses) => {
            for response in responses {
                if response.status != 0 {
                    error!("Error stopping port forward: {:?}", response.stderr);
                    // The terminal is already restored and this function always
                    // exits, so the popup would never be drawn.
                    eprintln!("Error stopping port forward: {}", response.stderr);
                    failed = true;
                }
            }
        }
        Err(e) => {
            error!("Failed to stop all port forwards: {e}");
            eprintln!("Failed to stop all port forwards: {e}");
            failed = true;
        }
    }

    // A create abandoned on the way out can surface after that first pass, and
    // the registry that tracks it lives only in this process.
    kftray_portforward::kube::reconcile_pending_cleanup(
        mode,
        crate::tui::app::CLEANUP_RECONCILE_TIMEOUT,
    )
    .await;

    if let Err(e) = cleanup_current_process_config_states_with_mode(mode).await {
        log::error!("Failed to cleanup config states: {e}");
        eprintln!("Failed to clean up configuration states: {e}");
        failed = true;
    }

    log::debug!("Exiting application...");

    std::process::exit(i32::from(failed));
}

pub async fn start_port_forward(
    config_id: i64, mode: DatabaseMode, ssl_override: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = get_config_with_mode(config_id, mode).await?;

    start_port_forwarding_with_ssl(config, mode, ssl_override)
        .await
        .map_err(|error| Box::new(std::io::Error::other(error)) as Box<dyn std::error::Error>)
}

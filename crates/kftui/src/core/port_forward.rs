use std::io::Write;
use std::sync::Arc;

use crossterm::{
    cursor::Show,
    execute,
    terminal::{
        disable_raw_mode,
        LeaveAlternateScreen,
    },
};
use kftray_commons::models::config_model::Config;
use kftray_commons::models::config_state_model::ConfigState;
use kftray_commons::utils::config::get_config_with_mode;
use kftray_commons::utils::config_state::update_config_state_with_mode;
use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_commons::models::config_state_model::ConfigState;
use kftray_commons::utils::config::get_config_with_mode;
use kftray_commons::utils::config_state::update_config_state_with_mode;
use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_http_logs::HttpLogState;
use kftray_portforward::kube::{
    deploy_and_forward_pod,
    start_port_forward as kube_start_port_forward,
    stop_all_port_forward_with_mode,
    stop_port_forward_with_mode,
    stop_proxy_forward_with_mode,
};
use log::error;

use crate::tui::input::{
    App,
    AppState,
};

pub async fn start_port_forwarding(app: &mut App, config: Config, mode: DatabaseMode) {
    let config_id = config.id.unwrap_or_default();
    let log_state = Arc::new(HttpLogState::new());

    let result = match config.workload_type.as_deref() {
        Some("proxy") => deploy_and_forward_pod(vec![config.clone()], log_state).await,
        Some("service") | Some("pod") => match config.protocol.as_str() {
            "tcp" => kube_start_port_forward(vec![config.clone()], "tcp", log_state).await,
            "udp" => deploy_and_forward_pod(vec![config.clone()], log_state).await,
            _ => return,
        },
        _ => return,
    };

    if let Err(e) = result {
        error!("Failed to start port forward: {e:?}");
        app.error_message = Some(format!("Failed to start port forward: {e:?}"));
        app.state = AppState::ShowErrorPopup;
        return;
    }

    // Update config state on success
    let config_state = ConfigState {
        id: None,
        config_id,
        is_running: true,
    };

    if let Err(e) = update_config_state_with_mode(&config_state, mode).await {
        error!("Failed to update config state: {e}");
    }
}

pub async fn stop_port_forwarding(app: &mut App, config: Config, mode: DatabaseMode) {
    let config_id = config.id.unwrap_or_default();

    let result = match config.workload_type.as_deref() {
        Some("proxy") => {
            stop_proxy_forward_with_mode(
                config_id,
                &config.namespace,
                config.service.clone().unwrap_or_default(),
                mode,
            )
            .await
        }
        Some("service") | Some("pod") => {
            stop_port_forward_with_mode(config_id.to_string(), mode).await
        }
        _ => return,
    };

    if let Err(e) = result {
        error!("Failed to stop port forward: {e:?}");
        app.error_message = Some(format!("Failed to stop port forward: {e:?}"));
        app.state = AppState::ShowErrorPopup;
        return;
    }

    // Update config state on success
    let config_state = ConfigState {
        id: None,
        config_id,
        is_running: false,
    };

    if let Err(e) = update_config_state_with_mode(&config_state, mode).await {
        error!("Failed to update config state: {e}");
    }
}

pub async fn stop_all_port_forward_and_exit(app: &mut App, mode: DatabaseMode) {
    log::debug!("Stopping all port forwards in mode: {mode:?}...");
    match stop_all_port_forward_with_mode(mode).await {
        Ok(responses) => {
            for response in responses {
                if response.status != 0 {
                    error!("Error stopping port forward: {:?}", response.stderr);
                }
            }
        }
        Err(e) => {
            error!("Failed to stop all port forwards: {e:?}");
            app.error_message = Some(format!("Failed to stop all port forwards: {e:?}"));
            app.state = AppState::ShowErrorPopup;
        }
    }
    log::debug!("Exiting application...");

    disable_raw_mode().expect("Failed to disable raw mode");
    execute!(std::io::stdout(), LeaveAlternateScreen, Show)
        .expect("Failed to leave alternate screen and show cursor");
    std::io::stdout().flush().expect("Failed to flush stdout");

    std::process::exit(0);
}

pub async fn start_port_forward(
    config_id: i64, mode: DatabaseMode,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = get_config_with_mode(config_id, mode).await?;
    let log_state = Arc::new(HttpLogState::new());

    match config.workload_type.as_deref() {
        Some("proxy") => {
            deploy_and_forward_pod(vec![config], log_state).await?;
        }
        Some("service") | Some("pod") => match config.protocol.as_str() {
            "tcp" => {
                kube_start_port_forward(vec![config], "tcp", log_state).await?;
            }
            "udp" => {
                deploy_and_forward_pod(vec![config], log_state).await?;
            }
            _ => return Ok(()),
        },
        _ => return Ok(()),
    }

    // Update config state (non-fatal if it fails)
    let config_state = ConfigState {
        id: None,
        config_id,
        is_running: true,
    };

    if let Err(e) = update_config_state_with_mode(&config_state, mode).await {
        error!("Failed to update config state: {e}");
    }

    Ok(())
}

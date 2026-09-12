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

use crate::tui::input::{
    App,
    AppState,
};

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
    let failures: Vec<String> = responses
        .into_iter()
        .filter(|response| response.status != 0)
        .map(|response| response.stderr)
        .collect();
    if !failures.is_empty() {
        let message = failures.join("; ");
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
    app.finish_forwarding().await;
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

    if let Err(e) = cleanup_current_process_config_states_with_mode(mode).await {
        log::error!("Failed to cleanup config states: {e}");
    }

    log::debug!("Exiting application...");

    disable_raw_mode().expect("Failed to disable raw mode");
    execute!(std::io::stdout(), LeaveAlternateScreen, Show)
        .expect("Failed to leave alternate screen and show cursor");
    std::io::stdout().flush().expect("Failed to flush stdout");

    std::process::exit(0);
}

pub async fn start_port_forward(
    config_id: i64, mode: DatabaseMode, ssl_override: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = get_config_with_mode(config_id, mode).await?;

    start_port_forwarding_with_ssl(config, mode, ssl_override)
        .await
        .map_err(|error| Box::new(std::io::Error::other(error)) as Box<dyn std::error::Error>)
}

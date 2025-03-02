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
use kftray_http_logs::HttpLogState;
use kftray_portforward::kube::stop_all_port_forward;
use kftray_portforward::kube::{
    deploy_and_forward_pod,
    start_port_forward,
    stop_port_forward,
    stop_proxy_forward,
};
use log::error;

use crate::tui::input::{
    App,
    AppState,
};

pub async fn start_port_forwarding(app: &mut App, config: Config) {
    match config.workload_type.as_deref() {
        Some("proxy") => {
            if let Err(e) =
                deploy_and_forward_pod(vec![config.clone()], Arc::new(HttpLogState::new())).await
            {
                error!("Failed to start proxy forward: {:?}", e);
                app.error_message = Some(format!("Failed to start proxy forward: {:?}", e));
                app.state = AppState::ShowErrorPopup;
            }
        }
        Some("service") | Some("pod") => match config.protocol.as_str() {
            "tcp" => {
                let log_state = Arc::new(HttpLogState::new());
                let result = start_port_forward(vec![config.clone()], "tcp", log_state).await;
                if let Err(e) = result {
                    error!("Failed to start TCP port forward: {:?}", e);
                    app.error_message = Some(format!("Failed to start TCP port forward: {:?}", e));
                    app.state = AppState::ShowErrorPopup;
                }
            }
            "udp" => {
                let result =
                    deploy_and_forward_pod(vec![config.clone()], Arc::new(HttpLogState::new()))
                        .await;
                if let Err(e) = result {
                    error!("Failed to start UDP port forward: {:?}", e);
                    app.error_message = Some(format!("Failed to start UDP port forward: {:?}", e));
                    app.state = AppState::ShowErrorPopup;
                }
            }
            _ => {}
        },
        _ => {}
    }
}

pub async fn stop_port_forwarding(app: &mut App, config: Config) {
    match config.workload_type.as_deref() {
        Some("proxy") => {
            if let Err(e) = stop_proxy_forward(
                config.id.unwrap_or_default(),
                &config.namespace,
                config.service.clone().unwrap_or_default(),
            )
            .await
            {
                error!("Failed to stop proxy forward: {:?}", e);
                app.error_message = Some(format!("Failed to stop proxy forward: {:?}", e));
                app.state = AppState::ShowErrorPopup;
            }
        }
        Some("service") | Some("pod") => {
            if let Err(e) = stop_port_forward(config.id.unwrap_or_default().to_string()).await {
                error!("Failed to stop port forward: {:?}", e);
                app.error_message = Some(format!("Failed to stop port forward: {:?}", e));
                app.state = AppState::ShowErrorPopup;
            }
        }
        _ => {}
    }
}

pub async fn stop_all_port_forward_and_exit(app: &mut App) {
    log::debug!("Stopping all port forwards...");
    match stop_all_port_forward().await {
        Ok(responses) => {
            for response in responses {
                if response.status != 0 {
                    error!("Error stopping port forward: {:?}", response.stderr);
                }
            }
        }
        Err(e) => {
            error!("Failed to stop all port forwards: {:?}", e);
            app.error_message = Some(format!("Failed to stop all port forwards: {:?}", e));
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

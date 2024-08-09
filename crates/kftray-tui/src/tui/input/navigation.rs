use std::sync::Arc;

use kftray_commons::models::config_model::Config;
use kftray_portforward::core::{
    deploy_and_forward_pod,
    start_port_forward,
    stop_port_forward,
    stop_proxy_forward,
};
use kftray_portforward::models::kube::HttpLogState;
use log::error;

use crate::tui::input::App;

pub async fn start_port_forwarding(app: &mut App, config: Config) {
    match config.workload_type.as_str() {
        "proxy" => {
            if let Err(e) =
                deploy_and_forward_pod(vec![config.clone()], Arc::new(HttpLogState::new())).await
            {
                error!("Failed to start proxy forward: {:?}", e);
                app.error_message = Some(format!("Failed to start proxy forward: {:?}", e));
                app.show_error_popup = true;
            }
        }
        "service" | "pod" => match config.protocol.as_str() {
            "tcp" => {
                let log_state = Arc::new(HttpLogState::new());
                let result = start_port_forward(vec![config.clone()], "tcp", log_state).await;
                if let Err(e) = result {
                    error!("Failed to start TCP port forward: {:?}", e);
                    app.error_message = Some(format!("Failed to start TCP port forward: {:?}", e));
                    app.show_error_popup = true;
                }
            }
            "udp" => {
                let result =
                    deploy_and_forward_pod(vec![config.clone()], Arc::new(HttpLogState::new()))
                        .await;
                if let Err(e) = result {
                    error!("Failed to start UDP port forward: {:?}", e);
                    app.error_message = Some(format!("Failed to start UDP port forward: {:?}", e));
                    app.show_error_popup = true;
                }
            }
            _ => {}
        },
        _ => {}
    }
}

pub async fn stop_port_forwarding(app: &mut App, config: Config) {
    match config.workload_type.as_str() {
        "proxy" => {
            if let Err(e) = stop_proxy_forward(
                config.id.unwrap_or_default().to_string(),
                &config.namespace,
                config.service.clone().unwrap_or_default(),
            )
            .await
            {
                error!("Failed to stop proxy forward: {:?}", e);
                app.error_message = Some(format!("Failed to stop proxy forward: {:?}", e));
                app.show_error_popup = true;
            }
        }
        "service" | "pod" => {
            if let Err(e) = stop_port_forward(config.id.unwrap_or_default().to_string()).await {
                error!("Failed to stop port forward: {:?}", e);
                app.error_message = Some(format!("Failed to stop port forward: {:?}", e));
                app.show_error_popup = true;
            }
        }
        _ => {}
    }
}

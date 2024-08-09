use std::sync::Arc;

use crossterm::event::KeyCode;
use kftray_commons::models::config_model::Config;
use kftray_commons::models::config_state_model::ConfigState;
use kftray_portforward::core::{
    deploy_and_forward_pod,
    start_port_forward,
    stop_port_forward,
    stop_proxy_forward,
};
use kftray_portforward::models::kube::HttpLogState;
use log::{
    error,
    info,
};

use crate::tui::input::{
    ActiveTable,
    App,
};

pub async fn handle_navigation_input(
    app: &mut App, key: KeyCode, config_states: &mut [ConfigState],
) -> Result<(), std::io::Error> {
    match key {
        KeyCode::Char('s') => {
            app.show_search = true;
        }
        KeyCode::Char('f') => {
            let (selected_rows, configs) = match app.active_table {
                ActiveTable::Stopped => (&app.selected_rows_stopped, &app.stopped_configs),
                ActiveTable::Running => (&app.selected_rows_running, &app.running_configs),
            };

            let selected_configs: Vec<Config> = selected_rows
                .iter()
                .filter_map(|&row| configs.get(row).cloned())
                .collect();

            if app.active_table == ActiveTable::Stopped {
                for config in selected_configs.clone() {
                    if let Some(state) = config_states
                        .iter_mut()
                        .find(|s| s.config_id == config.id.unwrap_or_default())
                    {
                        if !state.is_running {
                            info!("Starting port forward for config: {:?}", config);
                            start_port_forwarding(app, config.clone()).await;
                            state.is_running = true;
                        }
                    }
                }
            } else if app.active_table == ActiveTable::Running {
                for config in selected_configs.clone() {
                    if let Some(state) = config_states
                        .iter_mut()
                        .find(|s| s.config_id == config.id.unwrap_or_default())
                    {
                        if state.is_running {
                            info!("Stopping port forward for config: {:?}", config);
                            stop_port_forwarding(app, config.clone()).await;
                            state.is_running = false;
                        }
                    }
                }
            }

            if app.active_table == ActiveTable::Stopped {
                app.running_configs.extend(selected_configs.clone());
                app.stopped_configs
                    .retain(|config| !selected_configs.contains(config));
            } else {
                app.stopped_configs.extend(selected_configs.clone());
                app.running_configs
                    .retain(|config| !selected_configs.contains(config));
            }

            match app.active_table {
                ActiveTable::Stopped => app.selected_rows_stopped.clear(),
                ActiveTable::Running => app.selected_rows_running.clear(),
            }
        }

        KeyCode::Down => match app.active_table {
            ActiveTable::Stopped => {
                if !app.stopped_configs.is_empty() {
                    app.selected_row_stopped =
                        (app.selected_row_stopped + 1) % app.stopped_configs.len();
                }
            }
            ActiveTable::Running => {
                if !app.running_configs.is_empty() {
                    app.selected_row_running =
                        (app.selected_row_running + 1) % app.running_configs.len();
                }
            }
        },
        KeyCode::Up => match app.active_table {
            ActiveTable::Stopped => {
                if !app.stopped_configs.is_empty() {
                    app.selected_row_stopped = if app.selected_row_stopped == 0 {
                        app.stopped_configs.len() - 1
                    } else {
                        app.selected_row_stopped - 1
                    };
                }
            }
            ActiveTable::Running => {
                if !app.running_configs.is_empty() {
                    app.selected_row_running = if app.selected_row_running == 0 {
                        app.running_configs.len() - 1
                    } else {
                        app.selected_row_running - 1
                    };
                }
            }
        },
        KeyCode::Right => {
            app.active_table = ActiveTable::Running;
            app.selected_rows_stopped.clear();
        }
        KeyCode::Left => {
            app.active_table = ActiveTable::Stopped;
            app.selected_rows_running.clear();
        }
        KeyCode::Char(' ') => {
            let selected_row = match app.active_table {
                ActiveTable::Stopped => app.selected_row_stopped,
                ActiveTable::Running => app.selected_row_running,
            };

            let selected_rows = match app.active_table {
                ActiveTable::Stopped => &mut app.selected_rows_stopped,
                ActiveTable::Running => &mut app.selected_rows_running,
            };

            if selected_rows.contains(&selected_row) {
                selected_rows.remove(&selected_row);
            } else {
                selected_rows.insert(selected_row);
            }
        }
        KeyCode::Char('c') => {
            let mut stdout_output = app.stdout_output.lock().unwrap();
            stdout_output.clear();
        }
        _ => {}
    }
    Ok(())
}

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
                info!(
                    "Attempting to start TCP port forward for config: {:?}",
                    config
                );
                let log_state = Arc::new(HttpLogState::new());
                let result = start_port_forward(vec![config.clone()], "tcp", log_state).await;
                info!("Result: {:?}", result);
                if let Err(e) = result {
                    error!("Failed to start TCP port forward: {:?}", e);
                    app.error_message = Some(format!("Failed to start TCP port forward: {:?}", e));
                    app.show_error_popup = true;
                } else {
                    info!(
                        "TCP port forward started successfully for config: {:?}",
                        config
                    );
                }
            }
            "udp" => {
                info!(
                    "Attempting to start UDP port forward for config: {:?}",
                    config
                );
                let result =
                    deploy_and_forward_pod(vec![config.clone()], Arc::new(HttpLogState::new()))
                        .await;
                info!("Result: {:?}", result);
                if let Err(e) = result {
                    error!("Failed to start UDP port forward: {:?}", e);
                    app.error_message = Some(format!("Failed to start UDP port forward: {:?}", e));
                    app.show_error_popup = true;
                } else {
                    info!(
                        "UDP port forward started successfully for config: {:?}",
                        config
                    );
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

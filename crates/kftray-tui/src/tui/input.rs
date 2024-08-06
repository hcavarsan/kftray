use std::io;
use std::sync::Arc;

use crossterm::event::{
    self,
    Event,
    KeyCode,
    KeyModifiers,
};
use kftray_tauri::kubeforward::core::{
    deploy_and_forward_pod,
    start_port_forward,
    stop_port_forward,
    stop_proxy_forward,
};
use kftray_tauri::models::config::Config;
use kftray_tauri::models::config_state::ConfigState;

/// Handles user input.
pub async fn handle_input(
    selected_row: &mut usize, show_details: &mut bool, config_len: usize, configs: &[Config],
    config_states: &mut [ConfigState],
) -> io::Result<bool> {
    if event::poll(std::time::Duration::from_millis(100))? {
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') | KeyCode::Char('c') => {
                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        println!("Detected Ctrl+Q or Ctrl+C. Exiting...");
                        return Ok(true);
                    }
                }
                KeyCode::Down => {
                    *selected_row = (*selected_row + 1) % config_len;
                }
                KeyCode::Up => {
                    *selected_row = if *selected_row == 0 {
                        config_len - 1
                    } else {
                        *selected_row - 1
                    };
                }
                KeyCode::Enter => {
                    *show_details = !*show_details;
                }
                KeyCode::Char('f') => {
                    if let Some(config) = configs.get(*selected_row) {
                        if let Some(state) = config_states
                            .iter_mut()
                            .find(|s| s.config_id == config.id.unwrap_or_default())
                        {
                            if state.is_running {
                                match config.workload_type.as_str() {
                                    "proxy" => {
                                        stop_proxy_forward(
                                            config.id.unwrap_or_default().to_string(),
                                            &config.namespace,
                                            config.service.clone().unwrap_or_default(),
                                        )
                                        .await
                                        .unwrap();
                                    }
                                    "service" | "pod" => {
                                        stop_port_forward(
                                            config.id.unwrap_or_default().to_string(),
                                        )
                                        .await
                                        .unwrap();
                                    }
                                    _ => {}
                                }
                                state.is_running = false;
                            } else {
                                match config.workload_type.as_str() {
                                    "proxy" => {
                                        deploy_and_forward_pod(
                                            vec![config.clone()],
                                            Arc::new(Default::default()),
                                        )
                                        .await
                                        .unwrap();
                                    }
                                    "service" | "pod" => match config.protocol.as_str() {
                                        "tcp" => {
                                            start_port_forward(
                                                vec![config.clone()],
                                                "tcp",
                                                Arc::new(Default::default()),
                                            )
                                            .await
                                            .unwrap();
                                        }
                                        "udp" => {
                                            deploy_and_forward_pod(
                                                vec![config.clone()],
                                                Arc::new(Default::default()),
                                            )
                                            .await
                                            .unwrap();
                                        }
                                        _ => {}
                                    },
                                    _ => {}
                                }
                                state.is_running = true;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(false)
}

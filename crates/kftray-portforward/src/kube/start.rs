use std::sync::Arc;

use hostsfile::HostsBuilder;
use kftray_commons::{
    models::{
        config_model::Config,
        config_state_model::ConfigState,
        response::CustomResponse,
    },
    utils::config_state::update_config_state,
};
use kftray_http_logs::HttpLogState;
use log::{
    debug,
    error,
    info,
    warn,
};

use crate::{
    kube::models::{
        Port,
        PortForward,
        Target,
        TargetSelector,
    },
    port_forward::CHILD_PROCESSES,
};

pub async fn start_port_forward(
    configs: Vec<Config>, protocol: &str, http_log_state: Arc<HttpLogState>,
) -> Result<Vec<CustomResponse>, String> {
    let mut responses = Vec::new();
    let mut errors = Vec::new();
    let mut child_handles = Vec::new();

    for config in configs.iter() {
        let selector = match config.workload_type.as_deref() {
            Some("pod") => TargetSelector::PodLabel(config.target.clone().unwrap_or_default()),
            _ => TargetSelector::ServiceName(config.service.clone().unwrap_or_default()),
        };

        let remote_port = Port::from(config.remote_port.unwrap_or_default() as i32);
        let context_name = Some(config.context.clone());
        let kubeconfig = Some(config.kubeconfig.clone());
        let namespace = config.namespace.clone();
        let target = Target::new(selector, remote_port, namespace.clone());

        debug!("Remote Port: {:?}", config.remote_port);
        debug!("Local Port: {:?}", config.local_port);

        if config.workload_type.as_deref() == Some("pod") {
            info!("Attempting to forward to pod label: {:?}", &config.target);
        } else {
            info!("Attempting to forward to service: {:?}", &config.service);
        }

        let local_address_clone = config.local_address.clone();

        let port_forward_result: Result<PortForward, anyhow::Error> = PortForward::new(
            target,
            config.local_port,
            local_address_clone,
            context_name,
            kubeconfig.flatten(),
            config.id.unwrap_or_default(),
            config.workload_type.clone().unwrap_or_default(),
        )
        .await;

        match port_forward_result {
            Ok(port_forward) => {
                let forward_result = match protocol {
                    "udp" => port_forward.clone().port_forward_udp().await,
                    "tcp" => {
                        port_forward
                            .clone()
                            .port_forward_tcp(http_log_state.clone())
                            .await
                    }
                    _ => Err(anyhow::anyhow!("Unsupported protocol")),
                };

                match forward_result {
                    Ok((actual_local_port, handle)) => {
                        info!(
                            "{} port forwarding is set up on local port: {:?} for {}: {:?}",
                            protocol.to_uppercase(),
                            actual_local_port,
                            if config.workload_type.as_deref() == Some("pod") {
                                "pod label"
                            } else {
                                "service"
                            },
                            &config.service
                        );

                        debug!("Port forwarding details: {:?}", port_forward);
                        debug!("Actual local port: {:?}", actual_local_port);

                        let handle_key = format!(
                            "{}_{}",
                            config.id.unwrap(),
                            config.service.clone().unwrap_or_default()
                        );
                        CHILD_PROCESSES
                            .lock()
                            .unwrap()
                            .insert(handle_key.clone(), handle);
                        child_handles.push(handle_key.clone());

                        if config.domain_enabled.unwrap_or_default() {
                            let hostfile_comment = format!(
                                "kftray custom host for {} - {}",
                                config.service.clone().unwrap_or_default(),
                                config.id.unwrap_or_default()
                            );

                            let mut hosts_builder = HostsBuilder::new(hostfile_comment);

                            if let Some(service_name) = &config.service {
                                if let Some(local_address) = &config.local_address {
                                    match local_address.parse::<std::net::IpAddr>() {
                                        Ok(ip_addr) => {
                                            hosts_builder.add_hostname(
                                                ip_addr,
                                                config.alias.clone().unwrap_or_default(),
                                            );
                                            if let Err(e) = hosts_builder.write() {
                                                let error_message = format!(
                                                    "Failed to write to the hostfile for {}: {}",
                                                    service_name, e
                                                );
                                                error!("{}", &error_message);
                                                errors.push(error_message);

                                                if let Some(handle) = CHILD_PROCESSES
                                                    .lock()
                                                    .unwrap()
                                                    .remove(&handle_key)
                                                {
                                                    handle.abort();
                                                }
                                                continue;
                                            }
                                        }
                                        Err(_) => {
                                            let warning_message = format!(
                                                "Invalid IP address format: {}",
                                                local_address
                                            );
                                            warn!("{}", &warning_message);
                                            errors.push(warning_message);
                                        }
                                    }
                                }
                            }
                        }

                        let config_state = ConfigState {
                            id: None,
                            config_id: config.id.unwrap(),
                            is_running: true,
                        };
                        if let Err(e) = update_config_state(&config_state).await {
                            error!("Failed to update config state: {}", e);
                        }

                        responses.push(CustomResponse {
                            id: config.id,
                            service: config.service.clone().unwrap(),
                            namespace: namespace.clone(),
                            local_port: actual_local_port,
                            remote_port: config.remote_port.unwrap_or_default(),
                            context: config.context.clone(),
                            protocol: config.protocol.clone(),
                            stdout: format!(
                                "{} forwarding from 127.0.0.1:{} -> {:?}:{}",
                                protocol.to_uppercase(),
                                actual_local_port,
                                config.remote_port.unwrap_or_default(),
                                config.service.clone().unwrap()
                            ),
                            stderr: String::new(),
                            status: 0,
                        });
                    }
                    Err(e) => {
                        let error_message = format!(
                            "Failed to start {} port forwarding for {} {}: {}",
                            protocol.to_uppercase(),
                            if config.workload_type.as_deref() == Some("pod") {
                                "pod label"
                            } else {
                                "service"
                            },
                            config.service.clone().unwrap_or_default(),
                            e
                        );
                        error!("{}", &error_message);
                        errors.push(error_message);
                    }
                }
            }
            Err(e) => {
                let error_message = format!(
                    "Failed to create PortForward for {} {}: {}",
                    if config.workload_type.as_deref() == Some("pod") {
                        "pod label"
                    } else {
                        "service"
                    },
                    config.service.clone().unwrap_or_default(),
                    e
                );
                error!("{}", &error_message);
                errors.push(error_message);
            }
        }
    }

    if !errors.is_empty() {
        for handle_key in child_handles {
            if let Some(handle) = CHILD_PROCESSES.lock().unwrap().remove(&handle_key) {
                handle.abort();
            }
        }
        return Err(errors.join("\n"));
    }

    Ok(responses)
}

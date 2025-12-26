pub mod kubernetes;
pub mod models;
pub mod templates;
pub mod websocket_client;

use kftray_commons::models::{
    config_model::Config,
    config_state_model::ConfigState,
    response::CustomResponse,
};
use kftray_commons::utils::config_state::update_config_state_with_mode;
use kftray_commons::utils::db_mode::DatabaseMode;
use log::{
    error,
    info,
};

use crate::kube::shared_client::{
    SHARED_CLIENT_MANAGER,
    ServiceClientKey,
};

/// Start expose for given configs
pub async fn start_expose(
    configs: Vec<Config>, mode: DatabaseMode,
) -> Result<Vec<CustomResponse>, String> {
    let mut responses = Vec::new();

    for config in configs {
        match start_single_expose(config, mode).await {
            Ok(response) => responses.push(response),
            Err(e) => return Err(e),
        }
    }

    Ok(responses)
}

async fn start_single_expose(config: Config, mode: DatabaseMode) -> Result<CustomResponse, String> {
    use self::kubernetes::create_expose_resources;
    use self::websocket_client::WebSocketTunnelClient;
    use crate::kube::models::{
        NameSpace,
        Port,
        PortForward,
        Target,
        TargetSelector,
    };
    use crate::port_forward::CHILD_PROCESSES;

    let config_id = config.id.ok_or("Config has no ID")?;

    let client_key = ServiceClientKey::new(config.context.clone(), config.kubeconfig.clone());
    let client = SHARED_CLIENT_MANAGER
        .get_client(client_key)
        .await
        .map_err(|e| format!("Failed to get K8s client: {}", e))?;
    let client = (*client).clone();

    info!("Creating expose resources for config {}", config_id);
    let resources = create_expose_resources(client.clone(), &config).await?;

    info!(
        "Resources created: deployment={}, service={}, pod={}",
        resources.deployment_name, resources.service_name, resources.pod_name
    );

    let label_selector = format!("app=kftray-expose,config_id={}", config_id);
    let target = Target {
        selector: TargetSelector::PodLabel(label_selector),
        port: Port::Number(9999),
        namespace: NameSpace(Some(config.namespace.clone())),
    };

    let port_forward = PortForward::new(
        target,
        Some(0),
        None,
        config.context.clone(),
        config.kubeconfig.clone(),
        config_id,
        "expose".to_string(),
    )
    .await
    .map_err(|e| format!("Failed to create port-forward: {}", e))?;

    let (websocket_port, pf_process) = port_forward
        .port_forward_tcp(None)
        .await
        .map_err(|e| format!("Failed to start port-forward: {}", e))?;

    info!(
        "Port-forward established: localhost:{} → pod:9999",
        websocket_port
    );

    CHILD_PROCESSES.insert(config_id.to_string(), pf_process);

    let local_service_port = config.local_port.unwrap_or(8080);
    let local_service_address = config
        .local_address
        .clone()
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let ws_client = WebSocketTunnelClient::new(
        websocket_port,
        local_service_address.clone(),
        local_service_port,
    );

    info!(
        "Starting WebSocket tunnel: pod → localhost:{} → {}:{}",
        websocket_port, local_service_address, local_service_port
    );

    let ws_handle = tokio::spawn(async move {
        if let Err(e) = ws_client.start().await {
            error!("WebSocket client error: {}", e);
        }
    });

    // Store the WebSocket client handle so it can be aborted when stopping
    if let Some(mut process) = CHILD_PROCESSES.get_mut(&config_id.to_string()) {
        process.set_ws_client_handle(ws_handle);
    }

    let config_state = ConfigState {
        id: None,
        config_id,
        is_running: true,
        process_id: Some(std::process::id()),
    };
    update_config_state_with_mode(&config_state, mode).await?;

    info!("Expose tunnel fully established for config {}", config_id);

    Ok(CustomResponse {
        id: Some(config_id),
        service: config.service.unwrap_or(resources.service_name),
        namespace: config.namespace.clone(),
        local_port: local_service_port,
        remote_port: 9999,
        context: config.context.unwrap_or_default(),
        stdout: String::new(),
        stderr: String::new(),
        status: 0,
        protocol: "tcp".to_string(),
    })
}

pub async fn stop_expose(
    config_id: i64, namespace: &str, mode: DatabaseMode,
) -> Result<CustomResponse, String> {
    use kftray_commons::utils::config::get_config_with_mode;

    use self::kubernetes::delete_expose_resources;
    use crate::port_forward::CHILD_PROCESSES;

    info!("Stopping expose for config {}", config_id);

    let config = get_config_with_mode(config_id, mode).await?;

    if let Some((_, pf_process)) = CHILD_PROCESSES.remove(&config_id.to_string()) {
        info!("Cleaning up port-forward for config {}", config_id);
        pf_process.cleanup_and_abort().await;
    }

    let client_key = ServiceClientKey::new(config.context.clone(), config.kubeconfig.clone());
    let client = SHARED_CLIENT_MANAGER
        .get_client(client_key)
        .await
        .map_err(|e| format!("Failed to get K8s client: {}", e))?;
    let client = (*client).clone();

    delete_expose_resources(client, namespace, &config_id.to_string()).await?;

    let config_state = ConfigState {
        id: None,
        config_id,
        is_running: false,
        process_id: None,
    };
    update_config_state_with_mode(&config_state, mode).await?;

    info!("Expose stopped for config {}", config_id);

    Ok(CustomResponse {
        id: Some(config_id),
        service: config.service.unwrap_or_else(|| "expose".to_string()),
        namespace: config.namespace.clone(),
        local_port: config.local_port.unwrap_or(0),
        remote_port: config.remote_port.unwrap_or(0),
        context: config.context.unwrap_or_default(),
        stdout: String::new(),
        stderr: String::new(),
        status: 0,
        protocol: config.protocol.clone(),
    })
}

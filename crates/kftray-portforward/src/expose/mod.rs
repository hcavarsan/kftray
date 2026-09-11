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

use crate::expose::kubernetes::delete_expose_resources;
use crate::kube::shared_client::{
    SHARED_CLIENT_MANAGER,
    ServiceClientKey,
};

/// Start expose for given configs
pub async fn start_expose(
    configs: Vec<Config>, mode: DatabaseMode,
) -> Result<Vec<CustomResponse>, String> {
    let configs = configs
        .into_iter()
        .map(|mut config| {
            config.workload_type = Some("expose".to_string());
            config
        })
        .collect();
    crate::kube::start_port_forward_with_mode(configs, "tcp", mode, false).await
}

pub(crate) async fn start_single_expose(
    config: Config, mode: DatabaseMode,
) -> Result<CustomResponse, String> {
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
        .get_connection(client_key)
        .await
        .map_err(|e| format!("Failed to get K8s client: {}", e))?;
    let client = client.client.clone();

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
    );

    let (websocket_port, mut pf_process) = match port_forward.port_forward_tcp(None).await {
        Ok(started) => started,
        Err(error) => {
            let reason = format!("Failed to start port-forward: {error}");
            return match delete_expose_resources(
                client.clone(),
                &config.namespace,
                &config_id.to_string(),
            )
            .await
            {
                Ok(()) => Err(reason),
                Err(cleanup_error) => Err(format!("{reason}; cleanup failed: {cleanup_error}")),
            };
        }
    };

    info!(
        "Port-forward established: localhost:{} → pod:9999",
        websocket_port
    );

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

    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let ws_handle = tokio::spawn(async move {
        if let Err(e) = ws_client.start(ready_tx).await {
            error!("WebSocket client error: {}", e);
        }
    });

    let startup = match tokio::time::timeout(std::time::Duration::from_secs(30), ready_rx).await {
        Ok(Ok(result)) => result,
        Ok(Err(error)) => Err(format!(
            "Expose startup task ended before connecting: {error}"
        )),
        Err(_) => Err("Timed out connecting the reverse WebSocket tunnel".to_owned()),
    };
    if let Err(error) = startup {
        ws_handle.abort();
        pf_process.cleanup_and_abort().await;
        return match delete_expose_resources(
            client.clone(),
            &config.namespace,
            &config_id.to_string(),
        )
        .await
        {
            Ok(()) => Err(error),
            Err(cleanup_error) => Err(format!("{error}; cleanup failed: {cleanup_error}")),
        };
    }
    pf_process.set_ws_client_handle(ws_handle);

    let config_state = ConfigState {
        id: None,
        config_id,
        is_running: true,
        process_id: Some(std::process::id()),
        is_retrying: false,
        retry_count: None,
        last_error: None,
    };
    if let Err(error) = update_config_state_with_mode(&config_state, mode).await {
        pf_process.cleanup_and_abort().await;
        let _ = delete_expose_resources(client, &config.namespace, &config_id.to_string()).await;
        return Err(error);
    }
    CHILD_PROCESSES.insert(config_id, pf_process);

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
    config_id: i64, _namespace: &str, mode: DatabaseMode,
) -> Result<CustomResponse, String> {
    crate::kube::stop_port_forward_with_mode(config_id.to_string(), mode).await
}

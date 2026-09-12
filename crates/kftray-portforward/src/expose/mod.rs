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
use tokio_util::sync::CancellationToken;

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
    config: Config, mode: DatabaseMode, cancellation: Option<&CancellationToken>,
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
    // Startup holds the per-config lifecycle lock, and a stop waits on that
    // same lock, so without observing cancellation here a stop would block for
    // the full readiness budget.
    let cancelled = || cancellation.is_some_and(CancellationToken::is_cancelled);
    if cancelled() {
        return Err(format!("Expose startup cancelled for config {config_id}"));
    }

    let client_key = ServiceClientKey::new(config.context.clone(), config.kubeconfig.clone());
    let client = SHARED_CLIENT_MANAGER
        .get_connection(client_key)
        .await
        .map_err(|e| format!("Failed to get K8s client: {}", e))?;
    let client = client.client.clone();

    info!("Creating expose resources for config {}", config_id);
    // Armed before creation so a dropped startup future, or a create whose
    // response is lost, still leaves a trail for stop-all.
    let guard = crate::kube::stop::ClusterResourceGuard::arm(config_id, config.clone());
    let created = match cancellation {
        Some(token) => tokio::select! {
            biased;
            _ = token.cancelled() => {
                return Err(format!("Expose startup cancelled for config {config_id}"));
            }
            created = create_expose_resources(client.clone(), &config) => created,
        },
        None => create_expose_resources(client.clone(), &config).await,
    };
    // Confirmed only when the outcome is definitive. A transport failure or a
    // server-side timeout can be answered while the object is still being
    // applied, and rollback cannot name a resource whose create never returned.
    let resources = match created {
        Ok(resources) => {
            guard.confirm();
            resources
        }
        Err(error) => {
            if !error.ambiguous {
                guard.confirm();
            }
            return Err(error.message);
        }
    };

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

    let started = match cancellation {
        Some(token) => tokio::select! {
            biased;
            _ = token.cancelled() => Err(anyhow::anyhow!("startup cancelled")),
            started = port_forward.port_forward_tcp(None) => started,
        },
        None => port_forward.port_forward_tcp(None).await,
    };
    let (websocket_port, mut pf_process) = match started {
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
                Ok(()) => {
                    guard.disarm();
                    Err(reason)
                }
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
    // Attach before awaiting readiness: dropping this startup future must abort
    // the tunnel task through the process, not detach it.
    pf_process.set_ws_client_handle(ws_handle);

    let ready = async {
        match tokio::time::timeout(std::time::Duration::from_secs(30), ready_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => Err(format!(
                "Expose startup task ended before connecting: {error}"
            )),
            Err(_) => Err("Timed out connecting the reverse WebSocket tunnel".to_owned()),
        }
    };
    let startup = match cancellation {
        Some(token) => tokio::select! {
            biased;
            _ = token.cancelled() => Err(format!("Expose startup cancelled for config {config_id}")),
            startup = ready => startup,
        },
        None => ready.await,
    };
    if let Err(error) = startup {
        pf_process.cleanup_and_abort().await;
        return match delete_expose_resources(
            client.clone(),
            &config.namespace,
            &config_id.to_string(),
        )
        .await
        {
            Ok(()) => {
                guard.disarm();
                Err(error)
            }
            Err(cleanup_error) => Err(format!("{error}; cleanup failed: {cleanup_error}")),
        };
    }

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
        if delete_expose_resources(client, &config.namespace, &config_id.to_string())
            .await
            .is_ok()
        {
            guard.disarm();
        }
        return Err(error);
    }
    pf_process.set_config(config.clone());
    CHILD_PROCESSES.insert(config_id, pf_process);
    // Registered: stop can find the process, so the pending trail is redundant.
    guard.disarm();

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

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use http::header;
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::protocol::{
    Role,
    WebSocketConfig,
};
use tokio_util::sync::CancellationToken;
use tungstenite::Message;

use super::allocator::ChannelAllocator;
use crate::error::Error;
use super::keepalive::{
    RecoveryCallback,
    RecoverySignal,
    spawn_keepalive,
};
use super::reader::spawn_reader;
use super::routing::Router;
use super::session::Session;
use crate::connect::{
    KeepaliveConfig,
    build_portforward_request,
};
use crate::subprotocol::Subprotocol;
use crate::version;
use super::writer::writer_task;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn open_session(
    kube_client: &kube::Client, cluster_url: &http::Uri, namespace: &str, pod: &str, port: u16,
    capacity_pairs: usize, _subprotocols: &[Subprotocol], cancel: CancellationToken,
    keepalive_config: KeepaliveConfig, drain_timeout: Duration,
    recovery_callback: RecoveryCallback,
) -> Result<Session, Error> {
    let request = build_portforward_request(cluster_url, namespace, pod, port, capacity_pairs)?;

    let request_uri = request.uri().clone();
    let offered_protocols = request
        .headers()
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("<none>")
        .to_string();
    tracing::debug!(
        uri = %request_uri,
        sec_websocket_protocol = %offered_protocols,
        capacity_pairs,
        "open_session: sending WebSocket upgrade request"
    );

    let t_upgrade = std::time::Instant::now();
    let connection = match kube_client.connect(request).await {
        Ok(c) => c,
        Err(e) => {
            let msg = e.to_string();
            tracing::debug!(error = %msg, "open_session: WebSocket upgrade failed");
            recovery_callback(RecoverySignal::UpgradeFailed {
                status: None,
                message: msg,
            });
            return match version::detect(kube_client).await {
                Ok(info) if !info.supports_ws_portforward() => Err(Error::ServerVersionTooOld {
                    detected: info.git_version,
                    required: "1.30",
                }),
                _ => Err(Error::Kube(e)),
            };
        }
    };

    let negotiated = if connection.supports_stream_close() {
        "v5.channel.k8s.io"
    } else {
        "v4.channel.k8s.io"
    };
    tracing::info!(
        pod = %pod,
        port,
        elapsed_ms = t_upgrade.elapsed().as_millis() as u64,
        negotiated_protocol = negotiated,
        "open_session: kube_client.connect upgrade complete"
    );

    let protocol = if connection.supports_stream_close() {
        Subprotocol::V5
    } else {
        Subprotocol::V4
    };

    let mut join_set: JoinSet<Result<(), Error>> = JoinSet::new();
    let (writer_tx, writer_rx) = mpsc::channel::<Message>(256);

    let keepalive = spawn_keepalive(
        writer_tx.clone(),
        cancel.clone(),
        Arc::clone(&recovery_callback),
        keepalive_config.ping_interval,
        keepalive_config.watchdog_timeout,
        &mut join_set,
    );

    let raw = connection.into_stream().into_inner();
    let mut ws_config = WebSocketConfig::default();
    ws_config.max_message_size = Some(64 * 1024 * 1024);
    ws_config.max_frame_size = Some(16 * 1024 * 1024);
    let ws = WebSocketStream::from_raw_socket(raw, Role::Client, Some(ws_config)).await;
    let (sink, stream) = ws.split();
    join_set.spawn(writer_task(sink, writer_rx, cancel.clone()));
    let router = Router::new();

    for pair_index in 0..capacity_pairs {
        let data_id = (pair_index as u8) * 2;
        let error_id = data_id + 1;
        let (discard_tx_data, _) = mpsc::channel::<bytes::Bytes>(1);
        let (discard_tx_error, _) = mpsc::channel::<bytes::Bytes>(1);
        router.insert(data_id, discard_tx_data, false);
        router.insert(error_id, discard_tx_error, false);
    }

    spawn_reader(
        protocol,
        stream,
        router.clone(),
        writer_tx.clone(),
        cancel.clone(),
        keepalive.clone(),
        Arc::clone(&recovery_callback),
        &mut join_set,
    );

    let allocator = Arc::new(Mutex::new(ChannelAllocator::new(capacity_pairs)));

    Ok(Session::new(
        allocator,
        router,
        writer_tx,
        cancel,
        keepalive,
        protocol,
        join_set,
        drain_timeout,
        recovery_callback,
    ))
}

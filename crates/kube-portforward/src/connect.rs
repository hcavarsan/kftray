use std::time::Duration;

use http::{
    Method,
    Request,
    Uri,
    header,
};
use hyper::upgrade::Upgraded;
use hyper_util::rt::TokioIo;
use kube::client::Body;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::protocol::{
    Role,
    WebSocketConfig,
};

use crate::channel::keepalive::{
    RecoveryCallback,
    RecoverySignal,
};
use crate::error::Error;
use crate::subprotocol::Subprotocol;
use crate::version;

fn name_is_valid(s: &str) -> bool {
    !s.is_empty()
        && s.is_ascii()
        && s.bytes()
            .all(|b| !matches!(b, b'/' | b'?' | b'#') && !b.is_ascii_control())
}

pub(crate) fn build_portforward_request(
    cluster_url: &Uri, namespace: &str, pod: &str, port: u16, capacity_pairs: usize,
) -> Result<Request<Vec<u8>>, Error> {
    if !name_is_valid(namespace) || !name_is_valid(pod) {
        return Err(Error::Configuration(
            "invalid namespace or pod name: contains forbidden character or non-ASCII".into(),
        ));
    }
    if capacity_pairs == 0 {
        return Err(Error::Configuration("capacity_pairs must be > 0".into()));
    }
    let path = format!("/api/v1/namespaces/{namespace}/pods/{pod}/portforward");
    let query = (0..capacity_pairs)
        .map(|_| format!("ports={port}"))
        .collect::<Vec<_>>()
        .join("&");
    let scheme = cluster_url
        .scheme()
        .ok_or_else(|| Error::Configuration("cluster_url is missing scheme".into()))?;
    let authority = cluster_url
        .authority()
        .ok_or_else(|| Error::Configuration("cluster_url is missing authority".into()))?;
    let uri: Uri = format!("{scheme}://{authority}{path}?{query}")
        .parse()
        .map_err(|e: http::uri::InvalidUri| {
            Error::Configuration(format!("invalid port-forward URI: {e}"))
        })?;
    Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header(
            header::SEC_WEBSOCKET_PROTOCOL,
            Subprotocol::offered_header_value(),
        )
        .body(Vec::new())
        .map_err(|e: http::Error| {
            Error::Configuration(format!("failed to build port-forward request: {e}"))
        })
}

pub(crate) struct KeepaliveConfig {
    pub ping_interval: Duration,
    pub watchdog_timeout: Duration,
}

/// Result of a successful WebSocket upgrade for port-forwarding.
pub(crate) struct UpgradedTransport {
    pub ws: WebSocketStream<TokioIo<Upgraded>>,
    pub protocol: Subprotocol,
}

/// Perform the WebSocket upgrade for port-forwarding and return the raw
/// WebSocket stream plus negotiated subprotocol.
///
/// This bypasses `kube::Client::connect()` because kube-rs only supports
/// `v4.channel.k8s.io` and `v5.channel.k8s.io` subprotocols — it rejects
/// `SPDY/3.1+portforward.k8s.io`. We use `kube::Client::send()` to perform
/// a raw HTTP upgrade with our own subprotocol negotiation and validation.
pub(crate) async fn upgrade_portforward(
    kube_client: &kube::Client, cluster_url: &Uri, namespace: &str, pod: &str, port: u16,
    capacity_pairs: usize, recovery_callback: &RecoveryCallback,
) -> Result<UpgradedTransport, Error> {
    let request = build_portforward_request(cluster_url, namespace, pod, port, capacity_pairs)?;

    // Rebuild the request with WebSocket upgrade headers.
    // kube::Client::connect() would do this but it also validates the response
    // against its own hardcoded StreamProtocol enum (V4/V5 only), rejecting
    // SPDY/3.1+portforward.k8s.io. So we do the upgrade manually.
    let (mut parts, body) = request.into_parts();
    parts
        .headers
        .insert(header::CONNECTION, "Upgrade".parse().unwrap());
    parts
        .headers
        .insert(header::UPGRADE, "websocket".parse().unwrap());
    parts
        .headers
        .insert(header::SEC_WEBSOCKET_VERSION, "13".parse().unwrap());
    let key = tokio_tungstenite::tungstenite::handshake::client::generate_key();
    parts
        .headers
        .insert(header::SEC_WEBSOCKET_KEY, key.parse().unwrap());
    // Override the Sec-WebSocket-Protocol with our full offer list
    // (build_portforward_request already sets it, but we re-set after kube
    // would normally override)
    parts.headers.insert(
        header::SEC_WEBSOCKET_PROTOCOL,
        Subprotocol::offered_header_value().parse().unwrap(),
    );

    let request_uri = parts.uri.clone();
    let offered = Subprotocol::offered_header_value();
    tracing::debug!(
        uri = %request_uri,
        sec_websocket_protocol = %offered,
        capacity_pairs,
        "upgrade_portforward: sending WebSocket upgrade request"
    );

    let t_upgrade = std::time::Instant::now();
    let res = match kube_client
        .send(Request::from_parts(parts, Body::from(body)))
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let msg = e.to_string();
            tracing::debug!(error = %msg, "upgrade_portforward: WebSocket upgrade failed");
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

    // Validate 101 Switching Protocols
    if res.status() != http::StatusCode::SWITCHING_PROTOCOLS {
        let status_code = res.status().as_u16();
        let msg = format!("expected 101 Switching Protocols, got {status_code}");
        recovery_callback(RecoverySignal::UpgradeFailed {
            status: Some(status_code),
            message: msg.clone(),
        });
        return Err(Error::UpgradeFailed {
            status: Some(status_code),
            message: msg,
        });
    }

    // Extract the negotiated subprotocol from the response header
    let negotiated_str = res
        .headers()
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let protocol = Subprotocol::from_negotiated(negotiated_str).ok_or_else(|| {
        Error::ProtocolViolation {
            context: "WebSocket upgrade",
            detail: format!(
                "server negotiated unsupported subprotocol: {:?}",
                negotiated_str
            ),
        }
    })?;

    tracing::info!(
        pod = %pod,
        port,
        elapsed_ms = t_upgrade.elapsed().as_millis() as u64,
        negotiated_protocol = %protocol,
        "upgrade_portforward: upgrade complete"
    );

    // Extract the upgraded connection
    let upgraded = hyper::upgrade::on(res)
        .await
        .map_err(|e| Error::Network(format!("failed to complete HTTP upgrade: {e}")))?;

    let mut ws_config = WebSocketConfig::default();
    ws_config.max_message_size = Some(64 * 1024 * 1024);
    ws_config.max_frame_size = Some(16 * 1024 * 1024);
    let ws = WebSocketStream::from_raw_socket(
        TokioIo::new(upgraded),
        Role::Client,
        Some(ws_config),
    )
    .await;

    Ok(UpgradedTransport { ws, protocol })
}

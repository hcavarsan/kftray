use std::sync::Arc;
use std::time::Duration;

use kftray_commons::models::ssh_provider::WORKLOAD_SSH_PORT_FORWARD;
use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_ssh::StreamOpener;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio::task::JoinSet;
use tokio_rustls::TlsAcceptor;
use tokio_util::sync::CancellationToken;
use tracing::{
    debug,
    error,
    warn,
};

use crate::kube::http_log_watcher::HttpLogStateWatcher;
use crate::kube::listener::{
    ConnectionErrors,
    handle_http_redirect,
    is_http_request,
    map_bind_error,
};
use crate::kube::tcp_forwarder::TcpForwarder;

/// Bound for opening one stream; the opener has its own, shorter, bound.
const OPEN_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_ACCEPT_ERRORS: u32 = 10;
const BASE_BACKOFF_MS: u64 = 10;
const MAX_BACKOFF_MS: u64 = 5000;

/// Binds the local TCP listener, configuring the loopback alias first.
pub(crate) async fn bind(address: &str, port: u16) -> anyhow::Result<(TcpListener, u16)> {
    crate::network_utils::ensure_loopback_address(address)
        .await
        .map_err(|e| anyhow::anyhow!("Network config failed: {e}"))?;
    let ip = address
        .parse::<std::net::IpAddr>()
        .map_err(|e| anyhow::anyhow!("Invalid IP address '{address}': {e}"))?;
    let listener = TcpListener::bind(std::net::SocketAddr::new(ip, port))
        .await
        .map_err(|e| map_bind_error(e, address, ip, port))?;
    let port = listener.local_addr()?.port();
    Ok((listener, port))
}

/// Accepts local connections and pipes each one through a stream from
/// `opener`. Every task it starts lives in its own `JoinSet`, so aborting the
/// task running `serve` stops them all.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn serve(
    listener: TcpListener, port: u16, config_id: i64, opener: Arc<dyn StreamOpener>,
    tls: Option<TlsAcceptor>, mode: DatabaseMode, cancel: CancellationToken,
) -> anyhow::Result<()> {
    let mut tasks = JoinSet::new();

    let watcher = HttpLogStateWatcher::new();
    let logging_enabled =
        match kftray_commons::utils::http_logs_config::get_http_logs_config(config_id).await {
            Ok(config) => config.enabled,
            Err(_) => false,
        };
    watcher.set_http_logs(config_id, logging_enabled).await?;
    let sync_watcher = watcher.clone();
    let sync_cancel = cancel.clone();
    tasks.spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) = sync_watcher.sync_from_external_state(config_id).await {
                        debug!("Sync error for config {}: {}", config_id, e);
                    }
                }
                _ = sync_cancel.cancelled() => break,
            }
        }
    });
    let watcher = Arc::new(watcher);

    let forwarder = TcpForwarder::new(config_id, WORKLOAD_SSH_PORT_FORWARD.to_owned());
    let connection_errors = Arc::new(ConnectionErrors::new(config_id, Some(mode)));
    let mut consecutive_accept_errors: u32 = 0;

    loop {
        while tasks.try_join_next().is_some() {}

        let (client, peer) = tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok(connection) => {
                    consecutive_accept_errors = 0;
                    connection
                }
                Err(e) => {
                    consecutive_accept_errors += 1;
                    let exponent = consecutive_accept_errors.saturating_sub(1).min(10);
                    let backoff_ms =
                        (BASE_BACKOFF_MS.saturating_mul(1u64 << exponent)).min(MAX_BACKOFF_MS);
                    error!(
                        "Accept failed ({consecutive_accept_errors}/{MAX_ACCEPT_ERRORS}): {e}, \
                         backing off {backoff_ms}ms"
                    );
                    if consecutive_accept_errors >= MAX_ACCEPT_ERRORS {
                        return Err(anyhow::anyhow!(
                            "TCP listener failed after {MAX_ACCEPT_ERRORS} consecutive accept \
                             errors: {e}"
                        ));
                    }
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_millis(backoff_ms)) => {}
                        _ = cancel.cancelled() => break,
                    }
                    continue;
                }
            },
            _ = cancel.cancelled() => break,
        };

        let opener = Arc::clone(&opener);
        let tls = tls.clone();
        let mut forwarder = forwarder.clone();
        let watcher = Arc::clone(&watcher);
        let connection_errors = Arc::clone(&connection_errors);
        let cancel = cancel.child_token();
        tasks.spawn(async move {
            let mut client = client;
            let tls = match tls {
                Some(_) if is_http_request(&client).await => {
                    if let Err(e) = handle_http_redirect(client, port).await {
                        debug!("Failed to redirect HTTP to HTTPS: {}", e);
                    }
                    return;
                }
                acceptor => acceptor,
            };

            let upstream = match tokio::time::timeout(OPEN_TIMEOUT, opener.open(peer)).await {
                Ok(Ok(stream)) => {
                    connection_errors.succeeded().await;
                    stream
                }
                Ok(Err(e)) => {
                    warn!("Failed to open an SSH channel for {peer}: {e}");
                    let _ = client.shutdown().await;
                    connection_errors.failed(e.to_string()).await;
                    return;
                }
                Err(_) => {
                    let message = "Timed out opening a channel through the SSH session";
                    warn!("{message} for {peer}");
                    let _ = client.shutdown().await;
                    connection_errors.failed(message.to_owned()).await;
                    return;
                }
            };

            if let Some(acceptor) = tls {
                match acceptor.accept(client).await {
                    Ok(tls_stream) => {
                        if let Err(e) = forwarder
                            .forward_tls_streams(tls_stream, upstream, cancel)
                            .await
                        {
                            debug!("TLS forwarding error for {peer}: {e}");
                        }
                    }
                    Err(e) => debug!("TLS handshake failed for {peer}: {e}"),
                }
            } else if let Err(e) = forwarder
                .forward_streams(client, upstream, peer, cancel, watcher, port)
                .await
            {
                debug!("TCP forwarding error for {peer}: {e}");
            }
        });
    }

    tasks.shutdown().await;
    Ok(())
}

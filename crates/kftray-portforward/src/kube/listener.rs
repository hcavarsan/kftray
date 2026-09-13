use std::sync::Arc;

use anyhow::Context;
use httparse::Request;
use k8s_openapi::api::core::v1::Pod;
use kube::Api;
use tokio::io::{
    AsyncReadExt,
    AsyncWriteExt,
};
use tokio::net::{
    TcpListener,
    TcpStream,
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{
    debug,
    error,
    info,
};

use crate::kube::http_log_watcher::HttpLogStateWatcher;
use crate::kube::models::Target;
use crate::kube::shared_client::{
    SHARED_CLIENT_MANAGER,
    ServiceClientKey,
};
use crate::kube::tcp_forwarder::TcpForwarder;
use crate::kube::udp_forwarder::UdpForwarder;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Tcp,
    Udp,
}

#[derive(Clone)]
pub struct ListenerConfig {
    pub local_address: String,
    pub local_port: u16,
    pub protocol: Protocol,
    pub tls_acceptor: Option<tokio_rustls::TlsAcceptor>,
}

impl std::fmt::Debug for ListenerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListenerConfig")
            .field("local_address", &self.local_address)
            .field("local_port", &self.local_port)
            .field("protocol", &self.protocol)
            .field("tls_acceptor", &self.tls_acceptor.is_some())
            .finish()
    }
}

impl Default for ListenerConfig {
    fn default() -> Self {
        Self {
            local_address: "127.0.0.1".to_owned(),
            local_port: 0,
            protocol: Protocol::Tcp,
            tls_acceptor: None,
        }
    }
}

/// A target whose port is given by name, and the last number resolved for the
/// pod it was resolved against.
struct NamedPort {
    target: Target,
    pod_api: Api<Pod>,
    resolved: Arc<std::sync::Mutex<Option<(String, u16)>>>,
}

pub struct PortForwarder {
    namespace: Arc<str>,
    forwarder: Arc<kube_portforward::Forwarder>,
    target_port: u16,
    /// Set for a target whose port is given by name. The number can change when
    /// a rollout replaces the pod, so it is re-resolved for the pod actually
    /// selected rather than pinned at startup.
    named_port: Option<NamedPort>,
    http_log_watcher: HttpLogStateWatcher,
    background_tasks: Arc<std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    connection_tasks: Arc<std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    /// Set once shutdown has drained the registries, so a connection accepted
    /// afterwards is aborted rather than tracked by nobody.
    workers_closed: Arc<std::sync::atomic::AtomicBool>,
}

impl PortForwarder {
    pub async fn new(
        namespace: &str, target: Target, context_name: Option<String>, kubeconfig: Option<String>,
        pod_readiness: kube_portforward::PodReadiness,
    ) -> anyhow::Result<Self> {
        let client_key = ServiceClientKey::new(context_name, kubeconfig);
        let connection = SHARED_CLIENT_MANAGER.get_connection(client_key).await?;

        let pod_selector =
            crate::kube::target::resolve_pod_selector(&connection.client, namespace, &target)
                .await?;
        let pod_api: Api<Pod> = Api::namespaced(connection.client.clone(), namespace);

        let forwarder = kube_portforward::Forwarder::builder(
            connection.client.clone(),
            connection.cluster_url.clone(),
            namespace,
        )
        .pod_selector(pod_selector)
        .pod_readiness(pod_readiness)
        .build()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to build port forwarder: {}", e))?;
        let forwarder = Arc::new(forwarder);

        let startup = async {
            let port = crate::kube::target::resolve_target_port(
                &forwarder,
                &pod_api,
                &target,
                tokio::time::Duration::from_secs(5),
            )
            .await?;
            drop(forwarder.connect(port).await?);
            Ok::<_, anyhow::Error>(port)
        };
        let target_port = match tokio::time::timeout(tokio::time::Duration::from_secs(10), startup)
            .await
            .map_err(|err| anyhow::Error::new(err).context("Port-forward startup timed out"))
            .and_then(|result| result)
        {
            Ok(port) => port,
            Err(err) => {
                let _ = forwarder.shutdown().await;
                return Err(err);
            }
        };

        let named_port =
            matches!(target.port, crate::kube::models::Port::Name(_)).then(|| NamedPort {
                target: target.clone(),
                pod_api: pod_api.clone(),
                resolved: Arc::new(std::sync::Mutex::new(None)),
            });

        Ok(Self {
            namespace: namespace.into(),
            forwarder,
            target_port,
            named_port,
            http_log_watcher: HttpLogStateWatcher::new(),
            background_tasks: Arc::new(std::sync::Mutex::new(Vec::new())),
            connection_tasks: Arc::new(std::sync::Mutex::new(Vec::new())),
            workers_closed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        })
    }

    pub async fn get_stream(&self) -> anyhow::Result<kube_portforward::Stream> {
        const STREAM_ACQUIRE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

        // Resolution is inside the deadline: it can perform a pod GET, and a
        // stalled API server would otherwise hold the client past the ten
        // seconds this promises and delay stream-failure recovery.
        tokio::time::timeout(STREAM_ACQUIRE_TIMEOUT, self.acquire_stream())
            .await
            .context("Timed out acquiring a port-forward stream")?
    }

    /// Opens a stream on the pod whose port number it used.
    ///
    /// For a named port the two have to agree: a rollout between resolving the
    /// name and opening the connection would otherwise send traffic to a port
    /// number the replacement pod does not use.
    async fn acquire_stream(&self) -> anyhow::Result<kube_portforward::Stream> {
        const ACQUIRE_ATTEMPTS: usize = 3;

        let Some(named) = &self.named_port else {
            return Ok(self.forwarder.connect(self.target_port).await?);
        };

        for _ in 0..ACQUIRE_ATTEMPTS {
            let (port, pod) = self.resolve_named_port(named).await?;
            let (stream, connected_pod) = self.forwarder.connect_on_pod(port).await?;
            if connected_pod == pod {
                return Ok(stream);
            }
            // The stream reaches a pod this port number was not read from, so
            // it is dropped rather than used: resolving again picks up the
            // replacement's own mapping.
            debug!("Reconnecting: resolved on {pod} but connected to {connected_pod}");
            named
                .resolved
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
        }

        Err(anyhow::anyhow!(
            "The selected pod kept changing while resolving the named port"
        ))
    }

    /// The port to connect to for the pod currently selected.
    ///
    /// A named port is re-resolved when the ready pod changes: a rollout can
    /// map the same name to a different number, and the old one would then
    /// reach nothing or the wrong container port.
    async fn resolve_named_port(&self, named: &NamedPort) -> anyhow::Result<(u16, String)> {
        if let Some((pod, port)) = named
            .resolved
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return Ok((port, pod));
        }

        let (port, resolved_pod) = crate::kube::target::resolve_target_port_for_pod(
            &self.forwarder,
            &named.pod_api,
            &named.target,
            tokio::time::Duration::from_secs(5),
        )
        .await?;
        let pod = resolved_pod.ok_or_else(|| {
            anyhow::anyhow!("A named port must resolve against a pod, but none was reported")
        })?;
        // Cached under the pod the number was read from, so the acquisition
        // above can tell whether the stream it got belongs to that same pod.
        *named.resolved.lock().unwrap_or_else(|e| e.into_inner()) = Some((pod.clone(), port));

        Ok((port, pod))
    }

    pub async fn handle_tcp_listener(
        self: Arc<Self>, listener: TcpListener, config_id: i64, workload_type: String, port: u16,
        cancellation_token: CancellationToken, tls_acceptor: Option<tokio_rustls::TlsAcceptor>,
    ) -> anyhow::Result<()> {
        let initial_logging_enabled =
            match kftray_commons::utils::http_logs_config::get_http_logs_config(config_id).await {
                Ok(config) => config.enabled,
                Err(_) => false,
            };

        self.http_log_watcher
            .set_http_logs(config_id, initial_logging_enabled)
            .await?;

        let http_log_watcher_clone = self.http_log_watcher.clone();
        let sync_cancel_token = cancellation_token.clone();
        let sync_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if let Err(e) = http_log_watcher_clone
                            .sync_from_external_state(config_id)
                            .await
                        {
                            debug!("Sync error for config {}: {}", config_id, e);
                        }
                    }
                    _ = sync_cancel_token.cancelled() => {
                        debug!("HTTP log sync task cancelled for config {}", config_id);
                        break;
                    }
                }
            }
        });
        self.track_task(sync_task);

        let tcp_forwarder = TcpForwarder::new(config_id, workload_type);

        let forwarder_clone = Arc::clone(&self);
        let cancel_token = cancellation_token.clone();

        let mut pod_change_rx = self.forwarder.subscribe_pod_changes();
        let mut consecutive_accept_errors: u32 = 0;
        const MAX_ACCEPT_ERRORS: u32 = 10;
        const BASE_BACKOFF_MS: u64 = 10;
        const MAX_BACKOFF_MS: u64 = 5000;
        let consecutive_stream_failures = Arc::new(std::sync::atomic::AtomicU32::new(0));
        const MAX_STREAM_FAILURES: u32 = 5;

        loop {
            let (client_conn, client_addr) = tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok(connection) => {
                            consecutive_accept_errors = 0;
                            connection
                        }
                        Err(e) => {
                            consecutive_accept_errors += 1;
                            let exponent = consecutive_accept_errors.saturating_sub(1).min(10);
                            let backoff_ms = std::cmp::min(
                                BASE_BACKOFF_MS.saturating_mul(1u64 << exponent),
                                MAX_BACKOFF_MS
                            );
                            error!(
                                "Accept failed ({}/{}): {}, backing off {}ms",
                                consecutive_accept_errors, MAX_ACCEPT_ERRORS, e, backoff_ms
                            );
                            if consecutive_accept_errors >= MAX_ACCEPT_ERRORS {
                                error!(
                                    "Too many consecutive accept errors, stopping listener for config {}",
                                    config_id
                                );
                                return Err(anyhow::anyhow!(
                                    "TCP listener failed after {} consecutive accept errors: {}",
                                    MAX_ACCEPT_ERRORS, e
                                ));
                            }
                            tokio::select! {
                                _ = tokio::time::sleep(tokio::time::Duration::from_millis(backoff_ms)) => {}
                                _ = cancel_token.cancelled() => {
                                    debug!("Backoff interrupted by cancellation");
                                    break;
                                }
                            }
                            continue;
                        }
                    }
                }
                pod_change = pod_change_rx.recv() => {
                    match pod_change {
                        Ok(kube_portforward::PodChange::Died(dead_pod_name)) => {
                            debug!(
                                "Pod {} died reactively, signaling recovery for config {}",
                                dead_pod_name, config_id
                            );
                            if let Some(rm) = crate::kube::proxy_recovery::RECOVERY_MANAGERS.get(&config_id) {
                                rm.signal_recovery(crate::kube::proxy_recovery::RecoverySignal::PodDied);
                            }
                        }
                        Ok(_) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            debug!("pod_change_rx lagged by {} for config {}", n, config_id);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            debug!("pod_change_rx closed for config {}", config_id);
                        }
                    }
                    continue;
                }
                _ = cancel_token.cancelled() => {
                    break;
                }
            };

            let forwarder = Arc::clone(&forwarder_clone);
            let mut tcp_forwarder = tcp_forwarder.clone();
            let http_log_watcher_clone = Arc::new(self.http_log_watcher.clone());
            let cancel_token_clone = cancel_token.clone();
            let tls_acceptor_clone = tls_acceptor.clone();
            let connection_tasks = Arc::clone(&self.connection_tasks);
            let workers_closed = Arc::clone(&self.workers_closed);
            let stream_failures_clone = Arc::clone(&consecutive_stream_failures);

            let handle = tokio::spawn(async move {
                let mut client_conn = client_conn;
                let tls_acceptor_clone = match tls_acceptor_clone {
                    Some(_) if is_http_request(&client_conn).await => {
                        if let Err(e) = handle_http_redirect(client_conn, port).await {
                            debug!("Failed to redirect HTTP to HTTPS: {}", e);
                        }
                        return;
                    }
                    acceptor => acceptor,
                };

                let upstream_stream = match forwarder.get_stream().await {
                    Ok(stream) => {
                        stream_failures_clone.store(0, std::sync::atomic::Ordering::SeqCst);
                        stream
                    }
                    Err(e) => {
                        let failures = stream_failures_clone
                            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                            + 1;
                        if failures >= MAX_STREAM_FAILURES
                            && let Some(rm) =
                                crate::kube::proxy_recovery::RECOVERY_MANAGERS.get(&config_id)
                        {
                            rm.signal_recovery(
                                crate::kube::proxy_recovery::RecoverySignal::StreamFailed,
                            );
                        }
                        error!("Failed to create stream for {}: {}", client_addr, e);
                        let _ = client_conn.shutdown().await;
                        return;
                    }
                };

                if let Some(acceptor) = tls_acceptor_clone {
                    match acceptor.accept(client_conn).await {
                        Ok(tls_stream) => {
                            if let Err(e) = tcp_forwarder
                                .forward_tls_streams(
                                    tls_stream,
                                    upstream_stream,
                                    cancel_token_clone.clone(),
                                )
                                .await
                            {
                                debug!("TLS forwarding error for {}: {}", client_addr, e);
                            }
                        }
                        Err(e) => {
                            debug!("TLS handshake failed for {}: {}", client_addr, e);
                        }
                    }
                } else if let Err(e) = tcp_forwarder
                    .forward_streams(
                        client_conn,
                        upstream_stream,
                        client_addr,
                        cancel_token_clone,
                        Arc::clone(&http_log_watcher_clone),
                        port,
                    )
                    .await
                {
                    debug!("TCP forwarding error for {}: {}", client_addr, e);
                }
            });

            {
                let mut tasks = Self::registry(&connection_tasks);
                // Registration is closed under the same lock the final drain
                // takes, so a connection accepted while shutdown runs is
                // aborted instead of being left untracked.
                if workers_closed.load(std::sync::atomic::Ordering::SeqCst) {
                    handle.abort();
                } else {
                    tasks.retain(|handle| !handle.is_finished());
                    tasks.push(handle);
                }
            }
        }

        Ok(())
    }

    pub async fn start_listener(
        self: Arc<Self>, listener_config: ListenerConfig, config_id: i64, workload_type: String,
        cancellation_token: CancellationToken,
    ) -> anyhow::Result<(u16, JoinHandle<anyhow::Result<()>>)> {
        if let Err(e) =
            crate::network_utils::ensure_loopback_address(&listener_config.local_address).await
        {
            return Err(anyhow::anyhow!("Network config failed: {}", e));
        }

        match listener_config.protocol {
            Protocol::Tcp => {
                self.start_tcp_listener(
                    listener_config,
                    config_id,
                    workload_type,
                    cancellation_token,
                )
                .await
            }
            Protocol::Udp => {
                self.start_udp_listener(listener_config, config_id, cancellation_token)
                    .await
            }
        }
    }

    async fn start_tcp_listener(
        self: Arc<Self>, listener_config: ListenerConfig, config_id: i64, workload_type: String,
        cancellation_token: CancellationToken,
    ) -> anyhow::Result<(u16, JoinHandle<anyhow::Result<()>>)> {
        let ip = listener_config
            .local_address
            .parse::<std::net::IpAddr>()
            .map_err(|e| {
                anyhow::anyhow!(
                    "Invalid IP address '{}': {}",
                    listener_config.local_address,
                    e
                )
            })?;
        let addr = std::net::SocketAddr::new(ip, listener_config.local_port);

        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to bind TCP listener to {}: {}", addr, e))?;
        let port = listener.local_addr()?.port();

        let tls_acceptor = listener_config.tls_acceptor;
        let handle = tokio::spawn(async move {
            self.handle_tcp_listener(
                listener,
                config_id,
                workload_type,
                port,
                cancellation_token,
                tls_acceptor,
            )
            .await
        });

        Ok((port, handle))
    }

    async fn start_udp_listener(
        self: Arc<Self>, listener_config: ListenerConfig, config_id: i64,
        cancellation_token: CancellationToken,
    ) -> anyhow::Result<(u16, JoinHandle<anyhow::Result<()>>)> {
        let signal_token = cancellation_token.clone();
        let upstream = Arc::new(ForwarderUpstream {
            forwarder: Arc::clone(&self),
            failures: UdpUpstreamFailures::new(config_id),
        });
        let (port, forward_future) = UdpForwarder::bind_and_forward(
            listener_config.local_address,
            listener_config.local_port,
            upstream,
            cancellation_token,
        )
        .await?;
        let handle = spawn_udp_forward_owner(forward_future, signal_token, config_id);
        Ok((port, handle))
    }

    pub fn get_http_log_watcher(&self) -> &HttpLogStateWatcher {
        &self.http_log_watcher
    }

    pub async fn set_http_logging(&self, config_id: i64, enabled: bool) -> anyhow::Result<()> {
        self.http_log_watcher
            .set_http_logs(config_id, enabled)
            .await
    }

    pub async fn get_http_logging(&self, config_id: i64) -> bool {
        self.http_log_watcher.get_http_logs(config_id).await
    }

    fn track_task(&self, handle: tokio::task::JoinHandle<()>) {
        Self::registry(&self.background_tasks).push(handle);
    }

    fn registry(
        tasks: &Arc<std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    ) -> std::sync::MutexGuard<'_, Vec<tokio::task::JoinHandle<()>>> {
        tasks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    async fn cleanup_tasks(&self, tasks: &Arc<std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>>) {
        let tasks = {
            let mut registry = Self::registry(tasks);
            // Closed first: an accept iteration already running would otherwise
            // register a worker after this drain and never be joined.
            self.workers_closed
                .store(true, std::sync::atomic::Ordering::SeqCst);
            std::mem::take(&mut *registry)
        };
        for handle in &tasks {
            handle.abort();
        }
        for handle in tasks {
            let _ = handle.await;
        }
    }

    /// Requests abortion of every worker without awaiting, so a synchronous
    /// `Drop` can still release the sockets held by connections stalled outside
    /// a cancellation point, such as a TLS handshake.
    ///
    /// The handles are kept: aborting only schedules cancellation, so
    /// [`shutdown`](Self::shutdown) still has something to await before
    /// reporting the workers gone.
    pub fn abort_workers(&self) {
        for tasks in [&self.background_tasks, &self.connection_tasks] {
            for handle in Self::registry(tasks).iter() {
                handle.abort();
            }
        }
    }

    pub async fn get_current_active_pod(&self) -> Option<String> {
        match self.forwarder.ready_pod() {
            Some(pod_name) => Some(pod_name),
            None => {
                if self.forwarder.has_running_pods() {
                    Some("pending-rollout".to_string())
                } else {
                    None
                }
            }
        }
    }

    pub async fn shutdown(&self) {
        info!(
            "Shutting down port forwarder for namespace: {}",
            self.namespace.as_ref()
        );

        self.http_log_watcher.shutdown();

        self.cleanup_tasks(&self.background_tasks).await;
        self.cleanup_tasks(&self.connection_tasks).await;

        if let Err(e) = self.forwarder.shutdown().await {
            debug!("Forwarder shutdown returned an error: {}", e);
        }
    }
}

/// Tracks consecutive tunnel-open failures for one config and escalates to
/// recovery once the relay has stopped accepting new tunnels.
struct UdpUpstreamFailures {
    config_id: i64,
    consecutive: std::sync::atomic::AtomicU32,
}

impl UdpUpstreamFailures {
    const MAX: u32 = 5;

    fn new(config_id: i64) -> Self {
        Self {
            config_id,
            consecutive: std::sync::atomic::AtomicU32::new(0),
        }
    }

    fn reset(&self) {
        self.consecutive
            .store(0, std::sync::atomic::Ordering::SeqCst);
    }

    fn record(&self, error: &anyhow::Error) {
        let failures = self
            .consecutive
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        error!(
            "Failed to open a UDP tunnel for config {}: {}",
            self.config_id, error
        );
        if failures >= Self::MAX
            && let Some(rm) = crate::kube::proxy_recovery::RECOVERY_MANAGERS.get(&self.config_id)
        {
            rm.signal_recovery(crate::kube::proxy_recovery::RecoverySignal::StreamFailed);
        }
    }
}

/// Opens a fresh port-forward stream for every local UDP client so replies
/// cannot cross between clients sharing the listener.
struct ForwarderUpstream {
    forwarder: Arc<PortForwarder>,
    failures: UdpUpstreamFailures,
}

impl crate::kube::udp_forwarder::UdpUpstream for ForwarderUpstream {
    type Stream = kube_portforward::Stream;

    async fn connect(&self) -> anyhow::Result<Self::Stream> {
        self.forwarder.get_stream().await
    }

    fn on_connect_failure(&self, error: &anyhow::Error) {
        self.failures.record(error);
    }

    fn on_session_failure(&self, error: &anyhow::Error) {
        self.failures.record(error);
    }

    fn on_session_traffic(&self) {
        self.failures.reset();
    }
}

fn spawn_udp_forward_owner(
    forward_future: impl Future<Output = anyhow::Result<()>> + Send + 'static,
    signal_token: CancellationToken, config_id: i64,
) -> JoinHandle<anyhow::Result<()>> {
    tokio::spawn(async move {
        let result = forward_future.await;
        if !signal_token.is_cancelled()
            && let Some(rm) = crate::kube::proxy_recovery::RECOVERY_MANAGERS.get(&config_id)
        {
            log::info!(
                "UDP forwarder task completed, signaling recovery for config_id={}",
                config_id
            );
            rm.signal_recovery(crate::kube::proxy_recovery::RecoverySignal::StreamFailed);
        }
        result
    })
}

async fn is_http_request(client_conn: &TcpStream) -> bool {
    let mut peek_buf = [0u8; 64];

    if client_conn.peek(&mut peek_buf).await.is_ok() {
        let mut headers = [httparse::EMPTY_HEADER; 4];
        let mut req = Request::new(&mut headers);

        matches!(
            req.parse(&peek_buf),
            Ok(httparse::Status::Complete(_)) | Ok(httparse::Status::Partial)
        )
    } else {
        false
    }
}

async fn handle_http_redirect(mut stream: TcpStream, port: u16) -> anyhow::Result<()> {
    let mut buffer = [0u8; 1024];
    let n = stream.read(&mut buffer).await?;

    let request = std::str::from_utf8(&buffer[..n])?;
    let mut lines = request.lines();

    let request_line = match lines.next() {
        Some(line) => line,
        None => return Ok(()),
    };

    let path = request_line.split_whitespace().nth(1).unwrap_or("/");

    let host = lines
        .find(|line| line.to_lowercase().starts_with("host:"))
        .and_then(|line| line.split(':').nth(1))
        .map(|h| h.trim())
        .unwrap_or("localhost");

    let response = format!(
        "HTTP/1.1 301 Moved Permanently\r\n\
        Location: https://{}:{}{}\r\n\
        Content-Length: 0\r\n\
        \r\n",
        host, port, path
    );

    stream.write_all(response.as_bytes()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use http::{
        Request,
        Response,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use kube::client::Body;
    use tower_test::mock;

    use super::*;

    fn ready_test_pod(name: &str) -> Pod {
        use k8s_openapi::api::core::v1::{
            PodCondition,
            PodStatus,
        };

        Pod {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                uid: Some(format!("{name}-uid")),
                resource_version: Some("1".to_string()),
                ..Default::default()
            },
            status: Some(PodStatus {
                phase: Some("Running".to_string()),
                conditions: Some(vec![PodCondition {
                    type_: "Ready".into(),
                    status: "True".into(),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    async fn serve_ready_pod_then_stall(
        mut handle: mock::Handle<Request<Body>, Response<Body>>, pod_name: &str,
        upgrade_started: Arc<tokio::sync::Notify>, ready_delay: Duration,
    ) {
        let (request, send) = handle
            .next_request()
            .await
            .expect("expected the initial pod list request");
        assert!(
            request.uri().path().ends_with("/pods"),
            "expected a pod list request, got {}",
            request.uri()
        );

        let body = serde_json::json!({
            "apiVersion": "v1",
            "kind": "PodList",
            "metadata": { "resourceVersion": "1" },
            "items": [ready_test_pod(pod_name)],
        });
        let response = Response::builder()
            .status(200)
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        tokio::time::sleep(ready_delay).await;
        send.send_response(response);

        let mut held_pending = Vec::new();
        while let Some((request, send)) = handle.next_request().await {
            if request.uri().path().ends_with("/portforward") {
                upgrade_started.notify_one();
            }
            held_pending.push(send);
        }
    }

    #[tokio::test]
    async fn get_stream_preserves_the_pod_readiness_wait() {
        tokio::time::pause();
        let pod_name = "delayed-ready";
        let (mock_service, handle) = mock::pair::<Request<Body>, Response<Body>>();
        let kube_client = kube::Client::new(mock_service, "default");
        let upgrade_started = Arc::new(tokio::sync::Notify::new());
        let driver = tokio::spawn(serve_ready_pod_then_stall(
            handle,
            pod_name,
            Arc::clone(&upgrade_started),
            Duration::from_secs(4),
        ));
        let forwarder = kube_portforward::Forwarder::builder(
            kube_client,
            "http://127.0.0.1:1".parse().unwrap(),
            "default",
        )
        .pod_selector(kube_portforward::PodSelector::Name(pod_name.to_owned()))
        .build()
        .await
        .unwrap();
        let port_forwarder = PortForwarder {
            namespace: "default".into(),
            forwarder: Arc::new(forwarder),
            target_port: 8080,
            named_port: None,
            http_log_watcher: HttpLogStateWatcher::new(),
            background_tasks: Arc::new(std::sync::Mutex::new(Vec::new())),
            connection_tasks: Arc::new(std::sync::Mutex::new(Vec::new())),
            workers_closed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };
        let mut acquisition = Box::pin(port_forwarder.get_stream());
        tokio::select! {
            _ = upgrade_started.notified() => {}
            result = &mut acquisition => {
                panic!("acquisition ended before the ready pod could start its upgrade: {:?}", result.err());
            }
        }
        drop(acquisition);
        port_forwarder.shutdown().await;
        driver.abort();
        let _ = driver.await;
    }

    #[tokio::test]
    async fn get_stream_times_out_when_upgrade_response_never_arrives() {
        tokio::time::pause();

        let pod_name = "web-0";
        let (mock_service, handle) = mock::pair::<Request<Body>, Response<Body>>();
        let kube_client = kube::Client::new(mock_service, "default");
        let cluster_url: http::Uri = "http://127.0.0.1:1".parse().unwrap();

        let upgrade_started = Arc::new(tokio::sync::Notify::new());
        let driver = tokio::spawn(serve_ready_pod_then_stall(
            handle,
            pod_name,
            Arc::clone(&upgrade_started),
            Duration::ZERO,
        ));

        let forwarder = kube_portforward::Forwarder::builder(kube_client, cluster_url, "default")
            .pod_selector(kube_portforward::PodSelector::Name(pod_name.to_string()))
            .pod_readiness(kube_portforward::PodReadiness::default())
            .build()
            .await
            .expect("forwarder should build without contacting the apiserver");

        let ready = forwarder
            .wait_for_ready_pod(Duration::from_secs(5))
            .await
            .expect("pod list response should mark the pod ready");
        assert_eq!(ready, pod_name);

        let port_forwarder = PortForwarder {
            namespace: "default".into(),
            forwarder: Arc::new(forwarder),
            target_port: 8080,
            named_port: None,
            http_log_watcher: HttpLogStateWatcher::new(),
            background_tasks: Arc::new(std::sync::Mutex::new(Vec::new())),
            connection_tasks: Arc::new(std::sync::Mutex::new(Vec::new())),
            workers_closed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };

        let get_stream_fut = port_forwarder.get_stream();
        tokio::pin!(get_stream_fut);
        tokio::select! {
            _ = upgrade_started.notified() => {}
            _ = &mut get_stream_fut => panic!("acquisition ended before the upgrade request"),
        }

        tokio::time::advance(Duration::from_secs(10)).await;
        let result = tokio::time::timeout(Duration::from_secs(1), get_stream_fut)
            .await
            .expect("the acquisition deadline must complete the pending upgrade");
        let error = match result {
            Ok(_) => panic!("the pending upgrade unexpectedly completed"),
            Err(error) => error,
        };
        assert!(
            error
                .downcast_ref::<tokio::time::error::Elapsed>()
                .is_some()
        );
        port_forwarder.shutdown().await;
        driver.abort();
        let _ = driver.await;
    }

    fn recovery_receiver(
        config_id: i64,
    ) -> tokio::sync::broadcast::Receiver<crate::kube::proxy_recovery::RecoverySignal> {
        let config = kftray_commons::models::config_model::Config {
            id: Some(config_id),
            kubeconfig: Some("/nonexistent/isolated-test-kubeconfig".to_owned()),
            protocol: "udp".to_owned(),
            ..Default::default()
        };
        let manager = Arc::new(crate::kube::proxy_recovery::ProxyRecoveryManager::new(
            config,
            crate::kube::proxy_recovery::ProxyType::Deployment,
            kftray_commons::utils::db_mode::DatabaseMode::Memory,
            false,
        ));
        let receiver = manager.subscribe_recovery_signals();
        crate::kube::proxy_recovery::RECOVERY_MANAGERS.insert(config_id, manager);
        receiver
    }

    #[tokio::test]
    async fn repeated_udp_tunnel_failures_signal_recovery() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let config_id = 900_101;
        let mut receiver = recovery_receiver(config_id);
        let failures = UdpUpstreamFailures::new(config_id);

        for _ in 1..UdpUpstreamFailures::MAX {
            failures.record(&anyhow::anyhow!("no relay pod"));
        }
        let quiet = receiver.try_recv();
        failures.record(&anyhow::anyhow!("no relay pod"));
        let signal = receiver.try_recv();
        crate::kube::proxy_recovery::RECOVERY_MANAGERS.remove(&config_id);

        assert!(
            matches!(
                quiet,
                Err(tokio::sync::broadcast::error::TryRecvError::Empty)
            ),
            "a transient tunnel failure must not trigger recovery"
        );
        assert_eq!(
            signal.unwrap(),
            crate::kube::proxy_recovery::RecoverySignal::StreamFailed
        );
    }

    #[tokio::test]
    async fn a_successful_tunnel_clears_earlier_failures() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let config_id = 900_103;
        let mut receiver = recovery_receiver(config_id);
        let failures = UdpUpstreamFailures::new(config_id);

        for _ in 1..UdpUpstreamFailures::MAX {
            failures.record(&anyhow::anyhow!("no relay pod"));
        }
        failures.reset();
        failures.record(&anyhow::anyhow!("no relay pod"));
        let signal = receiver.try_recv();
        crate::kube::proxy_recovery::RECOVERY_MANAGERS.remove(&config_id);

        assert!(matches!(
            signal,
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn udp_owner_does_not_signal_recovery_on_intentional_cancellation() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let config_id = 900_102;
        let (upstream, _opened) = crate::kube::udp_forwarder::tests::SpawningUpstream::new(64);
        let token = CancellationToken::new();
        let (_, forward) =
            UdpForwarder::bind_and_forward("127.0.0.1".to_owned(), 0, upstream, token.clone())
                .await
                .unwrap();
        let mut receiver = recovery_receiver(config_id);
        token.cancel();
        let owner = spawn_udp_forward_owner(forward, token, config_id);
        tokio::time::timeout(Duration::from_secs(5), owner)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let signal = receiver.try_recv();
        crate::kube::proxy_recovery::RECOVERY_MANAGERS.remove(&config_id);
        assert!(matches!(
            signal,
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    }

    #[tokio::test]
    async fn aborting_the_udp_owner_releases_the_socket_for_immediate_rebind() {
        let (upstream, _opened) = crate::kube::udp_forwarder::tests::SpawningUpstream::new(1024);
        let cancellation_token = CancellationToken::new();

        let (port, forward_future) = UdpForwarder::bind_and_forward(
            "127.0.0.1".to_string(),
            0,
            upstream,
            cancellation_token.clone(),
        )
        .await
        .unwrap();

        let owner = spawn_udp_forward_owner(forward_future, cancellation_token, 900_201);
        owner.abort();
        let join_result = owner.await;
        assert!(
            join_result.is_err() && join_result.unwrap_err().is_cancelled(),
            "the owner task should report a cancelled join, not a normal result"
        );

        let rebound = tokio::net::UdpSocket::bind(format!("127.0.0.1:{port}")).await;
        assert!(
            rebound.is_ok(),
            "rebinding the same address right after awaited cleanup should succeed: {:?}",
            rebound.err()
        );
    }

    #[tokio::test]
    async fn cleanup_shuts_the_forwarder_down_when_the_listener_task_ignores_abort() {
        let pod_name = "web-0";
        let (mock_service, handle) = mock::pair::<Request<Body>, Response<Body>>();
        let kube_client = kube::Client::new(mock_service, "default");
        let upgrade_started = Arc::new(tokio::sync::Notify::new());
        let driver = tokio::spawn(serve_ready_pod_then_stall(
            handle,
            pod_name,
            Arc::clone(&upgrade_started),
            Duration::ZERO,
        ));
        let forwarder = kube_portforward::Forwarder::builder(
            kube_client,
            "http://127.0.0.1:1".parse().unwrap(),
            "default",
        )
        .pod_selector(kube_portforward::PodSelector::Name(pod_name.to_owned()))
        .build()
        .await
        .unwrap();
        let port_forwarder = Arc::new(PortForwarder {
            namespace: "default".into(),
            forwarder: Arc::new(forwarder),
            target_port: 8080,
            named_port: None,
            http_log_watcher: HttpLogStateWatcher::new(),
            background_tasks: Arc::new(std::sync::Mutex::new(Vec::new())),
            connection_tasks: Arc::new(std::sync::Mutex::new(Vec::new())),
            workers_closed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });
        port_forwarder.track_task(tokio::spawn(std::future::pending()));

        let stubborn = tokio::task::spawn_blocking(|| -> anyhow::Result<()> {
            std::thread::sleep(Duration::from_secs(3));
            Ok(())
        });
        let mut process = crate::port_forward::PortForwardProcess::with_forwarder_and_token(
            stubborn,
            Arc::clone(&port_forwarder),
            "410031".to_owned(),
            CancellationToken::new(),
        );

        tokio::time::timeout(Duration::from_secs(5), process.cleanup_and_abort())
            .await
            .expect("cleanup must not wait on a task that ignores abort");

        assert!(
            PortForwarder::registry(&port_forwarder.background_tasks).is_empty(),
            "the forwarder must be shut down even when the listener join times out"
        );
        driver.abort();
        let _ = driver.await;
    }

    #[tokio::test]
    async fn dropping_a_process_releases_a_connection_stalled_outside_cancellation() {
        let pod_name = "web-0";
        let (mock_service, handle) = mock::pair::<Request<Body>, Response<Body>>();
        let kube_client = kube::Client::new(mock_service, "default");
        let driver = tokio::spawn(serve_ready_pod_then_stall(
            handle,
            pod_name,
            Arc::new(tokio::sync::Notify::new()),
            Duration::ZERO,
        ));
        let forwarder = kube_portforward::Forwarder::builder(
            kube_client,
            "http://127.0.0.1:1".parse().unwrap(),
            "default",
        )
        .pod_selector(kube_portforward::PodSelector::Name(pod_name.to_owned()))
        .build()
        .await
        .unwrap();
        let port_forwarder = Arc::new(PortForwarder {
            namespace: "default".into(),
            forwarder: Arc::new(forwarder),
            target_port: 8080,
            named_port: None,
            http_log_watcher: HttpLogStateWatcher::new(),
            background_tasks: Arc::new(std::sync::Mutex::new(Vec::new())),
            connection_tasks: Arc::new(std::sync::Mutex::new(Vec::new())),
            workers_closed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });

        // A client that connected and then stalled where no cancellation token
        // is polled, holding its socket for as long as its task lives.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let (accepted, _) = listener.accept().await.unwrap();
        drop(listener);
        let stalled = tokio::spawn(async move {
            let _accepted = accepted;
            std::future::pending::<()>().await;
        });
        let stalled_task = stalled.abort_handle();
        PortForwarder::registry(&port_forwarder.connection_tasks).push(stalled);

        let process = crate::port_forward::PortForwardProcess::with_forwarder_and_token(
            tokio::spawn(std::future::pending()),
            Arc::clone(&port_forwarder),
            "410071".to_owned(),
            CancellationToken::new(),
        );

        drop(process);
        tokio::task::yield_now().await;

        assert!(
            stalled_task.is_finished(),
            "dropping an unregistered process must abort the forwarder's connection tasks"
        );
        drop(client);
        driver.abort();
        let _ = driver.await;
    }
}

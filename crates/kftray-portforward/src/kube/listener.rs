use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{
    AtomicBool,
    Ordering,
};
use std::task::{
    Context,
    Poll,
};

use anyhow::anyhow;
use httparse::Request;
use k8s_openapi::api::core::v1::{
    Pod,
    Service,
};
use kube::api::Api;
use kube_portforward::{
    Forwarder,
    PodSelector,
    RecoverySignal,
};
use tokio::io::{
    AsyncRead,
    AsyncReadExt,
    AsyncWrite,
    AsyncWriteExt,
    ReadBuf,
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
use crate::kube::models::{
    Port,
    Target,
    TargetSelector,
};
use crate::kube::tcp_forwarder::TcpForwarder;
use crate::kube::udp_forwarder::UdpForwarder;
use crate::registry::PORT_FORWARD_REGISTRY;

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

pub enum Upstream {
    Ws(kube_portforward::Stream),
}

impl AsyncRead for Upstream {
    fn poll_read(
        mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.as_mut().get_mut() {
            Upstream::Ws(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Upstream {
    fn poll_write(
        mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.as_mut().get_mut() {
            Upstream::Ws(s) => Pin::new(s).poll_write(cx, buf),
        }
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.as_mut().get_mut() {
            Upstream::Ws(s) => Pin::new(s).poll_flush(cx),
        }
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.as_mut().get_mut() {
            Upstream::Ws(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}

pub struct PortForwarder {
    forwarder: Arc<Forwarder>,
    pod_api: Api<Pod>,
    /// Service API for translating service-port to pod-targetPort. `None`
    /// for non-service workloads (pod, proxy).
    service_api: Option<Api<Service>>,
    /// Service name to look up for port translation. `None` for
    /// non-service workloads.
    service_name: Option<String>,
    target_port: Option<u16>,
    http_log_watcher: HttpLogStateWatcher,
    initialized: Arc<AtomicBool>,
    initialization_mutex: Arc<tokio::sync::Mutex<()>>,
    background_tasks: Arc<tokio::sync::Mutex<Vec<JoinHandle<()>>>>,
    connection_tasks: Arc<tokio::sync::Mutex<Vec<JoinHandle<()>>>>,
}

impl PortForwarder {
    pub async fn new(
        namespace: &str, target: Target, context_name: Option<String>, kubeconfig: Option<String>,
        config_id: i64,
    ) -> anyhow::Result<Self> {
        let client_key = crate::kube::shared_client::ServiceClientKey::new(
            context_name.clone(),
            kubeconfig.clone(),
        );

        let cluster_url = {
            let paths =
                crate::kube::client::config::get_kubeconfig_paths_from_option(kubeconfig.clone())?;
            let (merged, _ctxs, _errs) = crate::kube::client::config::merge_kubeconfigs(&paths)?;
            let ctx_name = context_name.as_deref().unwrap_or("@current");
            let cfg =
                crate::kube::client::config::create_config_with_context(&merged, ctx_name).await?;
            cfg.cluster_url.clone()
        };

        let client = PORT_FORWARD_REGISTRY.acquire_client(client_key).await?;
        let pod_api: Api<Pod> = Api::namespaced((*client).clone(), namespace);

        // Capture the service name (if any) up-front so we can translate
        // a service-level port (e.g. 80) to the pod's targetPort (e.g.
        // 8080) at initialize() time. Without this, the SPDY port-forward
        // sends the service port to the kubelet, which dials the pod on
        // that port and fails with "connection refused" because the pod
        // listens on a different port.
        let (service_api, service_name) = match &target.selector {
            TargetSelector::ServiceName(name) => (
                Some(Api::<Service>::namespaced((*client).clone(), namespace)),
                Some(name.clone()),
            ),
            TargetSelector::PodLabel(_) => (None, None),
        };

        let label_selector = resolve_label_selector(&client, namespace, &target.selector).await?;

        let forwarder = Forwarder::builder((*client).clone(), cluster_url, namespace)
            .pod_selector(PodSelector::Labels {
                selector: label_selector,
            })
            .max_sessions(128)
            .session_capacity(32)
            .keepalive(
                std::time::Duration::from_secs(15),
                std::time::Duration::from_secs(30),
            )
            .shutdown_grace(std::time::Duration::from_secs(2))
            .on_recovery(move |signal: RecoverySignal| {
                if let Some(rm) = crate::kube::proxy_recovery::RECOVERY_MANAGERS.get(&config_id) {
                    let sig = match signal {
                        RecoverySignal::ServerClose => {
                            crate::kube::proxy_recovery::RecoverySignal::PodDied
                        }
                        _ => crate::kube::proxy_recovery::RecoverySignal::StreamFailed,
                    };
                    rm.signal_recovery(sig);
                }
            })
            .build()
            .await
            .map_err(|e| anyhow!("failed to build forwarder: {}", e))?;

        Ok(Self {
            forwarder: Arc::new(forwarder),
            pod_api,
            service_api,
            service_name,
            target_port: None,
            http_log_watcher: HttpLogStateWatcher::new(),
            initialized: Arc::new(AtomicBool::new(false)),
            initialization_mutex: Arc::new(tokio::sync::Mutex::new(())),
            background_tasks: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            connection_tasks: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        })
    }

    /// Resolve the requested user-facing port to the actual pod container
    /// port that the kubelet should dial.
    ///
    /// Kubernetes port-forward operates on **pods**, not services. When the
    /// user configures a forward against a service (e.g. `port: 80` on a
    /// service that maps `targetPort: 8080`), we must translate the
    /// service port to the pod's targetPort before sending it to the
    /// kubelet. Otherwise the kubelet dials the pod on the service port
    /// (which it doesn't listen on) and the connection is refused.
    ///
    /// `kubectl port-forward svc/<name> LOCAL:SERVICEPORT` performs this
    /// translation internally; we replicate that here.
    async fn resolve_target_port(&self, target: &Target) -> anyhow::Result<u16> {
        match &target.port {
            Port::Number(port) => {
                let requested = u16::try_from(*port)
                    .map_err(|_| anyhow!("port number {} is out of range", port))?;
                // Translate via the service spec when this forward targets a
                // service. For pod/proxy workloads, the user already specified
                // the pod port directly — pass through.
                if let (Some(api), Some(name)) =
                    (self.service_api.as_ref(), self.service_name.as_ref())
                {
                    match resolve_service_target_port(api, name, requested).await {
                        Ok(translated) => Ok(translated),
                        Err(e) => {
                            // Service lookup failed (deleted, RBAC, etc.):
                            // fall back to the user-supplied port and let
                            // the kubelet surface a clearer error.
                            tracing::warn!(
                                service = %name,
                                requested,
                                error = %e,
                                "Failed to resolve service targetPort, using requested port as-is"
                            );
                            Ok(requested)
                        }
                    }
                } else {
                    Ok(requested)
                }
            }
            Port::Name(port_name) => {
                let ready = self
                    .forwarder
                    .ready_pod()
                    .ok_or_else(|| anyhow!("no ready pod to resolve port name '{}'", port_name))?;
                let pod = self
                    .pod_api
                    .get(&ready)
                    .await
                    .map_err(|e| anyhow!("failed to fetch pod {}: {}", ready, e))?;
                extract_named_port(&pod, port_name)
            }
        }
    }

    pub async fn initialize(&mut self, target: &Target) -> anyhow::Result<()> {
        if self.initialized.load(Ordering::Acquire) {
            return Ok(());
        }
        let _guard = self.initialization_mutex.lock().await;
        if self.initialized.load(Ordering::Acquire) {
            return Ok(());
        }
        let target_port = self.resolve_target_port(target).await?;
        self.target_port = Some(target_port);
        self.initialized.store(true, Ordering::Release);
        info!("Initialized WS port forwarder for port {}", target_port);
        Ok(())
    }

    pub async fn get_stream(&self) -> anyhow::Result<Upstream> {
        if !self.initialized.load(Ordering::Acquire) {
            return Err(anyhow!(
                "port forwarder not initialized - call initialize() first"
            ));
        }
        let target_port = self
            .target_port
            .ok_or_else(|| anyhow!("port forwarder not initialized"))?;
        let stream = self
            .forwarder
            .connect(target_port)
            .await
            .map_err(|e| anyhow!("failed to open local channel: {}", e))?;
        Ok(Upstream::Ws(stream))
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
                        break;
                    }
                }
            }
        });
        self.track_task(sync_task).await;

        let tcp_forwarder = TcpForwarder::new(config_id, workload_type);
        let forwarder_clone = Arc::clone(&self);
        let cancel_token = cancellation_token.clone();

        let mut consecutive_accept_errors: u32 = 0;
        const MAX_ACCEPT_ERRORS: u32 = 10;
        const BASE_BACKOFF_MS: u64 = 10;
        const MAX_BACKOFF_MS: u64 = 5000;
        let consecutive_stream_failures = Arc::new(std::sync::atomic::AtomicU32::new(0));
        const MAX_STREAM_FAILURES: u32 = 5;

        loop {
            let (client_conn, client_addr) = tokio::select! {
                result = listener.accept() => match result {
                    Ok(connection) => {
                        consecutive_accept_errors = 0;
                        TcpForwarder::apply_socket_optimizations(&connection.0);
                        connection
                    }
                    Err(e) => {
                        consecutive_accept_errors += 1;
                        let exponent = consecutive_accept_errors.saturating_sub(1).min(10);
                        let backoff_ms = std::cmp::min(
                            BASE_BACKOFF_MS.saturating_mul(1u64 << exponent),
                            MAX_BACKOFF_MS,
                        );
                        error!(
                            "Accept failed ({}/{}): {}, backing off {}ms",
                            consecutive_accept_errors, MAX_ACCEPT_ERRORS, e, backoff_ms
                        );
                        if consecutive_accept_errors >= MAX_ACCEPT_ERRORS {
                            return Err(anyhow!(
                                "TCP listener failed after {} consecutive accept errors: {}",
                                MAX_ACCEPT_ERRORS, e
                            ));
                        }
                        tokio::select! {
                            _ = tokio::time::sleep(tokio::time::Duration::from_millis(backoff_ms)) => {}
                            _ = cancel_token.cancelled() => break,
                        }
                        continue;
                    }
                },
                _ = cancel_token.cancelled() => break,
            };

            let forwarder = Arc::clone(&forwarder_clone);
            let mut tcp_forwarder = tcp_forwarder.clone();
            let http_log_watcher_clone = Arc::new(self.http_log_watcher.clone());
            let cancel_token_clone = cancel_token.clone();
            let tls_acceptor_clone = tls_acceptor.clone();
            let connection_tasks = Arc::clone(&self.connection_tasks);
            let stream_failures_clone = Arc::clone(&consecutive_stream_failures);

            let handle = crate::dataplane_runtime::spawn_on_dataplane(async move {
                let mut client_conn = client_conn;
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
                    if is_http_request(&client_conn).await {
                        if let Err(e) = handle_http_redirect(client_conn, port).await {
                            debug!("Failed to redirect HTTP to HTTPS: {}", e);
                        }
                        return;
                    }
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
                let mut tasks = connection_tasks.lock().await;
                tasks.retain(|h| !h.is_finished());
                tasks.push(handle);
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
            return Err(anyhow!("Network config failed: {}", e));
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
                self.start_udp_listener(
                    listener_config,
                    config_id,
                    workload_type,
                    cancellation_token,
                )
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
                anyhow!(
                    "Invalid IP address '{}': {}",
                    listener_config.local_address,
                    e
                )
            })?;
        let addr = std::net::SocketAddr::new(ip, listener_config.local_port);
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| anyhow!("Failed to bind TCP listener to {}: {}", addr, e))?;
        let port = listener.local_addr()?.port();

        // Eagerly validate upstream connectivity (same pattern as UDP).
        // The stream is dropped immediately; the underlying session and
        // spare-stream queue stay warm for real traffic.
        self.get_stream().await.map_err(|e| {
            anyhow!(
                "Failed to validate upstream connection for TCP port {}: {}",
                port,
                e
            )
        })?;

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
        self: Arc<Self>, listener_config: ListenerConfig, config_id: i64, workload_type: String,
        cancellation_token: CancellationToken,
    ) -> anyhow::Result<(u16, JoinHandle<anyhow::Result<()>>)> {
        if workload_type == "service" || workload_type == "proxy" {
            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
        }
        let upstream_stream = self
            .get_stream()
            .await
            .map_err(|e| anyhow!("Failed to get upstream connection for UDP: {}", e))?;
        let signal_token = cancellation_token.clone();
        let (port, handle) = UdpForwarder::bind_and_forward(
            listener_config.local_address,
            listener_config.local_port,
            upstream_stream,
            cancellation_token,
        )
        .await?;
        let result_handle = tokio::spawn(async move {
            let result = handle
                .await
                .map_err(|e| anyhow!("UDP forwarding task failed: {}", e));
            if !signal_token.is_cancelled()
                && let Some(rm) = crate::kube::proxy_recovery::RECOVERY_MANAGERS.get(&config_id)
            {
                log::info!(
                    "UDP forwarder task completed, signaling recovery for config_id={}",
                    config_id
                );
                rm.signal_recovery(crate::kube::proxy_recovery::RecoverySignal::StreamFailed);
            }
            result?;
            Ok(())
        });
        Ok((port, result_handle))
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

    async fn track_task(&self, handle: JoinHandle<()>) {
        self.background_tasks.lock().await.push(handle);
    }

    async fn cleanup_background_tasks(&self) {
        let mut tasks = self.background_tasks.lock().await;
        for h in tasks.drain(..) {
            h.abort();
        }
    }

    async fn cleanup_connection_tasks(&self) {
        let mut tasks = self.connection_tasks.lock().await;
        let count = tasks.len();
        for h in tasks.drain(..) {
            h.abort();
        }
        if count > 0 {
            debug!("Aborted {} active connection tasks", count);
        }
    }

    pub async fn get_current_active_pod(&self) -> Option<String> {
        self.forwarder.ready_pod()
    }

    pub async fn shutdown(&self) {
        info!("Shutting down port forwarder");
        self.http_log_watcher.shutdown();
        // Cancel the token first so active connections can observe cancellation
        // and send graceful v5 close frames before being aborted.
        self.forwarder.cancellation_token().cancel();
        // Brief grace period for cancellation-aware tasks to finish cleanup.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        self.cleanup_background_tasks().await;
        self.cleanup_connection_tasks().await;
    }
}

async fn resolve_label_selector(
    client: &kube::Client, namespace: &str, selector: &TargetSelector,
) -> anyhow::Result<String> {
    match selector {
        TargetSelector::PodLabel(s) => Ok(s.clone()),
        TargetSelector::ServiceName(name) => {
            let api: Api<Service> = Api::namespaced(client.clone(), namespace);
            let svc = api
                .get(name)
                .await
                .map_err(|e| anyhow!("service '{}' not found: {}", name, e))?;
            let labels = svc
                .spec
                .as_ref()
                .and_then(|s| s.selector.as_ref())
                .ok_or_else(|| anyhow!("service '{}' has no selector", name))?;
            let mut out = String::new();
            let mut first = true;
            for (k, v) in labels {
                if !first {
                    out.push(',');
                }
                out.push_str(k);
                out.push('=');
                out.push_str(v);
                first = false;
            }
            Ok(out)
        }
    }
}

/// Resolve `requested` (a Service `port`) to the pod container port it
/// maps to (`targetPort`).
///
/// Matches `kubectl port-forward svc/<name>` translation. If the Service
/// has a matching port entry:
///   - numeric `targetPort` → return it directly
///   - named `targetPort` → look it up in any pod matching the service selector
///   - missing `targetPort` → falls back to the service `port` itself
///     (Kubernetes default)
///
/// If no port entry matches, returns the requested port unchanged so the
/// kubelet's eventual "connection refused" surfaces with a useful port
/// number in its error message instead of being swallowed here.
async fn resolve_service_target_port(
    api: &Api<Service>, name: &str, requested: u16,
) -> anyhow::Result<u16> {
    use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;

    let svc = api
        .get(name)
        .await
        .map_err(|e| anyhow!("failed to fetch service {}: {}", name, e))?;
    let spec = svc
        .spec
        .as_ref()
        .ok_or_else(|| anyhow!("service {} has no spec", name))?;
    let ports = spec
        .ports
        .as_ref()
        .ok_or_else(|| anyhow!("service {} has no ports", name))?;

    let matching = ports
        .iter()
        .find(|p| p.port as u32 == requested as u32)
        .ok_or_else(|| anyhow!("service {} has no port entry for {}", name, requested))?;

    let target_port = match matching.target_port.as_ref() {
        // Per Kubernetes API: omitted targetPort defaults to the service port.
        None => requested,
        Some(IntOrString::Int(n)) => u16::try_from(*n)
            .map_err(|_| anyhow!("service {} targetPort {} out of range", name, n))?,
        Some(IntOrString::String(named)) => {
            // Named targetPort: look it up in a pod matching the service selector.
            // Get the pod api from the namespace path the service lives in.
            let namespace = svc
                .metadata
                .namespace
                .as_deref()
                .ok_or_else(|| anyhow!("service {} has no namespace", name))?;
            let selector = spec.selector.as_ref().ok_or_else(|| {
                anyhow!(
                    "service {} has named targetPort {:?} but no selector",
                    name,
                    named
                )
            })?;
            let selector_str = selector
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(",");
            let client = api.clone().into_client();
            let pod_api: Api<Pod> = Api::namespaced(client, namespace);
            let lp = kube::api::ListParams::default().labels(&selector_str);
            let pods = pod_api.list(&lp).await.map_err(|e| {
                anyhow!(
                    "failed to list pods for service {} named targetPort {:?}: {}",
                    name,
                    named,
                    e
                )
            })?;
            let pod = pods
                .items
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("no pods match service {} selector", name))?;
            extract_named_port(&pod, named)?
        }
    };

    if target_port != requested {
        tracing::info!(
            service = %name,
            service_port = requested,
            target_port,
            "Resolved service port to pod targetPort"
        );
    }
    Ok(target_port)
}

fn extract_named_port(pod: &Pod, name: &str) -> anyhow::Result<u16> {
    pod.spec
        .as_ref()
        .and_then(|spec| {
            spec.containers
                .iter()
                .filter_map(|c| c.ports.as_ref())
                .flatten()
                .find(|p| p.name.as_deref() == Some(name))
                .map(|p| p.container_port as u16)
        })
        .ok_or_else(|| anyhow!("named port '{}' not found in pod", name))
}

async fn is_http_request(client_conn: &TcpStream) -> bool {
    let mut peek_buf = [0u8; 64];
    let peek_result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        client_conn.peek(&mut peek_buf),
    )
    .await;
    match peek_result {
        Ok(Ok(_)) => {
            let mut headers = [httparse::EMPTY_HEADER; 4];
            let mut req = Request::new(&mut headers);
            matches!(
                req.parse(&peek_buf),
                Ok(httparse::Status::Complete(_)) | Ok(httparse::Status::Partial)
            )
        }
        _ => false,
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

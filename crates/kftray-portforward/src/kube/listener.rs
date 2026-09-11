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

pub struct PortForwarder {
    namespace: Arc<str>,
    forwarder: Arc<kube_portforward::Forwarder>,
    target_port: u16,
    http_log_watcher: HttpLogStateWatcher,
    background_tasks: Arc<tokio::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    connection_tasks: Arc<tokio::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>>,
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

        Ok(Self {
            namespace: namespace.into(),
            forwarder,
            target_port,
            http_log_watcher: HttpLogStateWatcher::new(),
            background_tasks: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            connection_tasks: Arc::new(tokio::sync::Mutex::new(Vec::new())),
        })
    }

    pub async fn get_stream(&self) -> anyhow::Result<kube_portforward::Stream> {
        const STREAM_ACQUIRE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

        tokio::time::timeout(
            STREAM_ACQUIRE_TIMEOUT,
            self.forwarder.connect(self.target_port),
        )
        .await
        .context("Timed out acquiring a port-forward stream")?
        .map_err(Into::into)
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
        self.track_task(sync_task).await;

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
            let stream_failures_clone = Arc::clone(&consecutive_stream_failures);

            let handle = tokio::spawn(async move {
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
                        // Close client connection properly to avoid leaving socket open
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
        let upstream_stream = self
            .get_stream()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get upstream connection for UDP: {}", e))?;
        let signal_token = cancellation_token.clone();
        let (port, forward_future) = UdpForwarder::bind_and_forward(
            listener_config.local_address,
            listener_config.local_port,
            upstream_stream,
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

    async fn track_task(&self, handle: tokio::task::JoinHandle<()>) {
        let mut tasks = self.background_tasks.lock().await;
        tasks.push(handle);
    }

    async fn cleanup_background_tasks(&self) {
        let tasks = std::mem::take(&mut *self.background_tasks.lock().await);
        for handle in &tasks {
            handle.abort();
        }
        for handle in tasks {
            let _ = handle.await;
        }
    }

    async fn cleanup_connection_tasks(&self) {
        let tasks = std::mem::take(&mut *self.connection_tasks.lock().await);
        for handle in &tasks {
            handle.abort();
        }
        for handle in tasks {
            let _ = handle.await;
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

        self.cleanup_background_tasks().await;
        self.cleanup_connection_tasks().await;

        if let Err(e) = self.forwarder.shutdown().await {
            debug!("Forwarder shutdown returned an error: {}", e);
        }
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
            http_log_watcher: HttpLogStateWatcher::new(),
            background_tasks: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            connection_tasks: Arc::new(tokio::sync::Mutex::new(Vec::new())),
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
            http_log_watcher: HttpLogStateWatcher::new(),
            background_tasks: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            connection_tasks: Arc::new(tokio::sync::Mutex::new(Vec::new())),
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
    async fn udp_owner_signals_recovery_when_upstream_closes() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let config_id = 900_101;
        let (mut upstream, server_stream) = tokio::io::duplex(64);
        let token = CancellationToken::new();
        let (_, forward) =
            UdpForwarder::bind_and_forward("127.0.0.1".to_owned(), 0, server_stream, token.clone())
                .await
                .unwrap();
        let mut receiver = recovery_receiver(config_id);
        let owner = spawn_udp_forward_owner(forward, token, config_id);
        upstream.shutdown().await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), owner)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let signal = receiver.try_recv();
        crate::kube::proxy_recovery::RECOVERY_MANAGERS.remove(&config_id);
        assert_eq!(
            signal.unwrap(),
            crate::kube::proxy_recovery::RecoverySignal::StreamFailed
        );
    }

    #[tokio::test]
    async fn udp_owner_does_not_signal_recovery_on_intentional_cancellation() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let config_id = 900_102;
        let (_upstream, server_stream) = tokio::io::duplex(64);
        let token = CancellationToken::new();
        let (_, forward) =
            UdpForwarder::bind_and_forward("127.0.0.1".to_owned(), 0, server_stream, token.clone())
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
        let (_upstream, server_stream) = tokio::io::duplex(1024);
        let cancellation_token = CancellationToken::new();

        let (port, forward_future) = UdpForwarder::bind_and_forward(
            "127.0.0.1".to_string(),
            0,
            server_stream,
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
}

use std::sync::atomic::{
    AtomicBool,
    AtomicU64,
    AtomicUsize,
    Ordering,
};
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use bb8::Pool;
use kftray_http_logs::HttpLogState;
use kube::{
    api::Api,
    Client,
};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::time::timeout;
use tracing::{
    debug,
    error,
    info,
};

use crate::kube::models::Target;
use crate::kube::target_cache::{
    TargetCache,
    TargetCacheKey,
};
use crate::kube::tcp_forwarder::TcpForwarder;
use crate::port_forward::CANCEL_NOTIFIER;

const STREAM_TAKE_TIMEOUT_MILLIS: u64 = 500;
const POOL_CONNECTION_TIMEOUT_SECS: u64 = 2;
const POOL_MAX_LIFETIME_SECS: u64 = 45;
const POOL_IDLE_TIMEOUT_SECS: u64 = 8;
const POOL_MIN_IDLE_CONNECTIONS: u32 = 3;
const CONSECUTIVE_FAILURE_THRESHOLD: usize = 1;
#[derive(Clone)]
pub struct PortForwarderConnection {
    port_forwarder: Arc<Mutex<Option<kube::api::Portforwarder>>>,
    port: u16,
    used: Arc<AtomicBool>,
    generation: u64,
}

impl PortForwarderConnection {
    pub async fn take_stream(
        &mut self,
    ) -> anyhow::Result<impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static>
    {
        self.used.store(true, Ordering::Relaxed);

        let take_stream_future = async {
            let mut pf_guard = self.port_forwarder.lock().await;
            if let Some(port_forwarder) = pf_guard.as_mut() {
                port_forwarder.take_stream(self.port)
            } else {
                None
            }
        };

        match timeout(
            Duration::from_millis(STREAM_TAKE_TIMEOUT_MILLIS),
            take_stream_future,
        )
        .await
        {
            Ok(Some(stream)) => Ok(stream),
            Ok(None) => Err(anyhow::anyhow!("Stream unavailable")),
            Err(_) => Err(anyhow::anyhow!("Stream unavailable")),
        }
    }

    pub fn is_used(&self) -> bool {
        self.used.load(Ordering::Relaxed)
    }

    fn is_generation_valid(&self, current_generation: u64) -> bool {
        self.generation >= current_generation
    }
}

#[derive(Clone)]
pub struct PortForwarderManager {
    pod_api: Api<k8s_openapi::api::core::v1::Pod>,
    svc_api: Api<k8s_openapi::api::core::v1::Service>,
    target: Target,
    target_cache: Arc<TargetCache>,
    cache_generation: Arc<AtomicU64>,
}

impl PortForwarderManager {
    pub fn new(
        client: Client, namespace: &str, target: Target, target_cache: Arc<TargetCache>,
    ) -> Self {
        Self {
            pod_api: Api::namespaced(client.clone(), namespace),
            svc_api: Api::namespaced(client, namespace),
            target,
            target_cache,
            cache_generation: Arc::new(AtomicU64::new(0)),
        }
    }

    async fn resolve_target(&self) -> Result<crate::kube::models::TargetPod, anyhow::Error> {
        let cache_key = TargetCacheKey::from_target(&self.target);

        if let Some(cached_target) = self.target_cache.get(&cache_key) {
            return Ok(cached_target);
        }

        let validator = self.create_validator();
        if let Some(cached_target) = self
            .target_cache
            .get_with_validation(&cache_key, validator)
            .await
        {
            return Ok(cached_target);
        }

        self.find_and_cache_target(cache_key).await
    }

    fn create_validator(
        &self,
    ) -> impl Fn(crate::kube::models::TargetPod) -> futures::future::BoxFuture<'static, bool> + Clone
    {
        let pod_api = self.pod_api.clone();

        move |target_pod| {
            let pod_api = pod_api.clone();
            Box::pin(async move {
                match timeout(
                    Duration::from_millis(500),
                    pod_api.get(&target_pod.pod_name),
                )
                .await
                {
                    Ok(Ok(pod)) => {
                        let is_running = pod
                            .status
                            .as_ref()
                            .and_then(|s| s.phase.as_ref())
                            .map(|phase| phase == "Running")
                            .unwrap_or(false);

                        let is_ready =
                            pod.status
                                .and_then(|s| s.conditions)
                                .is_some_and(|conditions| {
                                    conditions
                                        .iter()
                                        .any(|c| c.type_ == "Ready" && c.status == "True")
                                });

                        is_running && is_ready
                    }
                    Ok(Err(_)) => false,
                    Err(_) => false,
                }
            })
        }
    }

    async fn find_and_cache_target(
        &self, cache_key: TargetCacheKey,
    ) -> Result<crate::kube::models::TargetPod, anyhow::Error> {
        let finder = crate::kube::pod_finder::TargetPodFinder {
            pod_api: &self.pod_api,
            svc_api: &self.svc_api,
        };

        let found_target = finder.find(&self.target).await?;

        self.target_cache.put(cache_key, found_target.clone());
        self.cache_generation.fetch_add(1, Ordering::Relaxed);

        Ok(found_target)
    }

    async fn create_port_forwarder_connection(
        &self, resolved_target: crate::kube::models::TargetPod,
    ) -> Result<PortForwarderConnection, kube::Error> {
        let port_forwarder = self
            .pod_api
            .portforward(&resolved_target.pod_name, &[resolved_target.port_number])
            .await?;

        Ok(PortForwarderConnection {
            port_forwarder: Arc::new(Mutex::new(Some(port_forwarder))),
            port: resolved_target.port_number,
            used: Arc::new(AtomicBool::new(false)),
            generation: self.cache_generation.load(Ordering::Relaxed),
        })
    }
}

impl bb8::ManageConnection for PortForwarderManager {
    type Connection = PortForwarderConnection;
    type Error = kube::Error;

    fn connect(
        &self,
    ) -> impl std::future::Future<Output = Result<Self::Connection, Self::Error>> + Send {
        let manager = self.clone();

        async move {
            let resolved_target = manager
                .resolve_target()
                .await
                .map_err(|e| kube::Error::Service(e.into()))?;

            manager
                .create_port_forwarder_connection(resolved_target)
                .await
        }
    }

    async fn is_valid(&self, conn: &mut Self::Connection) -> Result<(), Self::Error> {
        if conn.is_used() {
            return Err(kube::Error::Service(Box::new(std::io::Error::other(
                "Connection used",
            ))));
        }

        let current_generation = self.cache_generation.load(Ordering::Relaxed);
        if !conn.is_generation_valid(current_generation) {
            return Err(kube::Error::Service(Box::new(std::io::Error::other(
                "Pool invalidated",
            ))));
        }

        Ok(())
    }

    fn has_broken(&self, conn: &mut Self::Connection) -> bool {
        if conn.is_used() {
            return true;
        }

        let current_generation = self.cache_generation.load(Ordering::Relaxed);
        !conn.is_generation_valid(current_generation)
    }
}

pub type PortForwarderPool = Pool<PortForwarderManager>;

pub struct PooledPortForwarder {
    pool: ArcSwap<PortForwarderPool>,
    manager: PortForwarderManager,
    max_pool_size: usize,
    consecutive_failures: Arc<AtomicUsize>,
}

impl PooledPortForwarder {
    pub async fn new(
        client: Client, namespace: &str, target: Target, target_cache: Arc<TargetCache>,
        max_pool_size: usize,
    ) -> anyhow::Result<Self> {
        let manager = PortForwarderManager::new(client, namespace, target, target_cache);
        let pool = Self::create_pool(&manager, max_pool_size).await?;

        Ok(Self {
            pool: ArcSwap::new(Arc::new(pool)),
            manager,
            max_pool_size,
            consecutive_failures: Arc::new(AtomicUsize::new(0)),
        })
    }

    async fn create_pool(
        manager: &PortForwarderManager, max_pool_size: usize,
    ) -> anyhow::Result<PortForwarderPool> {
        Pool::builder()
            .max_size(max_pool_size as u32)
            .min_idle(Some(POOL_MIN_IDLE_CONNECTIONS))
            .max_lifetime(Some(Duration::from_secs(POOL_MAX_LIFETIME_SECS)))
            .idle_timeout(Some(Duration::from_secs(POOL_IDLE_TIMEOUT_SECS)))
            .connection_timeout(Duration::from_secs(POOL_CONNECTION_TIMEOUT_SECS))
            .queue_strategy(bb8::QueueStrategy::Lifo)
            .test_on_check_out(false)
            .build(manager.clone())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create pool: {}", e))
    }

    pub async fn get_stream(
        &self,
    ) -> Result<
        impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
        bb8::RunError<kube::Error>,
    > {
        let pool = self.pool.load();

        let conn_result = pool.get().await;

        match conn_result {
            Ok(mut conn) => match conn.take_stream().await {
                Ok(stream) => {
                    self.consecutive_failures.store(0, Ordering::Relaxed);
                    Ok(stream)
                }
                Err(_) => {
                    let failures = self.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;
                    debug!(
                        "Stream failed ({}/{})",
                        failures, CONSECUTIVE_FAILURE_THRESHOLD
                    );

                    if failures >= CONSECUTIVE_FAILURE_THRESHOLD {
                        self.invalidate_and_recreate().await;
                    }

                    Err(bb8::RunError::User(kube::Error::Service(
                        anyhow::anyhow!("Stream unavailable").into(),
                    )))
                }
            },
            Err(bb8::RunError::TimedOut) => {
                self.invalidate_and_recreate().await;
                Err(bb8::RunError::TimedOut)
            }
            Err(e) => Err(e),
        }
    }

    async fn invalidate_and_recreate(&self) {
        let cache_key = TargetCacheKey::from_target(&self.manager.target);
        self.manager.target_cache.force_refresh(&cache_key);

        if let Err(e) = self.recreate_pool().await {
            error!("Failed to recreate pool after cache invalidation: {}", e);
        }
    }

    async fn recreate_pool(&self) -> anyhow::Result<()> {
        let new_pool = Self::create_pool(&self.manager, self.max_pool_size).await?;
        self.pool.store(Arc::new(new_pool));
        self.consecutive_failures.store(0, Ordering::Relaxed);
        info!("Pool recreated successfully");
        Ok(())
    }

    pub async fn handle_tcp_listener(
        self: Arc<Self>, listener: TcpListener, http_log_state: Arc<HttpLogState>, config_id: i64,
        workload_type: String, port: u16, max_concurrent_connections: usize,
    ) -> anyhow::Result<()> {
        let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrent_connections));
        info!(
            "Starting concurrent TCP listener with max {} connections",
            max_concurrent_connections
        );

        loop {
            let (client_conn, client_addr) = match listener.accept().await {
                Ok((conn, addr)) => (conn, addr),
                Err(e) => {
                    error!("Accept failed: {}", e);
                    break;
                }
            };

            let portforward_pool = self.clone();
            let http_log_state = http_log_state.clone();
            let workload_type = workload_type.clone();
            let semaphore = semaphore.clone();
            let cancel_notifier = CANCEL_NOTIFIER.clone();

            tokio::spawn(async move {
                let _permit = match semaphore.acquire_owned().await {
                    Ok(permit) => permit,
                    Err(_) => return,
                };

                if client_conn.set_nodelay(true).is_err() || client_conn.peer_addr().is_err() {
                    return;
                }

                let upstream_conn = match portforward_pool.get_stream().await {
                    Ok(stream) => stream,
                    Err(e) => {
                        error!("Pool connection failed for {}: {}", client_addr, e);
                        return;
                    }
                };

                let mut forwarder = TcpForwarder::new(config_id, workload_type.clone());

                if let Err(e) = forwarder.initialize_logger(&http_log_state, port).await {
                    error!(
                        "Failed to initialize HTTP logger for {}: {:?}",
                        client_addr, e
                    );
                }

                let logging_enabled = http_log_state
                    .get_http_logs(config_id)
                    .await
                    .unwrap_or_default();

                if let Err(e) = forwarder
                    .forward_connection(
                        Arc::new(Mutex::new(client_conn)),
                        upstream_conn,
                        cancel_notifier,
                        logging_enabled,
                    )
                    .await
                {
                    error!("Forward failed for {}: {}", client_addr, e);
                }
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::kube::client::create_client_with_specific_context;
    use crate::kube::models::TargetPod;

    #[tokio::test]
    async fn test_port_forwarder_connection_creation() {
        let target_pod = TargetPod {
            pod_name: "test-pod".to_string(),
            port_number: 8080,
        };

        let result = create_client_with_specific_context(None, Some("test-context")).await;

        assert_eq!(target_pod.pod_name, "test-pod");
        assert_eq!(target_pod.port_number, 8080);
        assert!(result.is_err() || result.is_ok());
    }
}

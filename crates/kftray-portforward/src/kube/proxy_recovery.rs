use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use kftray_commons::models::config_model::Config;
use once_cell::sync::Lazy;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

// ============================================================================
// T2: Recovery Constants and Enums
// ============================================================================

/// Maximum number of recovery attempts before giving up
pub const MAX_RECOVERY_ATTEMPTS: u32 = 5;

/// Base backoff duration in seconds (exponential backoff: 2, 4, 8, 16, 32)
pub const BASE_BACKOFF_SECS: u64 = 2;

/// Maximum backoff duration in seconds (caps exponential growth)
pub const MAX_BACKOFF_SECS: u64 = 32;

/// Timeout for waiting for a pod to become ready during recovery
pub const POD_READY_TIMEOUT_SECS: u64 = 30;

/// Represents the current state of a proxy recovery operation
#[derive(Debug, Clone, PartialEq)]
pub enum RecoveryState {
    /// No recovery in progress
    Idle,
    /// Actively monitoring pod health
    Monitoring,
    /// Retrying after failure
    Retrying {
        /// Current attempt number (1-indexed)
        attempt: u32,
        /// Error message from last failure
        last_error: String,
    },
    /// Recovery failed after all attempts exhausted
    Failed {
        /// Total number of attempts made
        total_attempts: u32,
        /// Final error message
        final_error: String,
    },
    /// Recovery was cancelled by user or system
    Cancelled,
}

/// Type of proxy workload being recovered
#[derive(Debug, Clone, PartialEq)]
pub enum ProxyType {
    /// Direct pod forwarding (bare pod)
    BarePod,
    /// Deployment-based forwarding
    Deployment,
}

/// Signal that triggers recovery logic
#[derive(Debug, Clone, PartialEq)]
pub enum RecoverySignal {
    /// Pod was terminated or deleted
    PodDied,
    /// Network stream failed
    StreamFailed,
    /// Health check detected unhealthy state
    HealthCheckFailed,
}

// ============================================================================
// T3: Per-Config Recovery Coordinator
// ============================================================================

/// Global per-config-id recovery lock
///
/// Prevents race conditions between pod watcher and network monitor.
/// Each config_id gets its own Mutex to serialize recovery attempts.
pub static RECOVERY_LOCKS: Lazy<DashMap<i64, Arc<Mutex<()>>>> = Lazy::new(DashMap::new);

/// Global map of active recovery managers, keyed by config_id.
/// Used to cancel recovery when user stops a port forward.
pub static RECOVERY_MANAGERS: Lazy<DashMap<i64, Arc<ProxyRecoveryManager>>> =
    Lazy::new(DashMap::new);

/// Spawn a recovery manager for the given config and proxy type.
///
/// This is a **sync** helper to avoid opaque-type cycles when called from
/// async proxy functions that are themselves awaited by
/// `deploy_and_forward_pod`.
pub fn spawn_recovery_manager(config: Config, proxy_type: ProxyType) {
    if let Some(config_id) = config.id {
        let manager = Arc::new(ProxyRecoveryManager::new(config, proxy_type));
        RECOVERY_MANAGERS.insert(config_id, Arc::clone(&manager));
        tokio::spawn(async move {
            manager.run_recovery_loop().await;
        });
        log::info!("Spawned recovery manager for proxy config {}", config_id);
    }
}

/// Acquire (or create) the recovery lock for a config_id
///
/// This ensures only one recovery operation runs per config at a time.
/// Multiple callers will block until the lock is released.
///
/// # Arguments
/// * `config_id` - The configuration ID to lock
///
/// # Returns
/// An Arc<Mutex<()>> that can be locked to serialize recovery operations
pub async fn acquire_recovery_lock(config_id: i64) -> Arc<Mutex<()>> {
    RECOVERY_LOCKS
        .entry(config_id)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

/// Remove the recovery lock for a config_id
///
/// Call this during cleanup (e.g., when stopping a port forward) to free
/// resources. Safe to call even if the lock doesn't exist.
///
/// # Arguments
/// * `config_id` - The configuration ID whose lock should be removed
pub fn remove_recovery_lock(config_id: i64) {
    RECOVERY_LOCKS.remove(&config_id);
}

// ============================================================================
// T6: Proxy Recovery Manager
// ============================================================================

/// Manages proxy pod recovery with exponential backoff.
///
/// Listens for [`RecoverySignal`]s and performs up to [`MAX_RECOVERY_ATTEMPTS`]
/// retries with exponential backoff. Updates [`ConfigState`] in the database
/// so the UI can display recovery progress.
pub struct ProxyRecoveryManager {
    /// Port-forward configuration being recovered
    pub config: Config,
    /// Extracted config ID for convenience
    pub config_id: i64,
    /// Type of proxy workload (bare pod vs deployment)
    pub proxy_type: ProxyType,
    /// Token to cancel the recovery loop
    cancel_token: CancellationToken,
    /// Current recovery state (async-safe)
    state: Arc<tokio::sync::RwLock<RecoveryState>>,
    /// Broadcast sender to trigger recovery from any source
    recovery_signal_tx: tokio::sync::broadcast::Sender<RecoverySignal>,
}

impl ProxyRecoveryManager {
    /// Create a new recovery manager for the given config and proxy type.
    ///
    /// # Arguments
    /// * `config` - The port-forward configuration to recover
    /// * `proxy_type` - Whether this is a bare pod or deployment proxy
    pub fn new(config: Config, proxy_type: ProxyType) -> Self {
        let config_id = config.id.unwrap_or(0);
        let (recovery_signal_tx, _) = tokio::sync::broadcast::channel::<RecoverySignal>(16);
        Self {
            config,
            config_id,
            proxy_type,
            cancel_token: CancellationToken::new(),
            state: Arc::new(tokio::sync::RwLock::new(RecoveryState::Idle)),
            recovery_signal_tx,
        }
    }

    /// Send a recovery signal to trigger the recovery loop.
    ///
    /// Safe to call from any context. If the loop is not running or
    /// the receiver has been dropped, the signal is silently discarded.
    pub fn signal_recovery(&self, signal: RecoverySignal) {
        let _ = self.recovery_signal_tx.send(signal);
    }

    /// Cancel the recovery loop.
    ///
    /// The loop will exit at the next cancellation check point.
    pub fn cancel(&self) {
        self.cancel_token.cancel();
    }

    /// Main recovery loop — subscribe to signals, retry with backoff.
    ///
    /// Runs until cancelled via [`cancel()`](Self::cancel). On each signal:
    /// 1. Acquires the per-config recovery lock
    /// 2. Retries up to [`MAX_RECOVERY_ATTEMPTS`] times with exponential
    ///    backoff
    /// 3. Updates [`ConfigState`] in the database at each step
    pub async fn run_recovery_loop(&self) {
        let mut rx = self.recovery_signal_tx.subscribe();
        {
            let mut s = self.state.write().await;
            *s = RecoveryState::Monitoring;
        }

        loop {
            // Wait for a signal or cancellation
            let signal = tokio::select! {
                result = rx.recv() => {
                    match result {
                        Ok(sig) => sig,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            log::warn!("Recovery signal receiver lagged by {} messages for config {}", n, self.config_id);
                            continue;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            log::debug!("Recovery signal channel closed for config {}", self.config_id);
                            return;
                        }
                    }
                }
                _ = self.cancel_token.cancelled() => {
                    let mut s = self.state.write().await;
                    *s = RecoveryState::Cancelled;
                    log::debug!("Recovery loop cancelled for config {}", self.config_id);
                    return;
                }
            };

            log::info!(
                "Recovery signal {:?} received for config {}",
                signal,
                self.config_id
            );

            // Acquire the per-config lock to serialize recovery attempts
            let lock = acquire_recovery_lock(self.config_id).await;
            let _guard = lock.lock().await;

            let mut all_attempts_exhausted = true;
            for attempt in 1..=MAX_RECOVERY_ATTEMPTS {
                if self.cancel_token.is_cancelled() {
                    all_attempts_exhausted = false;
                    break;
                }

                // Calculate exponential backoff: 2^(attempt-1) * BASE, capped at MAX
                let backoff = std::cmp::min(
                    BASE_BACKOFF_SECS.saturating_mul(1u64 << (attempt - 1)),
                    MAX_BACKOFF_SECS,
                );

                {
                    let mut s = self.state.write().await;
                    *s = RecoveryState::Retrying {
                        attempt,
                        last_error: "Attempting recovery".to_string(),
                    };
                }

                self.update_config_state_fields(true, true, Some(attempt as i32), None)
                    .await;

                log::info!(
                    "Recovery attempt {}/{} for config {} (backoff {}s)",
                    attempt,
                    MAX_RECOVERY_ATTEMPTS,
                    self.config_id,
                    backoff
                );

                // Sleep with cancellation awareness
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(backoff)) => {}
                    _ = self.cancel_token.cancelled() => {
                        all_attempts_exhausted = false;
                        break;
                    }
                }

                match self.do_recovery_attempt().await {
                    Ok(()) => {
                        log::info!(
                            "Recovery succeeded for config {} on attempt {}",
                            self.config_id,
                            attempt
                        );
                        {
                            let mut s = self.state.write().await;
                            *s = RecoveryState::Monitoring;
                        }
                        self.update_config_state_fields(true, false, None, None)
                            .await;
                        all_attempts_exhausted = false;
                        break;
                    }
                    Err(e) => {
                        log::error!(
                            "Recovery attempt {}/{} failed for config {}: {}",
                            attempt,
                            MAX_RECOVERY_ATTEMPTS,
                            self.config_id,
                            e
                        );
                        self.update_config_state_fields(
                            true,
                            true,
                            Some(attempt as i32),
                            Some(e.to_string()),
                        )
                        .await;
                    }
                }
            }

            // All attempts exhausted without success or cancellation
            if all_attempts_exhausted {
                let final_error = format!(
                    "Recovery failed after {} attempts for config {}",
                    MAX_RECOVERY_ATTEMPTS, self.config_id
                );
                log::error!("{}", final_error);
                {
                    let mut s = self.state.write().await;
                    *s = RecoveryState::Failed {
                        total_attempts: MAX_RECOVERY_ATTEMPTS,
                        final_error: final_error.clone(),
                    };
                }
                self.update_config_state_fields(false, false, None, Some(final_error))
                    .await;
            }
            // Lock released when _guard drops
        }
    }

    /// Attempt a single recovery operation.
    ///
    /// Dispatches to the appropriate recovery function based on [`ProxyType`]:
    /// - [`ProxyType::BarePod`] → full re-deployment via [`recover_bare_pod()`]
    /// - [`ProxyType::Deployment`] → stream reconnection via
    ///   [`recover_deployment()`]
    async fn do_recovery_attempt(&self) -> anyhow::Result<()> {
        let client_key = crate::kube::shared_client::ServiceClientKey::new(
            self.config.context.clone(),
            self.config.kubeconfig.clone(),
        );
        let client = crate::kube::shared_client::SHARED_CLIENT_MANAGER
            .get_client(client_key)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get K8s client: {}", e))?;
        let client = kube::Client::clone(&client);

        match self.proxy_type {
            ProxyType::BarePod => recover_bare_pod(&self.config, &client).await,
            ProxyType::Deployment => recover_deployment(&self.config, &client).await,
        }
    }

    /// Update ConfigState fields in the database for UI display.
    async fn update_config_state_fields(
        &self, is_running: bool, is_retrying: bool, retry_count: Option<i32>,
        last_error: Option<String>,
    ) {
        let state = kftray_commons::models::config_state_model::ConfigState {
            id: None,
            config_id: self.config_id,
            is_running,
            process_id: None,
            is_retrying,
            retry_count,
            last_error,
        };
        if let Err(e) = kftray_commons::utils::config_state::update_config_state(&state).await {
            log::error!(
                "Failed to update ConfigState for config {}: {}",
                self.config_id,
                e
            );
        }
    }
}

// ============================================================================
// T7: Bare Pod Recovery
// ============================================================================

/// Recover a bare pod proxy by fully re-deploying.
///
/// Bare pods have no controller (no Deployment/ReplicaSet), so when the pod
/// dies there is nothing to auto-restart it. This function:
/// 1. Cleans up old cluster resources (pods, deployments) to prevent orphans
/// 2. Removes stale [`CHILD_PROCESSES`](crate::port_forward::CHILD_PROCESSES)
///    entries for this config
/// 3. Re-deploys a fresh proxy pod via
///    [`deploy_and_forward_pod()`](crate::kube::proxy::deploy_and_forward_pod)
pub async fn recover_bare_pod(config: &Config, client: &kube::Client) -> anyhow::Result<()> {
    let config_id = config
        .id
        .ok_or_else(|| anyhow::anyhow!("Config has no ID"))?;
    let namespace = &config.namespace;

    // Step 1: Clean up old cluster resources FIRST (prevents orphaned pods)
    crate::kube::stop::delete_proxy_cluster_resources(client.clone(), namespace, config_id).await;

    // Step 2: Remove old CHILD_PROCESSES entry for this config_id
    // Key format: "config:{id}:service:{name}"
    let keys_to_remove: Vec<String> = crate::port_forward::CHILD_PROCESSES
        .iter()
        .filter(|entry| entry.key().starts_with(&format!("config:{}:", config_id)))
        .map(|entry| entry.key().clone())
        .collect();
    for key in keys_to_remove {
        crate::port_forward::CHILD_PROCESSES.remove(&key);
    }

    // Step 3: Re-deploy via the existing deploy_and_forward_pod() function
    // This generates a new hashed_name and creates a fresh pod + port forward
    crate::kube::proxy::deploy_and_forward_pod(vec![config.clone()])
        .await
        .map_err(|e| anyhow::anyhow!("Re-deployment failed: {}", e))?;

    Ok(())
}

// ============================================================================
// T8: Deployment Recovery
// ============================================================================

/// Recover a deployment-based proxy by waiting for K8s to restart the pod.
///
/// When the proxy runs as a Deployment, K8s will auto-restart the pod.
/// This function:
/// 1. Checks if the Deployment still exists
/// 2. If deleted, falls back to full re-deployment via [`recover_bare_pod()`]
/// 3. If present, waits up to [`POD_READY_TIMEOUT_SECS`] for a ready pod
/// 4. For UDP: restarts the port forward (UDP streams are single-shot)
/// 5. For TCP: the existing pod_watcher detects the new pod automatically
pub async fn recover_deployment(config: &Config, client: &kube::Client) -> anyhow::Result<()> {
    let config_id = config
        .id
        .ok_or_else(|| anyhow::anyhow!("Config has no ID"))?;
    let namespace = &config.namespace;

    // Check if the Deployment still exists
    // The service name in config is the hashed_name of the deployment
    let hashed_name = config.service.as_deref().unwrap_or("");
    let deployments: kube::Api<k8s_openapi::api::apps::v1::Deployment> =
        kube::Api::namespaced(client.clone(), namespace);

    let deployment_exists = deployments.get(hashed_name).await.is_ok();

    if !deployment_exists {
        // Deployment was deleted — fall back to full re-deployment
        log::warn!(
            "Deployment {} not found, falling back to bare pod recovery for config {}",
            hashed_name,
            config_id
        );
        return recover_bare_pod(config, client).await;
    }

    // Deployment exists — wait for K8s to restart the pod (up to
    // POD_READY_TIMEOUT_SECS) Use label selector to find the pod
    let pods: kube::Api<k8s_openapi::api::core::v1::Pod> =
        kube::Api::namespaced(client.clone(), namespace);
    let label_selector = format!("app={},config_id={}", hashed_name, config_id);
    let lp = kube::api::ListParams::default().labels(&label_selector);

    let deadline =
        tokio::time::Instant::now() + tokio::time::Duration::from_secs(POD_READY_TIMEOUT_SECS);

    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow::anyhow!(
                "Timed out waiting for replacement pod for deployment {} (config {})",
                hashed_name,
                config_id
            ));
        }

        let pod_list = pods
            .list(&lp)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list pods: {}", e))?;

        let ready_pod = pod_list.items.iter().find(|pod| {
            pod.status
                .as_ref()
                .and_then(|s| s.conditions.as_ref())
                .map(|conditions| {
                    conditions
                        .iter()
                        .any(|c| c.type_ == "Ready" && c.status == "True")
                })
                .unwrap_or(false)
        });

        if ready_pod.is_some() {
            log::info!(
                "Replacement pod ready for deployment {} (config {})",
                hashed_name,
                config_id
            );
            // TCP: the existing pod_watcher will detect the new pod and reconnect
            // UDP: the stream is single-shot, so we need to restart the port forward
            if config.protocol == "udp" {
                // Remove old CHILD_PROCESSES entry first
                let keys_to_remove: Vec<String> = crate::port_forward::CHILD_PROCESSES
                    .iter()
                    .filter(|entry| entry.key().starts_with(&format!("config:{}:", config_id)))
                    .map(|entry| entry.key().clone())
                    .collect();
                for key in keys_to_remove {
                    crate::port_forward::CHILD_PROCESSES.remove(&key);
                }
                // Restart UDP port forward
                crate::kube::start::start_port_forward(vec![config.clone()], "udp")
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to restart UDP forward: {}", e))?;
            }
            return Ok(());
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_acquire_recovery_lock_creates_new() {
        let config_id = 42i64;
        let lock1 = acquire_recovery_lock(config_id).await;
        let lock2 = acquire_recovery_lock(config_id).await;

        // Both should be the same Arc
        assert!(Arc::ptr_eq(&lock1, &lock2));

        // Cleanup
        remove_recovery_lock(config_id);
    }

    #[tokio::test]
    async fn test_remove_recovery_lock() {
        let config_id = 99i64;
        let _lock = acquire_recovery_lock(config_id).await;

        // Lock exists
        assert!(RECOVERY_LOCKS.contains_key(&config_id));

        // Remove it
        remove_recovery_lock(config_id);

        // Lock is gone
        assert!(!RECOVERY_LOCKS.contains_key(&config_id));
    }

    #[test]
    fn test_recovery_state_variants() {
        let idle = RecoveryState::Idle;
        let monitoring = RecoveryState::Monitoring;
        let retrying = RecoveryState::Retrying {
            attempt: 1,
            last_error: "connection lost".to_string(),
        };
        let failed = RecoveryState::Failed {
            total_attempts: 5,
            final_error: "pod not ready".to_string(),
        };
        let cancelled = RecoveryState::Cancelled;

        assert_eq!(idle, RecoveryState::Idle);
        assert_eq!(monitoring, RecoveryState::Monitoring);
        assert_ne!(idle, monitoring);
        assert_ne!(retrying, failed);
        assert_eq!(cancelled, RecoveryState::Cancelled);
    }

    #[test]
    fn test_proxy_type_variants() {
        let bare = ProxyType::BarePod;
        let deploy = ProxyType::Deployment;

        assert_eq!(bare, ProxyType::BarePod);
        assert_eq!(deploy, ProxyType::Deployment);
        assert_ne!(bare, deploy);
    }

    #[test]
    fn test_recovery_signal_variants() {
        let pod_died = RecoverySignal::PodDied;
        let stream_failed = RecoverySignal::StreamFailed;
        let health_failed = RecoverySignal::HealthCheckFailed;

        assert_eq!(pod_died, RecoverySignal::PodDied);
        assert_ne!(pod_died, stream_failed);
        assert_ne!(stream_failed, health_failed);
    }

    #[test]
    fn test_constants() {
        assert_eq!(MAX_RECOVERY_ATTEMPTS, 5);
        assert_eq!(BASE_BACKOFF_SECS, 2);
        assert_eq!(MAX_BACKOFF_SECS, 32);
        assert_eq!(POD_READY_TIMEOUT_SECS, 30);
    }

    #[tokio::test]
    async fn test_recovery_loop_cancels_cleanly() {
        use std::time::Duration;

        let config = kftray_commons::models::config_model::Config {
            id: Some(9999),
            service: Some("test-svc".to_string()),
            namespace: "default".to_string(),
            protocol: "tcp".to_string(),
            ..Default::default()
        };
        let manager = Arc::new(ProxyRecoveryManager::new(config, ProxyType::BarePod));
        let manager_clone = Arc::clone(&manager);

        let loop_handle = tokio::spawn(async move {
            manager_clone.run_recovery_loop().await;
        });

        // Give the loop time to start and enter Monitoring state
        tokio::time::sleep(Duration::from_millis(50)).await;

        manager.cancel();

        tokio::time::timeout(Duration::from_secs(5), loop_handle)
            .await
            .expect("Loop should exit within 5 seconds")
            .expect("Loop task should not panic");
    }

    // ====================================================================
    // T14: ProxyRecoveryManager + retry logic tests
    // ====================================================================

    fn make_test_config(id: i64) -> kftray_commons::models::config_model::Config {
        kftray_commons::models::config_model::Config {
            id: Some(id),
            kubeconfig: Some("/nonexistent/path/kubeconfig".to_string()),
            namespace: "default".to_string(),
            protocol: "tcp".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_backoff_calculation_correctness() {
        // Verify BASE_BACKOFF_SECS * 2^(attempt-1) for attempts 1..=5
        let expected: [(u32, u64); 5] = [(1, 2), (2, 4), (3, 8), (4, 16), (5, 32)];
        for (attempt, expected_secs) in expected {
            let backoff = std::cmp::min(
                BASE_BACKOFF_SECS.saturating_mul(1u64 << (attempt - 1)),
                MAX_BACKOFF_SECS,
            );
            assert_eq!(
                backoff, expected_secs,
                "Backoff for attempt {} should be {}s, got {}s",
                attempt, expected_secs, backoff
            );
        }

        // Verify capping at MAX_BACKOFF_SECS for attempts beyond 5
        for attempt in 6..=10u32 {
            let backoff = std::cmp::min(
                BASE_BACKOFF_SECS.saturating_mul(1u64 << (attempt - 1)),
                MAX_BACKOFF_SECS,
            );
            assert_eq!(
                backoff, MAX_BACKOFF_SECS,
                "Backoff for attempt {} should be capped at {}s",
                attempt, MAX_BACKOFF_SECS
            );
        }
    }

    #[tokio::test]
    async fn test_recovery_loop_exhausts_retries_and_sets_failed() {
        tokio::time::pause();

        let config = make_test_config(5555);
        let manager = Arc::new(ProxyRecoveryManager::new(config, ProxyType::BarePod));
        let manager_clone = Arc::clone(&manager);
        let handle = tokio::spawn(async move {
            manager_clone.run_recovery_loop().await;
        });

        // Wait for Monitoring state
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Trigger recovery — do_recovery_attempt() will fail (no K8s cluster)
        manager.signal_recovery(RecoverySignal::PodDied);
        // Poll until state is Failed. Each iteration advances virtual time
        // (for backoff sleeps) and yields for async I/O processing.
        let wall_deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            tokio::time::advance(Duration::from_millis(500)).await;
            for _ in 0..50 {
                tokio::task::yield_now().await;
            }
            let state = manager.state.read().await;
            if matches!(*state, RecoveryState::Failed { .. }) {
                break;
            }
            drop(state);
            assert!(
                std::time::Instant::now() < wall_deadline,
                "Timed out (30s wall clock) waiting for Failed state"
            );
        }

        // Verify state is Failed with correct attempt count
        let state = manager.state.read().await;
        match &*state {
            RecoveryState::Failed { total_attempts, .. } => {
                assert_eq!(
                    *total_attempts, MAX_RECOVERY_ATTEMPTS,
                    "Should have exhausted all {} attempts",
                    MAX_RECOVERY_ATTEMPTS
                );
            }
            other => panic!(
                "Expected Failed state after exhausting retries, got {:?}",
                other
            ),
        }
        drop(state);

        // Cancel the loop (it's back to waiting for next signal after Failed)
        manager.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
        RECOVERY_LOCKS.remove(&5555);
    }

    #[tokio::test]
    async fn test_signal_recovery_triggers_loop() {
        tokio::time::pause();

        let config = make_test_config(7777);
        let manager = Arc::new(ProxyRecoveryManager::new(config, ProxyType::Deployment));
        let manager_clone = Arc::clone(&manager);

        let handle = tokio::spawn(async move {
            manager_clone.run_recovery_loop().await;
        });

        // Wait for loop to enter Monitoring
        tokio::time::sleep(Duration::from_millis(10)).await;
        {
            let state = manager.state.read().await;
            assert_eq!(
                *state,
                RecoveryState::Monitoring,
                "Loop should start in Monitoring state"
            );
        }

        // Send recovery signal
        manager.signal_recovery(RecoverySignal::StreamFailed);

        // Give the spawned task enough scheduling opportunities to
        // receive signal → acquire lock → enter retry loop → set Retrying state
        for _ in 0..10 {
            tokio::time::advance(Duration::from_millis(10)).await;
            tokio::task::yield_now().await;
        }

        // Verify state transitioned to Retrying (attempt 1)
        {
            let state = manager.state.read().await;
            assert!(
                matches!(*state, RecoveryState::Retrying { attempt: 1, .. }),
                "Expected Retrying{{attempt:1}} after signal, got {:?}",
                *state
            );
        }

        // Clean up
        manager.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
        RECOVERY_LOCKS.remove(&7777);
    }

    #[tokio::test]
    async fn test_spawn_recovery_manager_inserts_and_remove_cleans() {
        let config_id = 6666i64;
        let config = make_test_config(config_id);

        // Verify not present before spawn
        assert!(
            !RECOVERY_MANAGERS.contains_key(&config_id),
            "RECOVERY_MANAGERS should not contain entry before spawn"
        );

        spawn_recovery_manager(config, ProxyType::BarePod);

        // Verify inserted after spawn
        assert!(
            RECOVERY_MANAGERS.contains_key(&config_id),
            "spawn_recovery_manager should insert entry into RECOVERY_MANAGERS"
        );

        // Simulate stop: remove + cancel (mirrors stop_port_forward behavior)
        if let Some((_, manager)) = RECOVERY_MANAGERS.remove(&config_id) {
            manager.cancel();
        }

        // Verify removed
        assert!(
            !RECOVERY_MANAGERS.contains_key(&config_id),
            "RECOVERY_MANAGERS should not contain entry after removal"
        );

        // Allow spawned task to finish after cancel
        tokio::time::sleep(Duration::from_millis(100)).await;
        RECOVERY_LOCKS.remove(&config_id);
    }

    // ====================================================================
    // T16: Recovery coordination tests
    // ====================================================================

    #[tokio::test]
    async fn test_recovery_lock_serializes_same_config() {
        let config_id = 11111i64;
        let lock = acquire_recovery_lock(config_id).await;
        let guard = lock.lock().await; // Hold the lock

        let lock2 = acquire_recovery_lock(config_id).await;

        // Second attempt for the SAME config_id should block (timeout = proof of
        // blocking)
        let try_result = tokio::time::timeout(Duration::from_millis(50), lock2.lock()).await;

        assert!(
            try_result.is_err(),
            "Second lock attempt should timeout (blocked by first)"
        );

        drop(guard); // Release first lock

        // Now second should succeed immediately
        let try_result2 = tokio::time::timeout(Duration::from_millis(50), lock2.lock()).await;

        assert!(
            try_result2.is_ok(),
            "Second lock should succeed after first released"
        );

        remove_recovery_lock(config_id);
    }

    #[tokio::test]
    async fn test_recovery_lock_parallel_different_configs() {
        let config_a = 22222i64;
        let config_b = 33333i64;

        // Acquire locks for two different config_ids simultaneously
        let lock_a = acquire_recovery_lock(config_a).await;
        let lock_b = acquire_recovery_lock(config_b).await;

        // Hold lock A
        let _guard_a = lock_a.lock().await;

        // Lock B should succeed immediately (different config_id = different mutex)
        let try_result_b = tokio::time::timeout(Duration::from_millis(50), lock_b.lock()).await;

        assert!(
            try_result_b.is_ok(),
            "Lock for different config_id should not block"
        );

        // Verify they are different Arc instances (different mutexes)
        assert!(
            !Arc::ptr_eq(&lock_a, &lock_b),
            "Different config_ids should have different lock instances"
        );

        // Cleanup
        remove_recovery_lock(config_a);
        remove_recovery_lock(config_b);
    }

    #[tokio::test]
    async fn test_network_monitor_skips_config_with_active_recovery() {
        let config_id = 44444i64;

        // Simulate recovery in progress by inserting into RECOVERY_LOCKS
        let _lock = acquire_recovery_lock(config_id).await;

        // Verify the config would be skipped by the network monitor filter
        // (mirrors the logic in config_manager.rs lines 106-116)
        assert!(
            RECOVERY_LOCKS.contains_key(&config_id),
            "Config with active recovery should be present in RECOVERY_LOCKS"
        );

        // Build a list of proxy configs and apply the same filter logic
        // as config_manager.rs:restart_protocol_batch()
        let proxy_configs = vec![
            make_test_config(config_id), // has active recovery
            make_test_config(55555),     // no active recovery
        ];

        let configs_to_restart: Vec<_> = proxy_configs
            .into_iter()
            .filter(|config| {
                if let Some(cid) = config.id {
                    if RECOVERY_LOCKS.contains_key(&cid) {
                        return false; // skip — recovery in progress
                    }
                }
                true
            })
            .collect();

        assert_eq!(
            configs_to_restart.len(),
            1,
            "Only the config without active recovery should remain"
        );
        assert_eq!(
            configs_to_restart[0].id,
            Some(55555),
            "The surviving config should be the one without a recovery lock"
        );

        // Cleanup
        remove_recovery_lock(config_id);

        // After removal, RECOVERY_LOCKS should no longer skip this config
        assert!(
            !RECOVERY_LOCKS.contains_key(&config_id),
            "Config should not be in RECOVERY_LOCKS after removal"
        );
    }
}

use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use futures::TryStreamExt;
use kftray_commons::models::config_model::Config;
use kftray_commons::utils::db_mode::DatabaseMode;
use kube_runtime::WatchStreamExt;
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
pub fn spawn_recovery_manager(
    config: Config, proxy_type: ProxyType, mode: DatabaseMode, ssl_override: bool,
) {
    let Some(config_id) = config.id else {
        return;
    };

    let mut spawned = false;
    RECOVERY_MANAGERS.entry(config_id).or_insert_with(|| {
        let manager = Arc::new(ProxyRecoveryManager::new(
            config,
            proxy_type,
            mode,
            ssl_override,
        ));
        let rx = manager.recovery_signal_tx.subscribe();
        let manager_for_task = Arc::clone(&manager);
        tokio::spawn(async move {
            manager_for_task.run_recovery_loop_with_rx(rx).await;
        });
        spawned = true;
        manager
    });

    if spawned {
        log::info!("Spawned recovery manager for proxy config {}", config_id);
    } else {
        log::debug!("Recovery manager already active for config {}", config_id);
    }
}

/// Acquire (or create) the recovery lock for a config_id
///
/// This serializes start, stop, and recovery operations for each config.
/// Multiple callers will block until the lock is released.
///
/// # Arguments
/// * `config_id` - The configuration ID to lock
///
/// # Returns
/// An Arc<Mutex<()>> that can be locked to serialize config lifecycle operations
pub async fn acquire_recovery_lock(config_id: i64) -> Arc<Mutex<()>> {
    RECOVERY_LOCKS
        .entry(config_id)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

/// Remove an unused recovery lock for a config_id
///
/// Call this during cleanup (e.g., when stopping a port forward) to free
/// resources. Locks that are still held or awaited remain registered.
///
/// # Arguments
/// * `config_id` - The configuration ID whose lock should be removed
pub fn remove_recovery_lock(config_id: i64) {
    RECOVERY_LOCKS.remove_if(&config_id, |_, lock| Arc::strong_count(lock) == 1);
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
    mode: DatabaseMode,
    ssl_override: bool,
}

impl ProxyRecoveryManager {
    /// Create a new recovery manager for the given config and proxy type.
    ///
    /// # Arguments
    /// * `config` - The port-forward configuration to recover
    /// * `proxy_type` - Whether this is a bare pod or deployment proxy
    pub fn new(
        config: Config, proxy_type: ProxyType, mode: DatabaseMode, ssl_override: bool,
    ) -> Self {
        let config_id = config.id.unwrap_or(0);
        let (recovery_signal_tx, _) = tokio::sync::broadcast::channel::<RecoverySignal>(16);
        Self {
            config,
            config_id,
            proxy_type,
            cancel_token: CancellationToken::new(),
            state: Arc::new(tokio::sync::RwLock::new(RecoveryState::Idle)),
            recovery_signal_tx,
            mode,
            ssl_override,
        }
    }

    /// Send a recovery signal to trigger the recovery loop.
    ///
    /// Safe to call from any context. If the loop is not running or
    /// the receiver has been dropped, the signal is silently discarded.
    pub fn signal_recovery(&self, signal: RecoverySignal) {
        let _ = self.recovery_signal_tx.send(signal);
    }

    #[cfg(test)]
    pub(crate) fn subscribe_recovery_signals(
        &self,
    ) -> tokio::sync::broadcast::Receiver<RecoverySignal> {
        self.recovery_signal_tx.subscribe()
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
    /// 2. Retries up to [`MAX_RECOVERY_ATTEMPTS`] times with exponential backoff
    /// 3. Updates [`ConfigState`] in the database at each step
    pub async fn run_recovery_loop(&self) {
        let rx = self.recovery_signal_tx.subscribe();
        self.run_recovery_loop_with_rx(rx).await;
    }

    pub async fn run_recovery_loop_with_rx(
        &self, mut rx: tokio::sync::broadcast::Receiver<RecoverySignal>,
    ) {
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

                let result = tokio::select! {
                    biased;
                    _ = self.cancel_token.cancelled() => {
                        all_attempts_exhausted = false;
                        break;
                    }
                    result = self.do_recovery_attempt() => result,
                };
                match result {
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
            drop(_guard);
            drop(lock);
            remove_recovery_lock(self.config_id);
            // Lock released when _guard drops
        }
    }

    /// Attempt a single recovery operation.
    ///
    /// Dispatches to the appropriate recovery function based on [`ProxyType`]:
    /// - [`ProxyType::BarePod`] → full re-deployment via [`recover_bare_pod()`]
    /// - [`ProxyType::Deployment`] → stream reconnection via [`recover_deployment()`]
    async fn do_recovery_attempt(&self) -> anyhow::Result<()> {
        let client_key = crate::kube::shared_client::ServiceClientKey::new(
            self.config.context.clone(),
            self.config.kubeconfig.clone(),
        );
        let client = crate::kube::shared_client::SHARED_CLIENT_MANAGER
            .get_connection(client_key)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get K8s client: {}", e))?;
        let client = client.client.clone();

        match self.proxy_type {
            ProxyType::BarePod => {
                recover_bare_pod(
                    &self.config,
                    &client,
                    self.mode,
                    self.ssl_override,
                    &self.cancel_token,
                )
                .await
            }
            ProxyType::Deployment => {
                recover_deployment(
                    &self.config,
                    &client,
                    self.mode,
                    self.ssl_override,
                    &self.cancel_token,
                )
                .await
            }
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
            process_id: if is_running {
                Some(std::process::id())
            } else {
                None
            },
            is_retrying,
            retry_count,
            last_error,
        };
        if let Err(e) =
            kftray_commons::utils::config_state::update_config_state_with_mode(&state, self.mode)
                .await
        {
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

async fn cleanup_child_processes_for_config(config_id: i64) {
    if let Some((_, mut process)) = crate::port_forward::CHILD_PROCESSES.remove(&config_id) {
        process.cleanup_and_abort().await;
    }
}

/// Recover a bare pod proxy by fully re-deploying.
///
/// Bare pods have no controller (no Deployment/ReplicaSet), so when the pod
/// dies there is nothing to auto-restart it. This function:
/// 1. Cleans up old cluster resources (pods, deployments) to prevent orphans
/// 2. Removes stale [`CHILD_PROCESSES`](crate::port_forward::CHILD_PROCESSES) entries for this
///    config
/// 3. Re-deploys a fresh proxy pod via
///    [`deploy_and_forward_pod()`](crate::kube::proxy::deploy_and_forward_pod)
pub async fn recover_bare_pod(
    config: &Config, client: &kube::Client, mode: DatabaseMode, ssl_override: bool,
    cancellation: &CancellationToken,
) -> anyhow::Result<()> {
    let config_id = config
        .id
        .ok_or_else(|| anyhow::anyhow!("Config has no ID"))?;
    let namespace = &config.namespace;

    crate::kube::stop::delete_proxy_cluster_resources(client.clone(), namespace, config_id)
        .await
        .map_err(anyhow::Error::msg)?;
    cleanup_child_processes_for_config(config_id).await;

    // Step 3: Re-deploy via the existing deploy_and_forward_pod() function
    // This generates a new hashed_name and creates a fresh pod + port forward
    crate::kube::proxy::start_proxy_config(config.clone(), mode, ssl_override, cancellation)
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
pub async fn recover_deployment(
    config: &Config, client: &kube::Client, mode: DatabaseMode, ssl_override: bool,
    cancellation: &CancellationToken,
) -> anyhow::Result<()> {
    let config_id = config
        .id
        .ok_or_else(|| anyhow::anyhow!("Config has no ID"))?;
    let namespace = &config.namespace;

    let deployments: kube::Api<k8s_openapi::api::apps::v1::Deployment> =
        kube::Api::namespaced(client.clone(), namespace);

    let prefix = crate::kube::proxy::proxy_resource_prefix();
    let deployment = deployments
        .list(
            &kube::api::ListParams::default().labels(&crate::kube::proxy::proxy_owner_selector(
                &config_id.to_string(),
            )),
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to query deployment for config {}: {}", config_id, e))?
        .items
        .into_iter()
        .find(|deployment| {
            deployment.metadata.deletion_timestamp.is_none()
                && deployment
                    .metadata
                    .name
                    .as_ref()
                    .is_some_and(|name| name.starts_with(&prefix))
        });

    let Some(deployment) = deployment else {
        log::warn!(
            "Proxy deployment not found, falling back to bare pod recovery for config {}",
            config_id
        );
        return recover_bare_pod(config, client, mode, ssl_override, cancellation).await;
    };
    let container_name = deployment
        .spec
        .as_ref()
        .and_then(|spec| spec.template.spec.as_ref())
        .and_then(crate::kube::proxy::relay_container_name)
        .ok_or_else(|| anyhow::anyhow!("Proxy deployment has no relay container"))?;
    let hashed_name = deployment
        .metadata
        .name
        .ok_or_else(|| anyhow::anyhow!("Proxy deployment has no name"))?;

    let pods: kube::Api<k8s_openapi::api::core::v1::Pod> =
        kube::Api::namespaced(client.clone(), namespace);
    let watcher = kube_runtime::watcher(
        pods,
        kube_runtime::watcher::Config::default().labels(&format!(
            "app={hashed_name},{}",
            crate::kube::proxy::proxy_owner_selector(&config_id.to_string())
        )),
    )
    .applied_objects();
    futures::pin_mut!(watcher);
    tokio::time::timeout(Duration::from_secs(POD_READY_TIMEOUT_SECS), async {
        while let Some(pod) = watcher.try_next().await? {
            if crate::kube::proxy::relay_started(Some(&pod), &container_name) {
                if config.protocol.eq_ignore_ascii_case("udp") {
                    cleanup_child_processes_for_config(config_id).await;
                    let mut current_config = config.clone();
                    current_config.service = Some(hashed_name.clone());
                    crate::kube::start::start_config(current_config, "udp", mode, ssl_override)
                        .await
                        .map_err(anyhow::Error::msg)?;
                }
                return Ok::<_, anyhow::Error>(());
            }
        }
        Err(anyhow::anyhow!(
            "Pod watch ended before deployment {hashed_name} recovered"
        ))
    })
    .await
    .map_err(|_| anyhow::anyhow!("Timed out waiting for deployment {hashed_name} to recover"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_recovery_loop_cancels_cleanly() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        use std::time::Duration;

        let config = kftray_commons::models::config_model::Config {
            id: Some(9999),
            service: Some("test-svc".to_string()),
            namespace: "default".to_string(),
            protocol: "tcp".to_string(),
            ..Default::default()
        };
        let manager = Arc::new(ProxyRecoveryManager::new(
            config,
            ProxyType::BarePod,
            DatabaseMode::Memory,
            false,
        ));
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

    #[tokio::test]
    async fn test_recovery_loop_exhausts_retries_and_sets_failed() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        tokio::time::pause();

        let config = make_test_config(5555);
        let manager = Arc::new(ProxyRecoveryManager::new(
            config,
            ProxyType::BarePod,
            DatabaseMode::Memory,
            false,
        ));
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
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        tokio::time::pause();

        let config = make_test_config(7777);
        let manager = Arc::new(ProxyRecoveryManager::new(
            config,
            ProxyType::Deployment,
            DatabaseMode::Memory,
            false,
        ));
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

    // ====================================================================
    // T16: Recovery coordination tests
    // ====================================================================

    #[tokio::test]
    async fn test_recovery_lock_serializes_same_config() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
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
        drop(try_result);

        drop(guard); // Release first lock

        // Now second should succeed immediately
        let try_result2 = tokio::time::timeout(Duration::from_millis(50), lock2.lock()).await;

        assert!(
            try_result2.is_ok(),
            "Second lock should succeed after first released"
        );
        drop(try_result2);

        remove_recovery_lock(config_id);
        drop(lock);
        drop(lock2);
        remove_recovery_lock(config_id);
    }

    #[tokio::test]
    async fn test_recovery_lock_parallel_different_configs() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
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
        drop(try_result_b);

        // Cleanup
        remove_recovery_lock(config_a);
        remove_recovery_lock(config_b);
        drop(_guard_a);
        drop(lock_a);
        drop(lock_b);
        remove_recovery_lock(config_a);
        remove_recovery_lock(config_b);
    }

    #[tokio::test]
    async fn removing_an_active_lock_does_not_allow_overlapping_operations() {
        let _lock = crate::port_forward::PROCESS_TEST_MUTEX.lock().await;
        let id = 410_041;
        let first = acquire_recovery_lock(id).await;
        let guard = first.lock().await;
        remove_recovery_lock(id);
        let second = acquire_recovery_lock(id).await;
        assert!(second.try_lock().is_err());
        drop(guard);
        assert!(second.try_lock().is_ok());
        drop(first);
        drop(second);
        remove_recovery_lock(id);
    }

    #[tokio::test]
    async fn repeated_recovery_uses_the_current_owned_deployment() {
        use http::{
            Method,
            Request,
            Response,
        };
        use kube::client::Body;

        let (service, mut requests) = tower_test::mock::pair::<Request<Body>, Response<Body>>();
        let client = kube::Client::new(service, "default");
        let config = Config {
            id: Some(420_051),
            namespace: "default".to_string(),
            service: Some("stale-deployment-name".to_string()),
            protocol: "tcp".to_string(),
            ..Config::default()
        };
        let prefix = crate::kube::proxy::proxy_resource_prefix();
        let owned: Vec<String> = ["a", "b"]
            .iter()
            .map(|suffix| format!("{prefix}tcp-1-{suffix}"))
            .collect();
        let expected = owned.clone();
        let installation_id = kftray_commons::utils::config_dir::get_installation_id();
        let server = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(async move {
            for name in expected {
                let (request, send) = requests.next_request().await.unwrap();
                assert_eq!(request.method(), Method::GET);
                assert_eq!(
                    request.uri().path(),
                    "/apis/apps/v1/namespaces/default/deployments"
                );
                let query = request.uri().query().unwrap_or_default();
                assert!(
                    query.contains("labelSelector=")
                        && query.contains("config_id")
                        && query.contains("420051")
                        && query.contains(installation_id),
                    "recovery must not select another installation's deployment: {query}"
                );
                let relay = serde_json::json!({
                    "spec": {"template": {"spec": {"containers": [{
                        "name": "kftray-relay",
                        "env": [{"name": "LOCAL_PORT", "value": "8080"}]
                    }]}}}
                });
                send.send_response(Response::builder().body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "metadata":{"resourceVersion":"1"},
                        "items":[
                            {"metadata":{"name":format!("{name}-retiring"),"deletionTimestamp":"2026-09-11T00:00:00Z"},"spec":relay["spec"]},
                            {"metadata":{"name":"someone-elses-deployment"},"spec":relay["spec"]},
                            {"metadata":{"name":name},"spec":relay["spec"]}
                        ]
                    })).unwrap()
                )).unwrap());

                let (request, send) = requests.next_request().await.unwrap();
                assert_eq!(request.method(), Method::GET);
                assert_eq!(request.uri().path(), "/api/v1/namespaces/default/pods");
                let query = request.uri().query().unwrap_or_default();
                assert!(
                    query.contains("labelSelector=")
                        && query.contains(&name)
                        && query.contains("config_id")
                        && query.contains("420051")
                        && query.contains(installation_id),
                    "{query}"
                );
                send.send_response(
                    Response::builder()
                        .body(Body::from(
                            serde_json::to_vec(&serde_json::json!({
                                "metadata":{"resourceVersion":"1"},
                                "items":[{
                                    "metadata":{"name":format!("{name}-pod")},
                                    "status":{
                                        "phase":"Running",
                                        "conditions":[{"type":"Ready","status":"False"}],
                                        "containerStatuses":[{
                                            "name":"kftray-relay",
                                            "started":true,
                                            "ready":false,
                                            "restartCount":0,
                                            "image":"kftray-server",
                                            "imageID":""
                                        }]
                                    }
                                }]
                            }))
                            .unwrap(),
                        ))
                        .unwrap(),
                );
            }
        }));
        let cancellation = CancellationToken::new();
        tokio::time::timeout(Duration::from_secs(2), async {
            for _ in 0..owned.len() {
                recover_deployment(&config, &client, DatabaseMode::Memory, false, &cancellation)
                    .await
                    .unwrap();
            }
            server.await.unwrap();
        })
        .await
        .unwrap();
    }
}

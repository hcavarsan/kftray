use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::{
    Duration,
    Instant,
};

use futures::FutureExt;
use log::{
    debug,
    error,
    info,
};
use tokio::sync::Mutex;
use tokio::time::sleep;

use crate::config_manager::ConfigManager;
use crate::health::HealthChecker;
use crate::network::NetworkChecker;
use crate::types::{
    MonitorConfig,
    TaskState,
};

static TASK_STATE: tokio::sync::OnceCell<Arc<Mutex<TaskState>>> =
    tokio::sync::OnceCell::const_new();

pub struct NetworkMonitor {
    config: MonitorConfig,
    network_checker: NetworkChecker,
    health_checker: HealthChecker,
}

impl NetworkMonitor {
    pub fn new() -> Self {
        let config = MonitorConfig::default();
        Self {
            network_checker: NetworkChecker::new(config.clone()),
            health_checker: HealthChecker::new(config.clone()),
            config,
        }
    }

    pub async fn start(self) {
        info!("Starting network monitor");

        let initial_configs = match ConfigManager::get_active_configs().await {
            Ok(configs) => configs,
            Err(e) => {
                error!("Failed to get initial active configs: {e}");
                Vec::new()
            }
        };

        if initial_configs.is_empty() {
            info!("No active port forward configs found, network monitor will idle until configs are started");
        } else {
            info!(
                "Network monitor starting with {} active port forward configs",
                initial_configs.len()
            );
        }

        let monitor = Arc::new(self);
        let background_monitor = monitor.clone();
        tokio::spawn(async move {
            background_monitor.run_background_monitor().await;
        });

        monitor.run_main_loop().await;
    }

    async fn run_main_loop(&self) {
        let mut network_up = self.network_checker.check_connectivity().await;
        let mut failure_count = 0;
        let mut last_health = Instant::now();
        let mut last_fast = Instant::now();

        loop {
            sleep(self.get_sleep_duration(network_up, failure_count)).await;

            let active_configs: Vec<kftray_commons::config_model::Config> =
                (ConfigManager::get_active_configs().await).unwrap_or_default();

            if active_configs.is_empty() {
                debug!("No active port forward configs found, network monitor idling");
                sleep(Duration::from_secs(60)).await;
                failure_count = 0;
                continue;
            }

            let is_up = self.network_checker.check_connectivity().await;

            {
                let state = self.get_task_state().await;
                let mut guard = state.lock().await;
                guard.update_network_state(is_up);
            }

            if !network_up && is_up {
                debug!("Network reconnected (main loop)");
                failure_count = 0;
                if self.should_start_reconnect().await {
                    debug!("Starting reconnection handler from main loop");
                    let monitor = self.clone();
                    tokio::spawn(async move {
                        monitor.handle_reconnect_with_state().await;
                    });
                } else {
                    debug!("Skipping reconnection - rate limited or network unstable");
                }
                last_health = Instant::now();
            } else if network_up && !is_up {
                info!("Network disconnected");
                failure_count = failure_count.saturating_add(1);
            }

            if network_up && last_health.elapsed() > self.config.health_interval {
                if self.should_start_health_check().await {
                    let monitor = self.clone();
                    tokio::spawn(async move {
                        monitor.check_health_with_state().await;
                    });
                }
                last_health = Instant::now();
            }

            if network_up && failure_count > 0 && last_fast.elapsed() > self.config.sleep_up {
                if self.should_start_health_check().await {
                    let failed_configs = self.check_health_fast().await;
                    if failed_configs.is_empty() {
                        failure_count = failure_count.saturating_sub(1);
                    }
                }
                last_fast = Instant::now();
            }

            network_up = is_up;
        }
    }

    async fn run_background_monitor(&self) {
        info!("Starting background monitor");
        let mut last_check = Instant::now();
        let mut last_network_state = self.network_checker.check_connectivity().await;
        let mut last_network_info = self.network_checker.get_network_fingerprint().await;

        loop {
            sleep(self.config.monitor_interval).await;

            let active_configs: Vec<kftray_commons::config_model::Config> =
                (ConfigManager::get_active_configs().await).unwrap_or_default();

            if active_configs.is_empty() {
                debug!("No active port forward configs found, background monitor idling");
                sleep(Duration::from_secs(120)).await;
                continue;
            }

            let current_network_info = self.network_checker.get_network_fingerprint().await;
            if current_network_info != last_network_info {
                info!("Network interface change detected (background monitor) - monitoring state changes only");
                last_network_info = current_network_info;
            }

            let current_network_state = self.network_checker.check_connectivity().await;

            if current_network_state != last_network_state {
                info!("Network state change detected: {last_network_state} -> {current_network_state}");
                if current_network_state {
                    debug!("Network recovered (background monitor)");
                    if self.should_start_reconnect().await {
                        let monitor = self.clone();
                        tokio::spawn(async move {
                            monitor.handle_reconnect_with_state().await;
                        });
                    }
                }
                last_network_state = current_network_state;
            }

            if last_check.elapsed() > self.config.health_interval {
                if self.should_start_health_check().await {
                    let monitor = self.clone();
                    tokio::spawn(async move {
                        monitor.check_health_with_state().await;
                    });
                }
                last_check = Instant::now();
            }
        }
    }

    fn get_sleep_duration(&self, network_up: bool, failure_count: u32) -> Duration {
        match (network_up, failure_count) {
            (true, 0) => self.config.sleep_up,
            (true, _) => self.config.network_timeout,
            (false, _) => self.config.sleep_down,
        }
    }

    async fn handle_reconnect(&self) {
        info!("Handling network reconnection");

        let active_configs = match ConfigManager::get_active_configs().await {
            Ok(configs) => configs,
            Err(e) => {
                error!("Failed to get active configs: {e}");
                return;
            }
        };

        if active_configs.is_empty() {
            info!("No active configs to restart");
            return;
        }

        sleep(Duration::from_secs(5)).await;

        info!("Restarting {} port forwards", active_configs.len());

        let http_log_state = Arc::new(kftray_http_logs::HttpLogState::new());
        ConfigManager::restart_port_forwards(active_configs, http_log_state).await;
    }

    async fn check_health(&self) {
        let active_configs = match ConfigManager::get_active_configs().await {
            Ok(configs) => configs,
            Err(_) => return,
        };

        if active_configs.is_empty() {
            return;
        }

        let failed_configs = self
            .health_checker
            .validate_port_forwards(&active_configs)
            .await;

        if !failed_configs.is_empty() {
            let mut confirmed_failed = Vec::new();
            for config in failed_configs {
                sleep(Duration::from_secs(3)).await;

                let mut is_failed = true;
                for _ in 0..3 {
                    if self.health_checker.check_single_port_forward(&config).await {
                        is_failed = false;
                        break;
                    }
                    sleep(Duration::from_secs(1)).await;
                }

                if is_failed {
                    confirmed_failed.push(config);
                }
            }

            if !confirmed_failed.is_empty() {
                info!(
                    "Restarting {} confirmed failed port forwards",
                    confirmed_failed.len()
                );
                let http_log_state = Arc::new(kftray_http_logs::HttpLogState::new());
                ConfigManager::restart_port_forwards(confirmed_failed, http_log_state).await;
            }
        }
    }

    async fn check_health_fast(&self) -> Vec<kftray_commons::models::config_model::Config> {
        let active_configs = match ConfigManager::get_active_configs().await {
            Ok(configs) => configs,
            Err(_) => return Vec::new(),
        };

        if active_configs.is_empty() {
            return Vec::new();
        }

        let failed_configs = self
            .health_checker
            .validate_port_forwards_fast(&active_configs)
            .await;

        if !failed_configs.is_empty() {
            info!(
                "Fast check found {} failed port forwards",
                failed_configs.len()
            );
        }

        failed_configs
    }

    async fn get_task_state(&self) -> Arc<Mutex<TaskState>> {
        TASK_STATE
            .get_or_init(|| async { Arc::new(Mutex::new(TaskState::default())) })
            .await
            .clone()
    }

    async fn should_start_reconnect(&self) -> bool {
        let active_configs: Vec<kftray_commons::config_model::Config> =
            (ConfigManager::get_active_configs().await).unwrap_or_default();

        if active_configs.is_empty() {
            debug!("Cannot reconnect: no active port forward configs");
            return false;
        }

        let state = self.get_task_state().await;
        let mut guard = state.lock().await;

        if guard.reconnect_in_progress {
            debug!("Cannot reconnect: already in progress");
            return false;
        }

        if !guard.can_reconnect() {
            debug!(
                "Cannot reconnect: circuit breaker tripped (attempts: {}/{})",
                guard.reconnect_attempts, guard.max_reconnect_attempts
            );
            return false;
        }

        if let Some(last) = guard.last_reconnect {
            let elapsed = last.elapsed();
            if elapsed < Duration::from_secs(30) {
                debug!(
                    "Cannot reconnect: only {:.1}s since last reconnect (need 30s)",
                    elapsed.as_secs_f32()
                );
                return false;
            }
        }

        if let Some(stable_since) = guard.network_stable_since {
            let stable_duration = stable_since.elapsed();
            if stable_duration < Duration::from_secs(10) {
                debug!(
                    "Cannot reconnect: network only stable for {:.1}s (need 10s)",
                    stable_duration.as_secs_f32()
                );
                return false;
            }
            debug!(
                "Network stable for {:.1}s, proceeding with reconnect (attempt {}/{})",
                stable_duration.as_secs_f32(),
                guard.reconnect_attempts + 1,
                guard.max_reconnect_attempts
            );
        } else {
            debug!("Cannot reconnect: network not stable");
            return false;
        }

        guard.start_reconnect();
        true
    }

    async fn should_start_health_check(&self) -> bool {
        let active_configs: Vec<kftray_commons::config_model::Config> =
            (ConfigManager::get_active_configs().await).unwrap_or_default();

        if active_configs.is_empty() {
            return false;
        }

        let state = self.get_task_state().await;
        let mut guard = state.lock().await;
        if guard.should_health_check() {
            guard.start_health_check();
            true
        } else {
            false
        }
    }

    async fn handle_reconnect_with_state(&self) {
        let state = self.get_task_state().await;

        let result = AssertUnwindSafe(self.handle_reconnect())
            .catch_unwind()
            .await;

        {
            let mut guard = state.lock().await;
            guard.finish_reconnect();
        }

        if let Err(e) = result {
            log::error!("Reconnect handler panicked: {e:?}");
        }
    }

    async fn check_health_with_state(&self) {
        let state = self.get_task_state().await;

        let result = AssertUnwindSafe(self.check_health()).catch_unwind().await;

        {
            let mut guard = state.lock().await;
            guard.finish_health_check();
        }

        if let Err(e) = result {
            log::error!("Health check handler panicked: {e:?}");
        }
    }
}

impl Clone for NetworkMonitor {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            network_checker: NetworkChecker::new(self.config.clone()),
            health_checker: HealthChecker::new(self.config.clone()),
        }
    }
}

pub async fn start_network_monitor() {
    let monitor = NetworkMonitor::new();
    monitor.start().await;
}

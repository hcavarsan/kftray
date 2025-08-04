use std::time::Duration;

use kftray_commons::models::config_model::Config;
use log::info;
use tokio::time::{
    sleep,
    timeout,
};

use crate::network::is_port_listening;
use crate::types::{
    HealthCheckResult,
    MonitorConfig,
};

pub struct HealthChecker {
    config: MonitorConfig,
}

impl HealthChecker {
    pub fn new(config: MonitorConfig) -> Self {
        Self { config }
    }

    pub async fn check_port_health(
        &self, config: &Config, conn_timeout: Duration, task_timeout: Duration, attempts: usize,
        retry_delay: Duration,
    ) -> bool {
        if attempts == 0 {
            return false;
        }

        let local_port = match config.local_port {
            Some(port) => port,
            None => return false,
        };
        let local_address = config.local_address.as_deref().unwrap_or("127.0.0.1");

        if !is_port_listening(local_address, local_port).await {
            return false;
        }

        let socket_addr = format!("{local_address}:{local_port}");

        for attempt in 0..attempts {
            let addr = socket_addr.clone();
            let health_check = tokio::spawn(async move {
                timeout(conn_timeout, tokio::net::TcpStream::connect(&addr))
                    .await
                    .is_ok()
            });

            match timeout(task_timeout, health_check).await {
                Ok(Ok(true)) => return true,
                Ok(Ok(false)) if attempt + 1 < attempts => {
                    if retry_delay > Duration::ZERO {
                        sleep(retry_delay).await;
                    }
                }
                Ok(Ok(false)) => {
                    // Last attempt failed
                    return false;
                }
                Ok(Err(_)) => {
                    log::warn!("Health check task failed for {local_address}:{local_port}");
                    return false;
                }
                Err(_) => {
                    log::warn!("Health check timeout for {local_address}:{local_port}");
                    return false;
                }
            }
        }

        false
    }

    pub async fn check_single_port_forward(&self, config: &Config) -> bool {
        self.check_port_health(
            config,
            self.config.network_timeout,
            self.config.sleep_up,
            1,
            Duration::ZERO,
        )
        .await
    }

    pub async fn check_single_port_forward_fast(&self, config: &Config) -> bool {
        self.check_port_health(
            config,
            self.config.sleep_down,
            self.config.network_timeout,
            3,
            self.config.retry_delay,
        )
        .await
    }

    pub async fn validate_port_forwards(&self, configs: &[Config]) -> Vec<Config> {
        if configs.is_empty() {
            return Vec::new();
        }

        let health_checks: Vec<_> = configs
            .iter()
            .map(|config| {
                let config_clone = config.clone();
                let checker = self.clone();
                tokio::spawn(async move {
                    let is_healthy = checker.check_single_port_forward(&config_clone).await;
                    HealthCheckResult {
                        config: config_clone,
                        is_healthy,
                    }
                })
            })
            .collect();

        let mut failed_configs = Vec::new();
        for health_check in health_checks {
            match health_check.await {
                Ok(result) => {
                    if !result.is_healthy {
                        info!(
                            "Port forward failed: config {} at {}:{}",
                            result.config.id.unwrap_or(-1),
                            result
                                .config
                                .local_address
                                .as_deref()
                                .unwrap_or("127.0.0.1"),
                            result.config.local_port.unwrap_or(0)
                        );
                        failed_configs.push(result.config);
                    }
                }
                Err(e) => {
                    log::warn!("Health check task failed: {e}");
                }
            }
        }

        failed_configs
    }

    pub async fn validate_port_forwards_fast(&self, configs: &[Config]) -> Vec<Config> {
        if configs.is_empty() {
            return Vec::new();
        }

        let health_checks: Vec<_> = configs
            .iter()
            .map(|config| {
                let config_clone = config.clone();
                let checker = self.clone();
                tokio::spawn(async move {
                    let is_healthy = checker.check_single_port_forward_fast(&config_clone).await;
                    HealthCheckResult {
                        config: config_clone,
                        is_healthy,
                    }
                })
            })
            .collect();

        let mut failed_configs = Vec::new();
        for health_check in health_checks {
            match health_check.await {
                Ok(result) => {
                    if !result.is_healthy {
                        info!(
                            "Fast check - Port forward failed: config {} at {}:{}",
                            result.config.id.unwrap_or(-1),
                            result
                                .config
                                .local_address
                                .as_deref()
                                .unwrap_or("127.0.0.1"),
                            result.config.local_port.unwrap_or(0)
                        );
                        failed_configs.push(result.config);
                    }
                }
                Err(e) => {
                    log::warn!("Fast health check task failed: {e}");
                }
            }
        }

        failed_configs
    }
}

impl Clone for HealthChecker {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
        }
    }
}

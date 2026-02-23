use std::time::Duration;

use futures::stream::{
    FuturesUnordered,
    StreamExt,
};
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

        if config.protocol == "udp" {
            // UDP health check: probe if local port is actually bound/listening
            // Try to bind the same local port — if it succeeds, the port is free (forward
            // is dead) If it fails with AddrInUse, the port is in use (forward
            // is alive)
            let socket_addr = format!("{local_address}:{local_port}");
            match std::net::UdpSocket::bind(&socket_addr) {
                Ok(_) => {
                    // Port is free — forward is dead
                    return false;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                    // Port is in use — forward is alive
                    return true;
                }
                Err(_) => {
                    // Other error (permission denied, etc.) — assume dead
                    return false;
                }
            }
        }

        if !is_port_listening(local_address, local_port).await {
            return false;
        }

        let socket_addr = format!("{local_address}:{local_port}");

        for attempt in 0..attempts {
            let addr = socket_addr.as_str();
            let attempt_fut = async {
                matches!(
                    timeout(conn_timeout, tokio::net::TcpStream::connect(addr)).await,
                    Ok(Ok(_))
                )
            };

            match timeout(task_timeout, attempt_fut).await {
                Ok(true) => return true,
                Ok(false) if attempt + 1 < attempts => {
                    if retry_delay > Duration::ZERO {
                        sleep(retry_delay).await;
                    }
                }
                Ok(false) => return false,
                Err(_) => return false,
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

        let mut futs: FuturesUnordered<_> = configs
            .iter()
            .map(|config_clone| {
                let checker = self.clone();
                let config_clone = config_clone.clone();
                async move {
                    let is_healthy = checker.check_single_port_forward(&config_clone).await;
                    HealthCheckResult {
                        config: config_clone,
                        is_healthy,
                    }
                }
            })
            .collect();

        let mut failed_configs = Vec::new();
        while let Some(result) = futs.next().await {
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

        failed_configs
    }

    pub async fn validate_port_forwards_fast(&self, configs: &[Config]) -> Vec<Config> {
        if configs.is_empty() {
            return Vec::new();
        }

        let mut futs: FuturesUnordered<_> = configs
            .iter()
            .map(|config_clone| {
                let checker = self.clone();
                let config_clone = config_clone.clone();
                async move {
                    let is_healthy = checker.check_single_port_forward_fast(&config_clone).await;
                    HealthCheckResult {
                        config: config_clone,
                        is_healthy,
                    }
                }
            })
            .collect();

        let mut failed_configs = Vec::new();
        while let Some(result) = futs.next().await {
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

#[cfg(test)]
mod tests {
    use std::net::UdpSocket;

    use kftray_commons::models::config_model::Config;

    use super::*;

    fn make_test_config(protocol: &str, local_port: u16, local_address: &str) -> Config {
        Config {
            protocol: protocol.to_string(),
            local_port: Some(local_port),
            local_address: Some(local_address.to_string()),
            ..Config::default()
        }
    }

    #[test]
    fn test_udp_health_check_returns_false_when_port_free() {
        // Bind a socket to get a free port, then drop it to free the port
        let port = {
            let socket = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind socket");
            socket
                .local_addr()
                .expect("Failed to get local addr")
                .port()
        };
        // Port is now free

        let config = make_test_config("udp", port, "127.0.0.1");
        let monitor_config = MonitorConfig::default();
        let checker = HealthChecker::new(monitor_config);

        // UDP health check should return false (port is free = forward is dead)
        let result = futures::executor::block_on(async {
            checker
                .check_port_health(
                    &config,
                    Duration::from_secs(1),
                    Duration::from_secs(1),
                    1,
                    Duration::ZERO,
                )
                .await
        });

        assert!(
            !result,
            "UDP health check should return false when port is free (forward is dead)"
        );
    }

    #[test]
    fn test_udp_health_check_returns_true_when_port_in_use() {
        // Bind a socket and keep it alive during the test
        let socket = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind socket");
        let port = socket
            .local_addr()
            .expect("Failed to get local addr")
            .port();
        // Port is now in use (socket is held open)

        let config = make_test_config("udp", port, "127.0.0.1");
        let monitor_config = MonitorConfig::default();
        let checker = HealthChecker::new(monitor_config);

        // UDP health check should return true (port is in use = forward is alive)
        let result = futures::executor::block_on(async {
            checker
                .check_port_health(
                    &config,
                    Duration::from_secs(1),
                    Duration::from_secs(1),
                    1,
                    Duration::ZERO,
                )
                .await
        });

        assert!(
            result,
            "UDP health check should return true when port is in use (forward is alive)"
        );

        // Explicitly drop socket to clean up
        drop(socket);
    }

    #[test]
    fn test_udp_health_check_returns_false_when_local_port_none() {
        let mut config = make_test_config("udp", 9999, "127.0.0.1");
        config.local_port = None;

        let monitor_config = MonitorConfig::default();
        let checker = HealthChecker::new(monitor_config);

        // Health check should return false when local_port is None
        let result = futures::executor::block_on(async {
            checker
                .check_port_health(
                    &config,
                    Duration::from_secs(1),
                    Duration::from_secs(1),
                    1,
                    Duration::ZERO,
                )
                .await
        });

        assert!(
            !result,
            "UDP health check should return false when local_port is None"
        );
    }

    #[test]
    fn test_udp_health_check_uses_default_local_address() {
        // Bind a socket to get a free port, then drop it
        let port = {
            let socket = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind socket");
            socket
                .local_addr()
                .expect("Failed to get local addr")
                .port()
        };

        let mut config = make_test_config("udp", port, "127.0.0.1");
        config.local_address = None; // Test default address handling

        let monitor_config = MonitorConfig::default();
        let checker = HealthChecker::new(monitor_config);

        // Should use default "127.0.0.1" and return false (port is free)
        let result = futures::executor::block_on(async {
            checker
                .check_port_health(
                    &config,
                    Duration::from_secs(1),
                    Duration::from_secs(1),
                    1,
                    Duration::ZERO,
                )
                .await
        });

        assert!(
            !result,
            "UDP health check should use default 127.0.0.1 and return false when port is free"
        );
    }
}

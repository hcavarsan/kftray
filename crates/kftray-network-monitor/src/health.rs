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

        if config.protocol.eq_ignore_ascii_case("udp") {
            return udp_port_in_use(local_address, local_port).await;
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

async fn udp_port_in_use(local_address: &str, local_port: u16) -> bool {
    if probe_udp_addr_in_use(local_address, local_port).await {
        return true;
    }
    if local_address == "0.0.0.0" || local_address == "::" {
        return probe_udp_addr_in_use("127.0.0.1", local_port).await;
    }
    false
}

async fn probe_udp_addr_in_use(addr: &str, port: u16) -> bool {
    let socket_addr = format!("{addr}:{port}");
    match tokio::net::UdpSocket::bind(&socket_addr).await {
        Ok(_) => false,
        Err(e) => e.kind() == std::io::ErrorKind::AddrInUse,
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

    async fn run_check(config: &Config) -> bool {
        let checker = HealthChecker::new(MonitorConfig::default());
        checker
            .check_port_health(
                config,
                Duration::from_secs(1),
                Duration::from_secs(1),
                1,
                Duration::ZERO,
            )
            .await
    }

    #[tokio::test]
    async fn test_udp_health_check_returns_false_when_port_free() {
        let port = {
            let socket = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind socket");
            socket.local_addr().unwrap().port()
        };

        let config = make_test_config("udp", port, "127.0.0.1");
        assert!(
            !run_check(&config).await,
            "UDP health check should return false when port is free (forward is dead)"
        );
    }

    #[tokio::test]
    async fn test_udp_health_check_returns_true_when_port_in_use() {
        let socket = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind socket");
        let port = socket.local_addr().unwrap().port();

        let config = make_test_config("udp", port, "127.0.0.1");
        assert!(
            run_check(&config).await,
            "UDP health check should return true when port is in use (forward is alive)"
        );
        drop(socket);
    }

    #[tokio::test]
    async fn test_udp_health_check_returns_false_when_local_port_none() {
        let mut config = make_test_config("udp", 9999, "127.0.0.1");
        config.local_port = None;

        assert!(
            !run_check(&config).await,
            "UDP health check should return false when local_port is None"
        );
    }

    #[tokio::test]
    async fn test_udp_health_check_uses_default_local_address() {
        let port = {
            let socket = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind socket");
            socket.local_addr().unwrap().port()
        };

        let mut config = make_test_config("udp", port, "127.0.0.1");
        config.local_address = None;

        assert!(
            !run_check(&config).await,
            "UDP health check should use default 127.0.0.1 and return false when port is free"
        );
    }

    #[tokio::test]
    async fn test_udp_health_check_wildcard_falls_back_to_loopback() {
        let socket = UdpSocket::bind("127.0.0.1:0").expect("Failed to bind socket");
        let port = socket.local_addr().unwrap().port();

        let config = make_test_config("udp", port, "0.0.0.0");
        assert!(
            run_check(&config).await,
            "UDP health check on 0.0.0.0 should detect a 127.0.0.1 listener via fallback"
        );
        drop(socket);
    }
}

use std::fmt;
use std::time::{
    Duration,
    Instant,
};

use kftray_commons::models::config_model::Config;

#[derive(Debug)]
pub enum NetworkMonitorError {
    AlreadyRunning,
    NotRunning,
    StartupFailed(String),
    ShutdownFailed(String),
}

impl fmt::Display for NetworkMonitorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetworkMonitorError::AlreadyRunning => write!(f, "Network monitor is already running"),
            NetworkMonitorError::NotRunning => write!(f, "Network monitor is not running"),
            NetworkMonitorError::StartupFailed(err) => {
                write!(f, "Failed to start network monitor: {err}")
            }
            NetworkMonitorError::ShutdownFailed(err) => {
                write!(f, "Failed to stop network monitor: {err}")
            }
        }
    }
}

impl std::error::Error for NetworkMonitorError {}

#[derive(Clone, Debug)]
pub struct MonitorConfig {
    pub network_timeout: Duration,
    pub health_interval: Duration,
    pub monitor_interval: Duration,
    pub sleep_up: Duration,
    pub sleep_down: Duration,
    pub retry_delay: Duration,
    pub network_endpoints: Vec<&'static str>,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            network_timeout: Duration::from_millis(200),
            health_interval: Duration::from_secs(3),
            monitor_interval: Duration::from_secs(2),
            sleep_up: Duration::from_millis(500),
            sleep_down: Duration::from_millis(100),
            retry_delay: Duration::from_millis(5),
            network_endpoints: vec!["8.8.8.8:53", "1.1.1.1:53", "8.8.4.4:53"],
        }
    }
}

#[derive(Default)]
pub struct TaskState {
    pub reconnect_in_progress: bool,
    pub health_check_in_progress: bool,
    pub last_reconnect: Option<Instant>,
    pub last_health_check: Option<Instant>,
    pub network_stable_since: Option<Instant>,
    pub last_network_state: bool,
}

impl TaskState {
    pub fn should_health_check(&self) -> bool {
        !self.health_check_in_progress
            && self
                .last_health_check
                .is_none_or(|last| last.elapsed() > Duration::from_secs(2))
    }

    pub fn update_network_state(&mut self, is_up: bool) {
        if is_up != self.last_network_state {
            self.last_network_state = is_up;
            if is_up {
                self.network_stable_since = Some(Instant::now());
            } else {
                self.network_stable_since = None;
            }
        }
    }

    pub fn start_reconnect(&mut self) {
        self.reconnect_in_progress = true;
        self.last_reconnect = Some(Instant::now());
        self.network_stable_since = Some(Instant::now());
    }

    pub fn finish_reconnect(&mut self) {
        self.reconnect_in_progress = false;
    }

    pub fn start_health_check(&mut self) {
        self.health_check_in_progress = true;
        self.last_health_check = Some(Instant::now());
    }

    pub fn finish_health_check(&mut self) {
        self.health_check_in_progress = false;
    }
}

pub struct HealthCheckResult {
    pub config: Config,
    pub is_healthy: bool,
}

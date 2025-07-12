use std::fmt;

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

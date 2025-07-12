use std::sync::Arc;
use std::time::{
    Duration,
    Instant,
};

use kftray_commons::models::config_model::Config;
use kftray_portforward::port_forward::CANCEL_NOTIFIER;
use log::{
    error,
    info,
    warn,
};
use tokio::sync::Mutex;
use tokio::time::{
    sleep,
    timeout,
};

const NETWORK_TIMEOUT: Duration = Duration::from_millis(200);
const HEALTH_INTERVAL: Duration = Duration::from_secs(3);
const MONITOR_INTERVAL: Duration = Duration::from_secs(2);
const SLEEP_UP: Duration = Duration::from_millis(500);
const SLEEP_DOWN: Duration = Duration::from_millis(100);

const NETWORK_ENDPOINTS: &[&str] = &["8.8.8.8:53", "1.1.1.1:53", "8.8.4.4:53"];

static TASK_STATE: tokio::sync::OnceCell<Arc<Mutex<TaskState>>> =
    tokio::sync::OnceCell::const_new();

#[derive(Default)]
struct TaskState {
    reconnect_in_progress: bool,
    health_check_in_progress: bool,
    last_reconnect: Option<Instant>,
    last_health_check: Option<Instant>,
}

impl TaskState {
    fn should_reconnect(&self) -> bool {
        !self.reconnect_in_progress
            && self
                .last_reconnect
                .is_none_or(|last| last.elapsed() > Duration::from_secs(1))
    }

    fn should_health_check(&self) -> bool {
        !self.health_check_in_progress
            && self
                .last_health_check
                .is_none_or(|last| last.elapsed() > Duration::from_secs(2))
    }

    fn start_reconnect(&mut self) {
        self.reconnect_in_progress = true;
        self.last_reconnect = Some(Instant::now());
    }

    fn finish_reconnect(&mut self) {
        self.reconnect_in_progress = false;
    }

    fn start_health_check(&mut self) {
        self.health_check_in_progress = true;
        self.last_health_check = Some(Instant::now());
    }

    fn finish_health_check(&mut self) {
        self.health_check_in_progress = false;
    }
}

pub async fn start_network_monitor() {
    info!("Starting network monitor");

    let _background_handle = tokio::spawn(background_monitor());

    let mut network_up = check_network().await;
    let mut failure_count = 0;
    let mut last_health = Instant::now();
    let mut last_fast = Instant::now();

    loop {
        sleep(get_sleep_duration(network_up, failure_count)).await;

        let is_up = check_network().await;

        if !network_up && is_up {
            info!("Network reconnected");
            failure_count = 0;
            handle_reconnect().await;
            last_health = Instant::now();
        } else if network_up && !is_up {
            info!("Network disconnected");
            failure_count += 1;
        }

        if network_up && last_health.elapsed() > HEALTH_INTERVAL {
            tokio::spawn(check_health());
            last_health = Instant::now();
        }

        if network_up && failure_count > 0 && last_fast.elapsed() > SLEEP_UP {
            let failed_configs = check_health_fast().await;
            last_fast = Instant::now();
            if failed_configs.is_empty() {
                failure_count = failure_count.saturating_sub(1);
            }
        }

        network_up = is_up;
    }
}

fn get_sleep_duration(network_up: bool, failure_count: u32) -> Duration {
    match (network_up, failure_count) {
        (true, 0) => SLEEP_UP,
        (true, _) => NETWORK_TIMEOUT,
        (false, _) => SLEEP_DOWN,
    }
}

async fn check_network() -> bool {
    let checks: Vec<_> = NETWORK_ENDPOINTS
        .iter()
        .map(|&endpoint| {
            tokio::spawn(async move {
                timeout(NETWORK_TIMEOUT, tokio::net::TcpStream::connect(endpoint))
                    .await
                    .is_ok()
            })
        })
        .collect();

    let mut success_count = 0;
    for check in checks {
        if matches!(timeout(SLEEP_UP, check).await, Ok(Ok(true))) {
            success_count += 1;
        }
    }

    success_count >= 1
}

async fn check_port_health(
    config: &Config, conn_timeout: Duration, task_timeout: Duration, attempts: usize,
    retry_delay: Duration,
) -> bool {
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
            Ok(Ok(false)) if attempt < attempts - 1 => {
                if retry_delay > Duration::ZERO {
                    sleep(retry_delay).await;
                }
            }
            _ => return false,
        }
    }

    false
}

async fn check_port_forward_health(config: &Config) -> bool {
    check_port_health(config, NETWORK_TIMEOUT, SLEEP_UP, 1, Duration::ZERO).await
}

async fn is_port_listening(address: &str, port: u16) -> bool {
    use std::net::{
        SocketAddr,
        TcpListener,
    };

    let addr: SocketAddr = match format!("{address}:{port}").parse() {
        Ok(addr) => addr,
        Err(_) => return false,
    };

    match TcpListener::bind(addr) {
        Ok(listener) => {
            drop(listener);
            false
        }
        Err(err) => match err.kind() {
            std::io::ErrorKind::AddrInUse => true,
            other => {
                warn!("port-check bind failed: {other}");
                false
            }
        },
    }
}

async fn validate_port_forwards(configs: &[Config]) -> Vec<Config> {
    if configs.is_empty() {
        return Vec::new();
    }

    let health_checks: Vec<_> = configs
        .iter()
        .map(|config| {
            let config_clone = config.clone();
            tokio::spawn(async move {
                let is_healthy = check_port_forward_health(&config_clone).await;
                (config_clone, is_healthy)
            })
        })
        .collect();

    let mut failed_configs = Vec::new();
    for health_check in health_checks {
        if let Ok((config, false)) = health_check.await {
            info!(
                "Port forward failed: config {} at {}:{}",
                config.id.unwrap_or(-1),
                config.local_address.as_deref().unwrap_or("127.0.0.1"),
                config.local_port.unwrap_or(0)
            );
            failed_configs.push(config);
        }
    }

    failed_configs
}

async fn validate_port_forwards_fast(configs: &[Config]) -> Vec<Config> {
    if configs.is_empty() {
        return Vec::new();
    }

    let health_checks: Vec<_> = configs
        .iter()
        .map(|config| {
            let config_clone = config.clone();
            tokio::spawn(async move {
                let is_healthy = check_port_health(
                    &config_clone,
                    SLEEP_DOWN,
                    NETWORK_TIMEOUT,
                    3,
                    Duration::from_millis(5),
                )
                .await;
                (config_clone, is_healthy)
            })
        })
        .collect();

    let mut failed_configs = Vec::new();
    for health_check in health_checks {
        if let Ok((config, false)) = health_check.await {
            info!(
                "Fast check - Port forward failed: config {} at {}:{}",
                config.id.unwrap_or(-1),
                config.local_address.as_deref().unwrap_or("127.0.0.1"),
                config.local_port.unwrap_or(0)
            );
            failed_configs.push(config);
        }
    }

    failed_configs
}

async fn handle_reconnect() {
    info!("Handling network reconnection");

    let active_configs = match get_active_configs().await {
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

    CANCEL_NOTIFIER.notify_waiters();
    sleep(SLEEP_UP).await;

    info!("Restarting {} port forwards", active_configs.len());

    let http_log_state = Arc::new(kftray_http_logs::HttpLogState::new());

    for protocol in ["tcp", "udp"] {
        let protocol_configs: Vec<Config> = active_configs
            .iter()
            .filter(|c| c.protocol == protocol)
            .cloned()
            .collect();

        if !protocol_configs.is_empty() {
            restart_batch(protocol_configs, protocol, http_log_state.clone()).await;
        }
    }
}

async fn restart_batch(
    configs: Vec<Config>, protocol: &str, http_log_state: Arc<kftray_http_logs::HttpLogState>,
) {
    info!("Restarting {} {} port forwards", configs.len(), protocol);

    let stop_tasks: Vec<_> = configs
        .iter()
        .filter_map(|config| {
            config.id.map(|config_id| {
                tokio::spawn(async move {
                    kftray_portforward::kube::stop_port_forward(config_id.to_string()).await
                })
            })
        })
        .collect();

    for stop_task in stop_tasks {
        let _ = stop_task.await;
    }

    sleep(NETWORK_TIMEOUT).await;

    match kftray_portforward::kube::start_port_forward(configs, protocol, http_log_state).await {
        Ok(_) => info!("Successfully restarted {protocol} port forwards"),
        Err(e) => error!("Failed to restart {protocol} port forwards: {e}"),
    }
}

async fn get_active_configs() -> Result<Vec<Config>, Box<dyn std::error::Error + Send + Sync>> {
    let config_states = kftray_commons::utils::config_state::get_configs_state().await?;

    let active_config_ids: Vec<i64> = config_states
        .into_iter()
        .filter(|state| state.is_running)
        .map(|state| state.config_id)
        .collect();

    if active_config_ids.is_empty() {
        return Ok(Vec::new());
    }

    let config_futures: Vec<_> = active_config_ids
        .into_iter()
        .map(|config_id| {
            tokio::spawn(async move { kftray_commons::config::get_config(config_id).await.ok() })
        })
        .collect();

    let mut configs = Vec::new();
    for config_future in config_futures {
        if let Ok(Some(config)) = config_future.await {
            configs.push(config);
        }
    }

    Ok(configs)
}

async fn check_health() {
    let active_configs = match get_active_configs().await {
        Ok(configs) => configs,
        Err(_) => return,
    };

    if active_configs.is_empty() {
        return;
    }

    let failed_configs = validate_port_forwards(&active_configs).await;

    if !failed_configs.is_empty() {
        let mut confirmed_failed = Vec::new();
        for config in failed_configs {
            sleep(SLEEP_DOWN).await;
            if !check_port_forward_health(&config).await {
                confirmed_failed.push(config);
            }
        }

        if !confirmed_failed.is_empty() {
            info!("Restarting {} failed port forwards", confirmed_failed.len());
            restart_failed_configs(confirmed_failed).await;
        }
    }
}

async fn check_health_fast() -> Vec<Config> {
    let active_configs = match get_active_configs().await {
        Ok(configs) => configs,
        Err(_) => return Vec::new(),
    };

    if active_configs.is_empty() {
        return Vec::new();
    }

    let failed_configs = validate_port_forwards_fast(&active_configs).await;

    if !failed_configs.is_empty() {
        info!(
            "Fast check found {} failed port forwards",
            failed_configs.len()
        );
        restart_failed_configs(failed_configs.clone()).await;
    }

    failed_configs
}

async fn background_monitor() {
    info!("Starting background monitor");
    let mut last_check = Instant::now();
    let mut last_network_state = check_network().await;
    let mut last_network_info = get_network_info().await;

    loop {
        sleep(MONITOR_INTERVAL).await;

        let current_network_info = get_network_info().await;
        if current_network_info != last_network_info {
            info!("Network interface change detected");
            if should_start_reconnect().await {
                tokio::spawn(handle_reconnect_with_state());
            }
            last_network_info = current_network_info;
        }

        let current_network_state = check_network().await;
        if current_network_state != last_network_state {
            info!("Network state change detected: {last_network_state} -> {current_network_state}");
            if current_network_state {
                info!("Network recovered");
                if should_start_reconnect().await {
                    tokio::spawn(handle_reconnect_with_state());
                }
            }
            last_network_state = current_network_state;
        }

        if last_check.elapsed() > HEALTH_INTERVAL {
            if should_start_health_check().await {
                tokio::spawn(check_health_with_state());
            }
            last_check = Instant::now();
        }
    }
}

async fn get_network_info() -> String {
    let mut fingerprint = Vec::new();

    for endpoint in NETWORK_ENDPOINTS {
        if let Ok(Ok(socket)) = timeout(SLEEP_DOWN, tokio::net::TcpStream::connect(endpoint)).await
        {
            if let Ok(local_addr) = socket.local_addr() {
                fingerprint.push(local_addr.ip().to_string());
                break;
            }
        }
    }

    let mut route_count = 0;
    for test_ip in NETWORK_ENDPOINTS {
        if timeout(SLEEP_DOWN, tokio::net::TcpStream::connect(test_ip))
            .await
            .is_ok()
        {
            route_count += 1;
        }
    }
    fingerprint.push(route_count.to_string());

    if fingerprint.is_empty() {
        format!(
            "no_network_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        )
    } else {
        fingerprint.join("_")
    }
}

async fn restart_failed_configs(configs: Vec<Config>) {
    let http_log_state = Arc::new(kftray_http_logs::HttpLogState::new());

    for protocol in ["tcp", "udp"] {
        let protocol_configs: Vec<Config> = configs
            .iter()
            .filter(|c| c.protocol == protocol)
            .cloned()
            .collect();

        if !protocol_configs.is_empty() {
            restart_batch(protocol_configs, protocol, http_log_state.clone()).await;
        }
    }
}

async fn get_task_state() -> Arc<Mutex<TaskState>> {
    TASK_STATE
        .get_or_init(|| async { Arc::new(Mutex::new(TaskState::default())) })
        .await
        .clone()
}

async fn should_start_reconnect() -> bool {
    let state = get_task_state().await;
    let guard = state.lock().await;
    guard.should_reconnect()
}

async fn should_start_health_check() -> bool {
    let state = get_task_state().await;
    let guard = state.lock().await;
    guard.should_health_check()
}

async fn handle_reconnect_with_state() {
    let state = get_task_state().await;
    {
        let mut guard = state.lock().await;
        guard.start_reconnect();
    }

    handle_reconnect().await;

    {
        let mut guard = state.lock().await;
        guard.finish_reconnect();
    }
}

async fn check_health_with_state() {
    let state = get_task_state().await;
    {
        let mut guard = state.lock().await;
        guard.start_health_check();
    }

    check_health().await;

    {
        let mut guard = state.lock().await;
        guard.finish_health_check();
    }
}

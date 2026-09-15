pub mod client;
pub mod http_log_watcher;
pub mod listener;
pub mod models;
pub mod operations;
pub(crate) mod proxy;
pub mod proxy_recovery;
mod service;
pub mod shared_client;
mod start;
pub(crate) mod stop;
pub mod target;
pub mod tcp_forwarder;
pub mod udp_forwarder;

pub use http_log_watcher::{
    HttpLogStateEvent,
    HttpLogStateWatcher,
};
pub use listener::{
    ListenerConfig,
    PortForwarder,
    Protocol,
};
pub use proxy::{
    INSTALLATION_LABEL,
    deploy_and_forward_pod,
    deploy_and_forward_pod_with_mode,
    proxy_resource_prefix,
    stop_proxy_forward,
    stop_proxy_forward_with_mode,
};
pub use proxy_recovery::recovery_in_progress;
pub use service::retrieve_service_configs;
pub use start::{
    cleanup_stale_timeout_entries,
    clear_stopped_by_timeout,
    is_stopped_by_timeout,
    start_port_forward,
    start_port_forward_with_mode,
};
pub use stop::{
    UNCERTAIN_CREATE_WINDOW,
    cancel_all_startups,
    delete_configs_if_idle,
    reconcile_pending_cleanup,
    settle_cluster_obligation,
    stop_all_port_forward,
    stop_all_port_forward_with_deadline,
    stop_all_port_forward_with_mode,
    stop_all_port_forward_with_mode_excluding,
    stop_port_forward,
    stop_port_forward_with_mode,
};
pub use target::NO_READY_PODS_ERROR;

/// Whether a startup for `config_id` is currently registered, queued or
/// running, regardless of workload type. Covers both the TCP-direct path in
/// `start.rs` and the relay-pod path in `proxy.rs`, since both register
/// through `proxy::register_start_batch`.
pub fn is_start_pending(config_id: i64) -> bool {
    proxy::STARTING_PROXIES.contains_key(&config_id)
}

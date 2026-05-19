use dashmap::DashSet;
use log::{
    debug,
    error,
    info,
};
use once_cell::sync::Lazy;

pub static STOPPED_BY_TIMEOUT: Lazy<DashSet<i64>> = Lazy::new(DashSet::new);

pub fn clear_stopped_by_timeout(config_id: i64) {
    STOPPED_BY_TIMEOUT.remove(&config_id);
}

pub fn is_stopped_by_timeout(config_id: i64) -> bool {
    STOPPED_BY_TIMEOUT.contains(&config_id)
}

pub async fn cleanup_stale_timeout_entries() {
    use kftray_commons::utils::config::get_configs;

    if let Ok(configs) = get_configs().await {
        let valid_ids: std::collections::HashSet<i64> =
            configs.iter().filter_map(|c| c.id).collect();

        STOPPED_BY_TIMEOUT.retain(|id| valid_ids.contains(id));
        debug!(
            "Cleaned up stale timeout entries, {} remaining",
            STOPPED_BY_TIMEOUT.len()
        );
    }
}

pub(super) async fn handle_timeout_callback(id: i64) {
    info!("User-configured timeout reached for config {id}, stopping port forward");

    STOPPED_BY_TIMEOUT.insert(id);

    if let Err(e) = crate::kube::stop::stop_port_forward(id.to_string()).await {
        error!("Failed to stop port forward {id} on timeout: {e}");
        STOPPED_BY_TIMEOUT.remove(&id);
    } else {
        info!("Port forward {id} stopped due to user-configured timeout");
    }
}

pub(super) fn create_static_timeout_callback() -> std::sync::Arc<dyn Fn(i64) + Send + Sync> {
    std::sync::Arc::new(move |id: i64| {
        let handle = tokio::spawn(async move {
            handle_timeout_callback(id).await;
        });
        tokio::spawn(async move {
            if let Err(e) = handle.await {
                error!("Timeout callback task for config {id} panicked: {e}");
            }
        });
    })
}

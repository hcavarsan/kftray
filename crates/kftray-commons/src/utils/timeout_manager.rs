use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use log::info;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio::time::sleep;

use crate::utils::settings::get_disconnect_timeout;

lazy_static::lazy_static! {
    static ref TIMEOUT_HANDLES: Arc<RwLock<HashMap<i64, JoinHandle<()>>>> =
        Arc::new(RwLock::new(HashMap::new()));
}

pub async fn start_timeout_for_forward(
    config_id: i64, stop_callback: Arc<dyn Fn(i64) + Send + Sync>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Some(timeout_minutes) = get_disconnect_timeout().await? {
        if timeout_minutes > 0 {
            info!("Starting timeout for config_id {config_id} ({timeout_minutes} minutes)");

            let timeout_duration = Duration::from_secs((timeout_minutes as u64) * 60);
            let stop_callback_clone = stop_callback.clone();

            let timeout_handle = tokio::spawn(async move {
                sleep(timeout_duration).await;
                info!("Timeout reached for config_id {config_id}, stopping port forward");
                stop_callback_clone(config_id);
            });

            let mut timeouts = TIMEOUT_HANDLES.write().await;
            timeouts.insert(config_id, timeout_handle);
        }
    }
    Ok(())
}

pub async fn cancel_timeout_for_forward(config_id: i64) {
    let mut timeouts = TIMEOUT_HANDLES.write().await;
    if let Some(handle) = timeouts.remove(&config_id) {
        handle.abort();
        info!("Cancelled timeout for config_id {config_id}");
    }
}

pub async fn get_active_timeout_count() -> usize {
    let timeouts = TIMEOUT_HANDLES.read().await;
    timeouts.len()
}

pub async fn get_timeout_info_for_forward(config_id: i64) -> Option<u32> {
    let timeouts = TIMEOUT_HANDLES.read().await;
    if timeouts.contains_key(&config_id) {
        get_disconnect_timeout().await.unwrap_or(None)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{
        AtomicBool,
        Ordering,
    };

    use tokio::time::{
        sleep,
        Duration,
    };

    use super::*;

    #[tokio::test]
    async fn test_timeout_functions() {
        let callback_called = Arc::new(AtomicBool::new(false));
        let callback_called_clone = callback_called.clone();

        let callback = Arc::new(move |_config_id: i64| {
            callback_called_clone.store(true, Ordering::Relaxed);
        });

        let result = start_timeout_for_forward(1, callback).await;
        assert!(result.is_ok());

        cancel_timeout_for_forward(1).await;

        sleep(Duration::from_millis(10)).await;

        assert!(!callback_called.load(Ordering::Relaxed));
    }
}

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use log::info;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio::time::{
    Instant,
    sleep_until,
};

use crate::utils::db_mode::DatabaseMode;
use crate::utils::settings::get_disconnect_timeout_with_mode;

type StopCallback = Arc<dyn Fn(i64) + Send + Sync>;

struct ForwardTimeout {
    started: Instant,
    stop_callback: StopCallback,
    mode: DatabaseMode,
    timer: Option<JoinHandle<()>>,
}

lazy_static::lazy_static! {
    static ref TIMEOUTS: Arc<RwLock<HashMap<i64, ForwardTimeout>>> =
        Arc::new(RwLock::new(HashMap::new()));
}

fn spawn_timer(
    config_id: i64, started: Instant, timeout_minutes: u32, stop_callback: StopCallback,
) -> Option<JoinHandle<()>> {
    if timeout_minutes == 0 {
        return None;
    }
    info!("Starting timeout for config_id {config_id} ({timeout_minutes} minutes)");
    let deadline = started + Duration::from_secs(u64::from(timeout_minutes) * 60);
    Some(tokio::spawn(async move {
        sleep_until(deadline).await;
        info!("Timeout reached for config_id {config_id}, stopping port forward");
        stop_callback(config_id);
    }))
}

pub async fn start_timeout_for_forward(
    config_id: i64, stop_callback: StopCallback, mode: DatabaseMode,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut timeouts = TIMEOUTS.write().await;
    let timeout_minutes = get_disconnect_timeout_with_mode(mode).await?.unwrap_or(0);
    let started = Instant::now();
    let timer = spawn_timer(config_id, started, timeout_minutes, stop_callback.clone());
    let timeout = ForwardTimeout {
        started,
        stop_callback,
        mode,
        timer,
    };
    if let Some(replaced) = timeouts.insert(config_id, timeout)
        && let Some(timer) = replaced.timer
    {
        timer.abort();
    }
    Ok(())
}

pub async fn restart_timeouts_for_running_forwards(
    mode: DatabaseMode,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut timeouts = TIMEOUTS.write().await;
    let timeout_minutes = get_disconnect_timeout_with_mode(mode).await?.unwrap_or(0);
    for (&config_id, timeout) in timeouts.iter_mut().filter(|(_, t)| t.mode == mode) {
        if let Some(timer) = timeout.timer.take() {
            timer.abort();
        }
        timeout.timer = spawn_timer(
            config_id,
            timeout.started,
            timeout_minutes,
            timeout.stop_callback.clone(),
        );
    }
    Ok(())
}

pub async fn cancel_timeout_for_forward(config_id: i64) {
    let mut timeouts = TIMEOUTS.write().await;
    if let Some(timeout) = timeouts.remove(&config_id)
        && let Some(timer) = timeout.timer
    {
        timer.abort();
        info!("Cancelled timeout for config_id {config_id}");
    }
}

pub async fn get_active_timeout_count() -> usize {
    let timeouts = TIMEOUTS.read().await;
    timeouts.values().filter(|t| t.timer.is_some()).count()
}

pub async fn get_timeout_info_for_forward(config_id: i64, mode: DatabaseMode) -> Option<u32> {
    let timeouts = TIMEOUTS.read().await;
    if timeouts.get(&config_id).is_some_and(|t| t.timer.is_some()) {
        get_disconnect_timeout_with_mode(mode).await.unwrap_or(None)
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

    use sqlx::SqlitePool;
    use tokio::time::{
        Duration,
        sleep,
    };

    use super::*;
    use crate::utils::db::{
        create_db_table,
        set_db_pool,
    };
    use crate::utils::db_mode::DatabaseMode;
    use crate::utils::settings::{
        set_disconnect_timeout,
        set_disconnect_timeout_with_mode,
    };

    async fn setup_test_db() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        create_db_table(&pool).await.unwrap();
        let arc_pool = Arc::new(pool);
        set_db_pool(arc_pool);
    }

    #[tokio::test]
    async fn test_timeout_functions() {
        let _db = crate::test_utils::test_db().await;
        setup_test_db().await;
        set_disconnect_timeout(1).await.unwrap(); // Set 1 minute timeout

        let callback_called = Arc::new(AtomicBool::new(false));
        let callback_called_clone = callback_called.clone();

        let callback = Arc::new(move |_config_id: i64| {
            callback_called_clone.store(true, Ordering::Relaxed);
        });

        let result = start_timeout_for_forward(1, callback, DatabaseMode::File).await;
        assert!(result.is_ok());

        // Verify timeout was started
        assert_eq!(get_active_timeout_count().await, 1);
        assert_eq!(
            get_timeout_info_for_forward(1, DatabaseMode::File).await,
            Some(1)
        );

        // Cancel timeout
        cancel_timeout_for_forward(1).await;

        // Verify timeout was cancelled
        assert_eq!(get_active_timeout_count().await, 0);
        assert_eq!(
            get_timeout_info_for_forward(1, DatabaseMode::File).await,
            None
        );

        sleep(Duration::from_millis(10)).await;

        assert!(!callback_called.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn a_memory_mode_forward_uses_the_memory_mode_timeout() {
        let _db = crate::test_utils::test_db().await;
        setup_test_db().await;
        set_disconnect_timeout(1).await.unwrap();
        set_disconnect_timeout_with_mode(5, DatabaseMode::Memory)
            .await
            .unwrap();

        start_timeout_for_forward(42, Arc::new(|_config_id: i64| {}), DatabaseMode::Memory)
            .await
            .unwrap();
        let minutes = get_timeout_info_for_forward(42, DatabaseMode::Memory).await;
        cancel_timeout_for_forward(42).await;

        assert_eq!(minutes, Some(5));
    }

    async fn stop_flag_after_minutes(
        config_id: i64, minutes_at_start: u32, minutes_saved_later: u32,
    ) -> Arc<AtomicBool> {
        set_disconnect_timeout_with_mode(minutes_at_start, DatabaseMode::Memory)
            .await
            .unwrap();
        let stopped = Arc::new(AtomicBool::new(false));
        let flag = stopped.clone();
        start_timeout_for_forward(
            config_id,
            Arc::new(move |_config_id: i64| flag.store(true, Ordering::SeqCst)),
            DatabaseMode::Memory,
        )
        .await
        .unwrap();
        set_disconnect_timeout_with_mode(minutes_saved_later, DatabaseMode::Memory)
            .await
            .unwrap();
        stopped
    }

    async fn advance_paused_clock(duration: Duration) {
        tokio::time::pause();
        tokio::time::advance(duration).await;
        tokio::task::yield_now().await;
        tokio::time::resume();
    }

    #[tokio::test]
    async fn a_zero_timeout_saved_while_a_forward_runs_keeps_it_running() {
        let _db = crate::test_utils::test_db().await;
        setup_test_db().await;
        let stopped = stop_flag_after_minutes(43, 1, 0).await;

        advance_paused_clock(Duration::from_secs(61)).await;
        let stopped_after_61s = stopped.load(Ordering::SeqCst);
        cancel_timeout_for_forward(43).await;

        assert!(!stopped_after_61s);
    }

    #[tokio::test]
    async fn a_timeout_saved_while_a_forward_runs_counts_from_its_start() {
        let _db = crate::test_utils::test_db().await;
        setup_test_db().await;
        let stopped = stop_flag_after_minutes(44, 0, 2).await;

        advance_paused_clock(Duration::from_secs(61)).await;
        let stopped_after_61s = stopped.load(Ordering::SeqCst);
        advance_paused_clock(Duration::from_secs(60)).await;
        let stopped_after_121s = stopped.load(Ordering::SeqCst);
        cancel_timeout_for_forward(44).await;

        assert_eq!((stopped_after_61s, stopped_after_121s), (false, true));
    }
}

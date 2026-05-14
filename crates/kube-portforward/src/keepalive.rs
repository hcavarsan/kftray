use std::sync::Arc;
use std::sync::atomic::{
    AtomicI64,
    Ordering,
};
use std::time::{
    Duration,
    SystemTime,
    UNIX_EPOCH,
};

use bytes::Bytes;
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio::time::{
    MissedTickBehavior,
    interval,
};
use tokio_util::sync::CancellationToken;
use tungstenite::Message;

use crate::error::Error;

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum RecoverySignal {
    KeepaliveTimeout {
        last_pong_age: Duration,
    },
    ServerClose,
    NetworkError(String),
    UpgradeFailed {
        status: Option<u16>,
        message: String,
    },
}

/// User-supplied callback invoked on recovery-worthy events. Implementations
/// must be cheap (typically a channel send) — they run on the keepalive /
/// reader tasks.
///
/// This uses `Arc<dyn Fn>` for simplicity. A trait-based approach (e.g.
/// `trait RecoveryHandler`) would allow richer extensibility (async callbacks,
/// typed returns) but would complicate the builder API for minimal gain
/// in the current use cases.
pub type RecoveryCallback = Arc<dyn Fn(RecoverySignal) + Send + Sync>;

const WATCHDOG_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub(crate) struct KeepaliveHandle {
    last_pong: Arc<AtomicI64>,
    cancel: CancellationToken,
}

impl KeepaliveHandle {
    pub(crate) fn note_pong(&self) {
        self.last_pong.store(now_ms(), Ordering::Relaxed);
    }

    pub(crate) fn arm(&self) {
        self.last_pong.store(now_ms(), Ordering::Relaxed);
    }

    #[allow(dead_code)]
    pub(crate) fn last_pong_age(&self) -> Duration {
        let last = self.last_pong.load(Ordering::Relaxed);
        if last == i64::MAX {
            return Duration::ZERO;
        }
        let delta = now_ms().saturating_sub(last);
        Duration::from_millis(u64::try_from(delta).unwrap_or(0))
    }

    #[allow(dead_code)]
    pub(crate) fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }
}

pub(crate) fn spawn_keepalive(
    writer_mailbox: mpsc::Sender<Message>, cancel: CancellationToken,
    recovery_callback: RecoveryCallback, ping_interval: Duration, watchdog_timeout: Duration,
    join_set: &mut JoinSet<Result<(), Error>>,
) -> KeepaliveHandle {
    let last_pong = Arc::new(AtomicI64::new(i64::MAX));

    let heartbeat_cancel = cancel.clone();
    let heartbeat_writer = writer_mailbox.clone();
    join_set.spawn(async move {
        let mut ticker = interval(ping_interval);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                biased;
                _ = heartbeat_cancel.cancelled() => return Ok(()),
                _ = ticker.tick() => {
                    tracing::debug!("WebSocket keepalive heartbeat tick");
                    if heartbeat_writer
                        .send(Message::Ping(Bytes::new()))
                        .await
                        .is_err()
                    {
                        return Ok(());
                    }
                }
            }
        }
    });

    let watchdog_cancel = cancel.clone();
    let watchdog_pong = Arc::clone(&last_pong);
    let watchdog_cb = Arc::clone(&recovery_callback);
    let watchdog_threshold_ms = i64::try_from(watchdog_timeout.as_millis()).unwrap_or(i64::MAX);
    join_set.spawn(async move {
        let mut ticker = interval(WATCHDOG_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                biased;
                _ = watchdog_cancel.cancelled() => return Ok(()),
                _ = ticker.tick() => {
                    let last = watchdog_pong.load(Ordering::Relaxed);
                    if last == i64::MAX {
                        continue;
                    }
                    let elapsed = now_ms().saturating_sub(last);
                    if elapsed > watchdog_threshold_ms {
                        let age = Duration::from_millis(u64::try_from(elapsed).unwrap_or(0));
                        watchdog_cb(RecoverySignal::KeepaliveTimeout { last_pong_age: age });
                        watchdog_cancel.cancel();
                        tracing::warn!(
                            elapsed_ms = elapsed,
                            "WebSocket keepalive timed out: no Pong received within threshold"
                        );
                        return Ok(());
                    }
                }
            }
        }
    });

    KeepaliveHandle { last_pong, cancel }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(i64::MAX)
}

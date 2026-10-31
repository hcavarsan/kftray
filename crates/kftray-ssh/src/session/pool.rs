use std::sync::{
    Arc,
    Weak,
};

use dashmap::DashMap;
use russh::Disconnect;
use russh::client::Handle;
use tokio::sync::{
    Mutex,
    RwLock,
    Semaphore,
    oneshot,
    watch,
};
use tokio_util::sync::CancellationToken;

use super::handler::ClientHandler;
use super::{
    SessionState,
    SshLease,
    Stage,
    establish,
};
use crate::consts::{
    MAX_CHANNELS_PER_SESSION,
    RECONNECT_BACKOFF_MAX,
    RECONNECT_BACKOFF_MIN,
};
use crate::error::SshError;
use crate::host_keys::HostKeyStore;
use crate::key::KftrayKey;
use crate::route::{
    RouteKey,
    SshRoute,
};

/// Sessions shared by every forward that uses the same route.
#[derive(Default)]
pub(crate) struct SshPool {
    // One lock per route, so routes never wait on each other's handshake.
    slots: DashMap<RouteKey, Arc<Mutex<Weak<SessionEntry>>>>,
}

pub(super) struct SessionEntry {
    pub(super) label: String,
    /// Swapped by the supervisor after a reconnect.
    pub(super) handle: RwLock<Arc<Handle<ClientHandler>>>,
    pub(super) state: watch::Sender<SessionState>,
    pub(super) channels: Arc<Semaphore>,
    cancel: CancellationToken,
}

impl Drop for SessionEntry {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let handle = Arc::clone(self.handle.get_mut());
            runtime.spawn(async move {
                let _ = handle.disconnect(Disconnect::ByApplication, "", "en").await;
            });
        }
        log::info!("[ssh {}] closed (no forwards left)", self.label);
    }
}

impl SshPool {
    pub(crate) async fn acquire(
        &self, route: SshRoute, key: &KftrayKey, store: &Arc<dyn HostKeyStore>,
        on_stage: &mut (dyn FnMut(Stage) + Send),
    ) -> Result<SshLease, SshError> {
        let slot = Arc::clone(
            &*self
                .slots
                .entry(route.key.clone())
                .or_insert_with(|| Arc::new(Mutex::new(Weak::new()))),
        );
        let mut slot = slot.lock().await;

        if let Some(entry) = slot.upgrade() {
            let failed = matches!(*entry.state.borrow(), SessionState::Failed(_));
            if !failed {
                return Ok(SshLease(entry));
            }
        }

        // Nothing is stored before this succeeds, so a dropped `acquire` leaks nothing.
        let (handle, closed) = establish(&route, key, store, on_stage).await?;
        let cancel = CancellationToken::new();
        let entry = Arc::new(SessionEntry {
            label: route.label.clone(),
            handle: RwLock::new(Arc::new(handle)),
            state: watch::channel(SessionState::Ready).0,
            channels: Arc::new(Semaphore::new(MAX_CHANNELS_PER_SESSION)),
            cancel: cancel.clone(),
        });
        tokio::spawn(supervise(
            Arc::downgrade(&entry),
            cancel,
            route,
            key.clone(),
            Arc::clone(store),
            closed,
        ));
        *slot = Arc::downgrade(&entry);
        Ok(SshLease(entry))
    }
}

/// Keeps the session alive: when it ends, reconnects with a doubling
/// backoff until it works or fails for good. Holds the entry only weakly,
/// so the last lease still closes the session.
async fn supervise(
    entry: Weak<SessionEntry>, cancel: CancellationToken, route: SshRoute, key: KftrayKey,
    store: Arc<dyn HostKeyStore>, mut closed: oneshot::Receiver<()>,
) {
    let label = route.label.as_str();
    let mut ignore_stage = |_: Stage| {};
    let publish = |state: SessionState| match entry.upgrade() {
        Some(live) => {
            // `send` would drop the value while nobody is subscribed.
            live.state.send_replace(state);
            true
        }
        None => false,
    };

    loop {
        tokio::select! {
            biased;
            () = cancel.cancelled() => return,
            _ = &mut closed => {}
        }
        log::warn!("[ssh {label}] connection lost, reconnecting");

        let mut attempt = 1;
        let mut backoff = RECONNECT_BACKOFF_MIN;
        if !publish(SessionState::Reconnecting { attempt }) {
            return;
        }
        loop {
            let established = tokio::select! {
                biased;
                () = cancel.cancelled() => return,
                result = establish(&route, &key, &store, &mut ignore_stage) => result,
            };
            match established {
                Ok((handle, new_closed)) => {
                    let Some(live) = entry.upgrade() else { return };
                    *live.handle.write().await = Arc::new(handle);
                    live.state.send_replace(SessionState::Ready);
                    log::info!("[ssh {label}] reconnected after {attempt} attempt(s)");
                    closed = new_closed;
                    break;
                }
                Err(e) if !e.is_retryable() => {
                    log::error!("[ssh {label}] cannot reconnect: {e}");
                    publish(SessionState::Failed(e.to_string()));
                    return;
                }
                Err(e) => {
                    log::warn!("[ssh {label}] reconnect attempt {attempt} failed: {e}");
                    attempt += 1;
                    if !publish(SessionState::Reconnecting { attempt }) {
                        return;
                    }
                    tokio::select! {
                        biased;
                        () = cancel.cancelled() => return,
                        () = tokio::time::sleep(backoff) => {}
                    }
                    backoff = (backoff * 2).min(RECONNECT_BACKOFF_MAX);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{
        AtomicBool,
        AtomicUsize,
        Ordering,
    };
    use std::time::Duration;

    use async_trait::async_trait;
    use tokio::io::{
        AsyncReadExt,
        AsyncWriteExt,
    };
    use tokio::time::timeout;

    use super::*;
    use crate::route::{
        Authenticator,
        BoxStream,
        Credentials,
        StreamOpener,
        Transport,
    };
    use crate::test_support::{
        Fixture,
        KeyAuth,
        peer,
        route,
    };
    use crate::testing::DuplexTransport;

    const WAIT: Duration = Duration::from_secs(10);

    async fn assert_echo(opener: &dyn StreamOpener) {
        let mut stream = opener.open(peer()).await.unwrap();
        stream.write_all(b"ping").await.unwrap();
        let mut back = [0; 4];
        stream.read_exact(&mut back).await.unwrap();
        assert_eq!(&back, b"ping");
    }

    async fn wait_for(state: &mut watch::Receiver<SessionState>, wanted: SessionState) {
        timeout(WAIT, state.wait_for(|s| *s == wanted))
            .await
            .unwrap()
            .unwrap();
    }

    /// First connect after `armed` is set fails with a retryable error.
    struct FailOnce {
        inner: DuplexTransport,
        armed: Arc<AtomicBool>,
    }

    #[async_trait]
    impl Transport for FailOnce {
        async fn connect(&self) -> Result<BoxStream, SshError> {
            if self.armed.swap(false, Ordering::SeqCst) {
                return Err(SshError::Transport {
                    route: "retry".to_owned(),
                    reason: "injected".to_owned(),
                });
            }
            self.inner.connect().await
        }
    }

    /// Works once, then the login is refused for good.
    struct OnceThenRejected(AtomicUsize);

    #[async_trait]
    impl Authenticator for OnceThenRejected {
        async fn credentials(&self, key: &KftrayKey) -> Result<Credentials, SshError> {
            if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                return KeyAuth.credentials(key).await;
            }
            Err(SshError::AuthRejected {
                route: "failing".to_owned(),
                user: "kftray".to_owned(),
            })
        }
    }

    struct Never;

    #[async_trait]
    impl Transport for Never {
        async fn connect(&self) -> Result<BoxStream, SshError> {
            std::future::pending().await
        }
    }

    #[tokio::test]
    async fn acquire_should_share_one_session_between_leases_of_the_same_route() {
        let fixture = Fixture::new().await;
        let first = fixture.acquire("shared").await.unwrap();
        let mut stages = Vec::new();

        let second = fixture
            .runtime
            .acquire(route(&fixture.mock, "shared"), &mut |s| stages.push(s))
            .await
            .unwrap();

        assert!(stages.is_empty(), "a shared session reports no stages");
        assert!(Arc::ptr_eq(&first.0, &second.0));
        assert_eq!(fixture.mock.session_count(), 1);
    }

    #[tokio::test]
    async fn acquire_should_open_one_session_per_route() {
        let fixture = Fixture::new().await;

        let _a = fixture.acquire("a").await.unwrap();
        let _b = fixture.acquire("b").await.unwrap();

        assert_eq!(fixture.mock.session_count(), 2);
    }

    #[tokio::test]
    async fn dropping_the_last_lease_should_close_the_session() {
        let fixture = Fixture::new().await;
        let first = fixture.acquire("shared").await.unwrap();
        let second = first.clone();

        drop(first);
        assert_echo(&fixture.echo_opener(&second)).await;
        assert_eq!(fixture.mock.session_count(), 1);

        drop(second);
        assert!(
            fixture.mock.wait_session_count(0).await,
            "{} session(s) left",
            fixture.mock.session_count()
        );
    }

    #[tokio::test]
    async fn acquire_should_leak_nothing_when_it_is_dropped_mid_connect() {
        let fixture = Fixture::new().await;
        let mut stuck = route(&fixture.mock, "stuck");
        stuck.transport = Arc::new(Never);
        let outcome = timeout(
            Duration::from_millis(50),
            fixture.runtime.acquire(stuck, &mut |_| {}),
        )
        .await;
        assert!(outcome.is_err());

        let lease = fixture.acquire("stuck").await.unwrap();

        assert_echo(&fixture.echo_opener(&lease)).await;
        assert_eq!(fixture.mock.session_count(), 1);
    }

    #[tokio::test]
    async fn supervisor_should_retry_with_backoff_when_a_reconnect_fails_transiently() {
        let fixture = Fixture::new().await;
        let armed = Arc::new(AtomicBool::new(false));
        let mut flaky = route(&fixture.mock, "retry");
        flaky.transport = Arc::new(FailOnce {
            inner: DuplexTransport::new(&fixture.mock),
            armed: Arc::clone(&armed),
        });
        let lease = fixture.runtime.acquire(flaky, &mut |_| {}).await.unwrap();
        let mut state = lease.state();

        armed.store(true, Ordering::SeqCst);
        fixture.mock.drop_sessions().await;

        // Sticky for the whole first backoff, so it cannot be missed.
        wait_for(&mut state, SessionState::Reconnecting { attempt: 2 }).await;
        wait_for(&mut state, SessionState::Ready).await;
        assert_echo(&fixture.echo_opener(&lease)).await;
    }

    #[tokio::test]
    async fn supervisor_should_fail_the_session_and_acquire_should_replace_it() {
        let fixture = Fixture::new().await;
        let mut failing = route(&fixture.mock, "failing");
        failing.auth = Arc::new(OnceThenRejected(AtomicUsize::new(0)));
        let lease = fixture.runtime.acquire(failing, &mut |_| {}).await.unwrap();
        let mut state = lease.state();

        fixture.mock.drop_sessions().await;

        let failed = timeout(
            WAIT,
            state.wait_for(|s| matches!(s, SessionState::Failed(_))),
        )
        .await
        .unwrap()
        .unwrap()
        .clone();
        assert!(
            matches!(&failed, SessionState::Failed(why) if why.contains("rejected the login")),
            "{failed:?}"
        );
        let err = fixture
            .echo_opener(&lease)
            .open(peer())
            .await
            .err()
            .unwrap();
        assert!(matches!(err, SshError::Transport { .. }), "{err}");

        let fresh = fixture.acquire("failing").await.unwrap();

        assert!(!Arc::ptr_eq(&lease.0, &fresh.0));
        assert_eq!(*fresh.state().borrow(), SessionState::Ready);
        assert_echo(&fixture.echo_opener(&fresh)).await;
    }
}

use std::fmt::Display;
use std::net::{
    Ipv4Addr,
    SocketAddr,
};
use std::path::PathBuf;
use std::sync::Arc;
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
use tokio::sync::watch;
use tokio::time::timeout;

use super::{
    MemoryHostKeyStore,
    MockSshd,
    MockSshdOptions,
};
use crate::SshRuntime;
use crate::error::{
    ProviderError,
    SshError,
};
use crate::host_keys::HostKeyStore;
use crate::key::KftrayKey;
use crate::route::{
    Authenticator,
    Credentials,
    SshRoute,
    StreamOpener,
};
use crate::session::{
    SessionState,
    Stage,
};

/// Bound for every wait on the session; a provider's transport may need a
/// whole tunnel handshake to reconnect.
const WAIT: Duration = Duration::from_secs(30);

/// Checks that a provider's pieces behave like an SSH route: login, channels
/// to a TCP echo, trust on first use, host key rotation, reconnect after the
/// server drops the session, and a permanent failure on reconnect.
///
/// `build` makes a route to the [`MockSshd`] it is given; it is async because
/// providers talk to their API to build one, and it is called more than once.
/// The runtime's key is authorized on the sshd before `build` runs and is
/// listed by [`MockSshd::authorized_keys`]; a provider that signs
/// certificates can call [`MockSshd::trust_user_ca`].
pub async fn route_conformance(build: impl AsyncFn(&MockSshd) -> SshRoute) {
    let mock = ok(
        MockSshd::start(MockSshdOptions::default()).await,
        "start MockSshd",
    );
    // The port of a live mock is unique, so it names the directory.
    let dir = TempDir::new(mock.addr().port());
    let store = Arc::new(MemoryHostKeyStore::default());
    let runtime = ok(SshRuntime::new(&dir.path, store.clone()), "create runtime");
    mock.authorize(runtime.key().public());

    let control = Control::new();
    let route = control.wrap(build(&mock).await);
    let alias = route.host_keys.alias();
    let vouched = !ok(route.host_keys.trusted().await, "list trusted host keys").is_empty();

    // The session opens and authenticates.
    let mut stages = Vec::new();
    let lease = ok(
        runtime
            .acquire(route, &mut |stage| stages.push(stage))
            .await,
        "acquire a session",
    );
    assert_eq!(stages, [Stage::Transport, Stage::Handshake, Stage::Auth]);
    assert_eq!(*lease.state().borrow(), SessionState::Ready);
    assert_eq!(control.calls(), 1, "credentials() once per new session");

    // Bytes reach the echo server and come back.
    let echo = mock.echo_addr();
    let opener = lease.opener(echo.ip().to_string(), echo.port());
    assert_echo(&opener).await;

    // First use stores the key, unless the provider vouches for the keys.
    let stored = ok(store.get(&alias).await, "read the host key store");
    match (vouched, stored) {
        (true, stored) => assert!(stored.is_none(), "a vouched host key must not be stored"),
        (false, Some(stored)) => assert!(
            stored.key_data() == mock.host_key().key_data(),
            "the stored key is not the host key"
        ),
        (false, None) => panic!("the first host key was not stored"),
    }

    // The server drops the session: reconnect with fresh credentials.
    let mut state = lease.state();
    control.hold(true);
    mock.drop_sessions().await;
    wait_state(&mut state, |s| {
        matches!(s, SessionState::Reconnecting { .. })
    })
    .await;
    control.hold(false);
    wait_state(&mut state, |s| *s == SessionState::Ready).await;
    assert_eq!(
        control.calls(),
        2,
        "credentials() again for the new session"
    );
    assert_echo(&opener).await;

    // A permanent error while reconnecting ends in Failed.
    control.fail(true);
    mock.drop_sessions().await;
    let failed = wait_state(&mut state, |s| matches!(s, SessionState::Failed(_))).await;
    assert_eq!(failed, SessionState::Failed(INJECTED.to_owned()));
    let err = expect_err(opener.open(peer()).await, "open on a failed session");
    assert!(matches!(err, SshError::Transport { .. }), "{err}");

    // A failed session is replaced by the next acquire.
    control.fail(false);
    let second = ok(
        runtime
            .acquire(control.wrap(build(&mock).await), &mut |_| {})
            .await,
        "acquire after a failed session",
    );
    assert_eq!(*second.state().borrow(), SessionState::Ready);
    assert_echo(&second.opener(echo.ip().to_string(), echo.port())).await;

    // A changed host key is refused.
    if !vouched {
        mock.rotate_host_key();
        let fresh = ok(
            SshRuntime::new(&dir.path, store.clone()),
            "create a second runtime",
        );
        let outcome = fresh
            .acquire(control.wrap(build(&mock).await), &mut |_| {})
            .await;
        match outcome {
            Err(SshError::HostKeyMismatch { .. }) => {}
            Err(e) => panic!("expected HostKeyMismatch, got: {e}"),
            Ok(_) => panic!("a rotated host key was accepted"),
        }
    }

    // The last lease closes the session.
    drop((opener, lease, second));
    assert!(
        mock.wait_session_count(0).await,
        "{} session(s) left after the last lease was dropped",
        mock.session_count()
    );
}

const INJECTED: &str = "injected failure";

#[derive(Debug, thiserror::Error)]
#[error("{INJECTED}")]
struct Injected;

impl ProviderError for Injected {
    fn user_message(&self) -> String {
        INJECTED.to_owned()
    }

    fn is_retryable(&self) -> bool {
        false
    }
}

/// Counts `credentials()` calls and can stall or fail them.
struct Control {
    calls: AtomicUsize,
    fail: AtomicBool,
    hold: watch::Sender<bool>,
}

impl Control {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            fail: AtomicBool::new(false),
            hold: watch::channel(false).0,
        })
    }

    fn wrap(self: &Arc<Self>, route: SshRoute) -> SshRoute {
        SshRoute {
            auth: Arc::new(Instrumented {
                control: Arc::clone(self),
                inner: route.auth,
            }),
            ..route
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn fail(&self, fail: bool) {
        self.fail.store(fail, Ordering::SeqCst);
    }

    /// While held, `credentials()` waits, which pins the session in
    /// `Reconnecting` until the test looks at it.
    fn hold(&self, hold: bool) {
        self.hold.send_replace(hold);
    }
}

struct Instrumented {
    control: Arc<Control>,
    inner: Arc<dyn Authenticator>,
}

#[async_trait]
impl Authenticator for Instrumented {
    async fn credentials(&self, key: &KftrayKey) -> Result<Credentials, SshError> {
        self.control.calls.fetch_add(1, Ordering::SeqCst);
        let mut hold = self.control.hold.subscribe();
        let _ = hold.wait_for(|held| !*held).await;
        if self.control.fail.load(Ordering::SeqCst) {
            return Err(SshError::provider("conformance", Injected));
        }
        self.inner.credentials(key).await
    }
}

fn peer() -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, 50_000))
}

async fn assert_echo(opener: &dyn StreamOpener) {
    let mut stream = ok(opener.open(peer()).await, "open a channel");
    let payload: Vec<u8> = (0..64).collect();
    ok(stream.write_all(&payload).await, "write to the channel");
    let mut back = vec![0; payload.len()];
    ok(
        ok(
            timeout(WAIT, stream.read_exact(&mut back)).await,
            "echo timed out",
        ),
        "read from the channel",
    );
    assert_eq!(back, payload);
}

/// Waits for a state that satisfies `wanted`, and returns it.
async fn wait_state(
    state: &mut watch::Receiver<SessionState>, wanted: impl Fn(&SessionState) -> bool,
) -> SessionState {
    let reached = ok(
        timeout(WAIT, state.wait_for(wanted)).await,
        "state wait timed out",
    );
    let reached = ok(reached, "session state channel closed");
    reached.clone()
}

#[track_caller]
fn ok<T, E: Display>(result: Result<T, E>, what: &str) -> T {
    match result {
        Ok(value) => value,
        Err(e) => panic!("{what}: {e}"),
    }
}

#[track_caller]
fn expect_err<T>(result: Result<T, SshError>, what: &str) -> SshError {
    match result {
        Ok(_) => panic!("{what}: expected an error"),
        Err(e) => e,
    }
}

/// The runtime's data directory, removed when the check ends.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(unique: u16) -> Self {
        let path = std::env::temp_dir().join(format!(
            "kftray-ssh-conformance-{}-{unique}",
            std::process::id()
        ));
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

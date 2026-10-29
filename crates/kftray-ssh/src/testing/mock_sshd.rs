use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::sync::atomic::{
    AtomicU64,
    Ordering,
};
use std::sync::{
    Arc,
    Mutex,
    MutexGuard,
    PoisonError,
};
use std::time::Duration;

use russh::keys::ssh_key::private::Ed25519Keypair;
use russh::keys::{
    Certificate,
    HashAlg,
    PrivateKey,
    PublicKey,
};
use russh::server::{
    self,
    Auth,
    ChannelOpenHandle,
    Handler,
    Msg,
    Session,
    VerifiedPublicKey,
};
use russh::{
    Channel,
    ChannelOpenFailure,
    Disconnect,
};
use tokio::io::{
    AsyncRead,
    AsyncWrite,
    copy,
    copy_bidirectional,
};
use tokio::net::{
    TcpListener,
    TcpStream,
};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::timeout;

const SESSION_WAIT: Duration = Duration::from_secs(10);

pub struct MockSshdOptions {
    /// Keys accepted for public-key login.
    pub authorized: Vec<PublicKey>,
    /// CA whose user certificates are accepted for certificate login.
    pub user_ca: Option<PublicKey>,
    /// `false` makes the sshd refuse every `direct-tcpip` channel, like
    /// `AllowTcpForwarding no`.
    pub forwarding: bool,
}

impl Default for MockSshdOptions {
    fn default() -> Self {
        Self {
            authorized: Vec::new(),
            user_ca: None,
            forwarding: true,
        }
    }
}

/// An in-process sshd: key and certificate login, `direct-tcpip` channels
/// connected for real, reachable over loopback TCP ([`addr`](Self::addr))
/// and over in-memory streams ([`DuplexTransport`](super::DuplexTransport)).
pub struct MockSshd {
    shared: Arc<Shared>,
    addr: SocketAddr,
    echo_addr: SocketAddr,
    tasks: [JoinHandle<()>; 2],
}

pub(super) struct Shared {
    forwarding: bool,
    authorized: Mutex<Vec<PublicKey>>,
    user_cas: Mutex<Vec<PublicKey>>,
    host: Mutex<Host>,
    sessions: watch::Sender<usize>,
    live: Mutex<HashMap<u64, server::Handle>>,
    next_id: AtomicU64,
}

struct Host {
    config: Arc<server::Config>,
    public: PublicKey,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn random_host() -> Host {
    let key = PrivateKey::from(Ed25519Keypair::random(&mut russh::keys::key::safe_rng()));
    let public = key.public_key().clone();
    let config = server::Config {
        keys: vec![key],
        // The default 1 s rejection delay would slow every failed-login test.
        auth_rejection_time: Duration::from_millis(1),
        ..Default::default()
    };
    Host {
        config: Arc::new(config),
        public,
    }
}

impl MockSshd {
    pub async fn start(options: MockSshdOptions) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let echo = TcpListener::bind(("127.0.0.1", 0)).await?;
        let (addr, echo_addr) = (listener.local_addr()?, echo.local_addr()?);

        let shared = Arc::new(Shared {
            forwarding: options.forwarding,
            authorized: Mutex::new(options.authorized),
            user_cas: Mutex::new(options.user_ca.into_iter().collect()),
            host: Mutex::new(random_host()),
            sessions: watch::channel(0).0,
            live: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(0),
        });
        let tasks = [
            tokio::spawn(accept_loop(listener, Arc::clone(&shared))),
            tokio::spawn(echo_loop(echo)),
        ];
        Ok(Self {
            shared,
            addr,
            echo_addr,
            tasks,
        })
    }

    /// Where the sshd listens.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// A TCP echo server, reachable through `direct-tcpip` channels.
    pub fn echo_addr(&self) -> SocketAddr {
        self.echo_addr
    }

    /// Accepts `key` for public-key login, also for sessions already open.
    pub fn authorize(&self, key: PublicKey) {
        lock(&self.shared.authorized).push(key);
    }

    pub fn authorized_keys(&self) -> Vec<PublicKey> {
        lock(&self.shared.authorized).clone()
    }

    /// Accepts user certificates signed by `ca`.
    pub fn trust_user_ca(&self, ca: PublicKey) {
        lock(&self.shared.user_cas).push(ca);
    }

    /// Sessions that are connecting or open.
    pub fn session_count(&self) -> usize {
        *self.shared.sessions.borrow()
    }

    /// Waits (bounded) until exactly `count` sessions are left.
    pub async fn wait_session_count(&self, count: usize) -> bool {
        let mut sessions = self.shared.sessions.subscribe();
        timeout(SESSION_WAIT, sessions.wait_for(|open| *open == count))
            .await
            .is_ok_and(|reached| reached.is_ok())
    }

    /// Disconnects every open session; the sshd keeps accepting new ones.
    pub async fn drop_sessions(&self) {
        let handles: Vec<server::Handle> = lock(&self.shared.live).values().cloned().collect();
        for handle in handles {
            let _ = handle
                .disconnect(
                    Disconnect::ByApplication,
                    "dropped by MockSshd".into(),
                    "en".into(),
                )
                .await;
        }
    }

    /// New sessions present a new host key; open sessions keep theirs.
    pub fn rotate_host_key(&self) {
        *lock(&self.shared.host) = random_host();
    }

    pub fn host_key(&self) -> PublicKey {
        lock(&self.shared.host).public.clone()
    }

    pub(super) fn shared(&self) -> Arc<Shared> {
        Arc::clone(&self.shared)
    }
}

impl Drop for MockSshd {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

impl Shared {
    /// Runs one SSH session over `stream`.
    pub(super) fn serve<S>(self: &Arc<Self>, stream: S)
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let shared = Arc::clone(self);
        let config = Arc::clone(&lock(&self.host).config);
        self.sessions.send_modify(|open| *open += 1);
        tokio::spawn(async move {
            let _counted = Counted(Arc::clone(&shared));
            let handler = MockHandler {
                shared: Arc::clone(&shared),
            };
            let Ok(session) = server::run_stream(config, stream, handler).await else {
                return;
            };
            let id = shared.next_id.fetch_add(1, Ordering::Relaxed);
            lock(&shared.live).insert(id, session.handle());
            let _ = session.await;
            lock(&shared.live).remove(&id);
        });
    }
}

/// Keeps `session_count` right however the session task ends.
struct Counted(Arc<Shared>);

impl Drop for Counted {
    fn drop(&mut self) {
        self.0
            .sessions
            .send_modify(|open| *open = open.saturating_sub(1));
    }
}

async fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    while let Ok((socket, _)) = listener.accept().await {
        let _ = socket.set_nodelay(true);
        shared.serve(socket);
    }
}

async fn echo_loop(listener: TcpListener) {
    while let Ok((socket, _)) = listener.accept().await {
        tokio::spawn(async move {
            let (mut reader, mut writer) = socket.into_split();
            let _ = copy(&mut reader, &mut writer).await;
        });
    }
}

struct MockHandler {
    shared: Arc<Shared>,
}

impl Handler for MockHandler {
    type Error = russh::Error;

    async fn auth_publickey(
        &mut self, _user: &str, key: VerifiedPublicKey<'_>,
    ) -> Result<Auth, Self::Error> {
        let known = lock(&self.shared.authorized)
            .iter()
            .any(|authorized| authorized.key_data() == key.public_key.key_data());
        Ok(if known { Auth::Accept } else { Auth::reject() })
    }

    async fn auth_openssh_certificate(
        &mut self, user: &str, certificate: &Certificate,
    ) -> Result<Auth, Self::Error> {
        let authorities: Vec<_> = lock(&self.shared.user_cas)
            .iter()
            .map(|ca| ca.fingerprint(HashAlg::Sha256))
            .collect();
        let trusted = certificate.cert_type().is_user()
            && certificate
                .valid_principals()
                .iter()
                .any(|principal| principal == user)
            && certificate.validate(&authorities).is_ok();
        Ok(if trusted {
            Auth::Accept
        } else {
            Auth::reject()
        })
    }

    async fn channel_open_direct_tcpip(
        &mut self, channel: Channel<Msg>, host_to_connect: &str, port_to_connect: u32,
        _originator_address: &str, _originator_port: u32, reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if !self.shared.forwarding {
            reply
                .reject(ChannelOpenFailure::AdministrativelyProhibited)
                .await;
            return Ok(());
        }

        // Off the session loop, so a slow connect never stalls other channels.
        let host = host_to_connect.to_owned();
        tokio::spawn(async move {
            let target = match u16::try_from(port_to_connect) {
                Ok(port) => TcpStream::connect((host.as_str(), port)).await.ok(),
                Err(_) => None,
            };
            let Some(mut target) = target else {
                reply.reject(ChannelOpenFailure::ConnectFailed).await;
                return;
            };
            reply.accept().await;
            let _ = copy_bidirectional(&mut channel.into_stream(), &mut target).await;
        });
        Ok(())
    }
}

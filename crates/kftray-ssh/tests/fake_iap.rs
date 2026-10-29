#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(feature = "iap")]

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{
    AtomicUsize,
    Ordering,
};
use std::time::Duration;

use async_trait::async_trait;
use kftray_ssh::testing::iap::FakeIap;
use kftray_ssh::transport::iap::{
    IapError,
    IapTarget,
    IapTransport,
    TokenSource,
};
use kftray_ssh::{
    BoxStream,
    ProviderError,
    SshError,
    Transport,
};
use secrecy::SecretString;
use tokio::io::{
    AsyncReadExt,
    AsyncWriteExt,
};
use tokio::net::{
    TcpListener,
    TcpStream,
};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Error as WsError;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{
    HeaderName,
    HeaderValue,
};

const TOKEN: &str = "test-token";

struct Tokens {
    token: String,
    calls: AtomicUsize,
}

impl Tokens {
    fn new(token: &str) -> Arc<Self> {
        Arc::new(Self {
            token: token.to_owned(),
            calls: AtomicUsize::new(0),
        })
    }
}

#[async_trait]
impl TokenSource for Tokens {
    async fn access_token(&self) -> Result<SecretString, SshError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(SecretString::from(self.token.clone()))
    }
}

async fn within<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(60), future)
        .await
        .expect("timed out")
}

/// Pseudo-random but reproducible bytes, so a misplaced chunk cannot go unnoticed.
fn pattern(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 24) as u8
        })
        .collect()
}

async fn listen() -> (TcpListener, SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    (listener, addr)
}

async fn echo_upstream() -> SocketAddr {
    let (listener, addr) = listen().await;
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let (mut read, mut write) = stream.into_split();
                let _ = tokio::io::copy(&mut read, &mut write).await;
            });
        }
    });
    addr
}

/// Upstream that writes `data` to its first client and then only listens.
async fn push_upstream(data: Vec<u8>) -> SocketAddr {
    let (listener, addr) = listen().await;
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (mut read, mut write) = stream.into_split();
        write.write_all(&data).await.unwrap();
        let _ = tokio::io::copy(&mut read, &mut tokio::io::sink()).await;
    });
    addr
}

fn transport(fake: &FakeIap, tokens: Arc<Tokens>) -> IapTransport {
    IapTransport::new(
        IapTarget::Instance {
            project: "p".into(),
            zone: "z".into(),
            instance: "vm".into(),
            interface: "nic0".into(),
            port: 22,
        },
        fake.endpoint(),
        tokens,
    )
}

async fn open(fake: &FakeIap) -> BoxStream {
    within(transport(fake, Tokens::new(TOKEN)).connect())
        .await
        .map_err(|e| e.to_string())
        .unwrap()
}

fn expect_iap_error(result: Result<BoxStream, SshError>, expected: IapError) {
    match result {
        Ok(_) => panic!("connect should have failed with: {expected}"),
        Err(error) => {
            assert!(matches!(
                error,
                SshError::Provider {
                    provider: "iap",
                    ..
                }
            ));
            assert_eq!(error.to_string(), expected.user_message());
        }
    }
}

async fn raw_handshake(fake: &FakeIap, headers: &[(&str, &str)]) -> Result<(), WsError> {
    let url = format!(
        "ws://{}/v4/connect?project=p&port=22&newWebsocket=True",
        fake.addr()
    );
    let mut request = url.into_client_request().unwrap();
    for (name, value) in headers {
        request.headers_mut().insert(
            HeaderName::from_bytes(name.as_bytes()).unwrap(),
            HeaderValue::from_str(value).unwrap(),
        );
    }
    let tcp = TcpStream::connect(fake.addr()).await.unwrap();
    tokio_tungstenite::client_async(request, tcp)
        .await
        .map(|_| ())
}

fn status_of(result: Result<(), WsError>) -> u16 {
    match result {
        Err(WsError::Http(response)) => response.status().as_u16(),
        other => panic!("expected an HTTP rejection, got {other:?}"),
    }
}

async fn read_exactly(stream: &mut BoxStream, len: usize) -> Vec<u8> {
    let mut buf = vec![0u8; len];
    within(stream.read_exact(&mut buf)).await.unwrap();
    buf
}

async fn wait_for_acks(fake: &FakeIap) -> Vec<u64> {
    within(async {
        while fake.acks_received().is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    fake.acks_received()
}

#[tokio::test]
async fn connect_should_send_required_headers_when_opening_tunnel() {
    let fake = FakeIap::start(echo_upstream().await, TOKEN).await.unwrap();
    let mut stream = open(&fake).await;

    stream.write_all(b"ping").await.unwrap();
    assert_eq!(read_exactly(&mut stream, 4).await, b"ping");

    let headers = &fake.received_headers()[0];
    let get = |name: &str| headers.get(name).unwrap().to_str().unwrap().to_owned();
    assert_eq!(get("authorization"), format!("Bearer {TOKEN}"));
    assert_eq!(get("origin"), "bot:iap-tunneler");
    assert_eq!(get("sec-websocket-protocol"), "relay.tunnel.cloudproxy.app");
    assert!(get("user-agent").starts_with("kftray/"));
    assert_eq!(
        fake.received_uris()[0],
        "/v4/connect?project=p&port=22&newWebsocket=True&zone=z&instance=vm&interface=nic0"
    );
}

#[tokio::test]
async fn fake_should_reject_handshake_when_required_headers_are_missing() {
    let fake = FakeIap::start(echo_upstream().await, TOKEN).await.unwrap();
    let auth = ("authorization", "Bearer test-token");
    let origin = ("origin", "bot:iap-tunneler");
    let protocol = ("sec-websocket-protocol", "relay.tunnel.cloudproxy.app");

    assert_eq!(status_of(raw_handshake(&fake, &[]).await), 401);
    assert_eq!(status_of(raw_handshake(&fake, &[auth]).await), 400);
    assert_eq!(status_of(raw_handshake(&fake, &[auth, origin]).await), 400);
    raw_handshake(&fake, &[auth, origin, protocol])
        .await
        .unwrap();
}

#[tokio::test]
async fn connect_should_fail_with_permission_denied_when_token_is_wrong() {
    let fake = FakeIap::start(echo_upstream().await, TOKEN).await.unwrap();
    let result = within(transport(&fake, Tokens::new("other")).connect()).await;

    expect_iap_error(result, IapError::PermissionDenied);
    assert_eq!(fake.connect_count(), 1);
}

#[tokio::test]
async fn connect_should_fail_with_permission_denied_when_handshake_is_403() {
    let fake = FakeIap::start(echo_upstream().await, TOKEN).await.unwrap();
    fake.reject_handshake_with(403);
    let result = within(transport(&fake, Tokens::new(TOKEN)).connect()).await;

    expect_iap_error(result, IapError::PermissionDenied);
    assert_eq!(
        fake.connect_count(),
        1,
        "a refused handshake must not be retried"
    );
}

#[tokio::test]
async fn connect_should_fail_with_backend_unreachable_when_iap_closes_with_4003() {
    let fake = FakeIap::start(echo_upstream().await, TOKEN).await.unwrap();
    fake.close_next_with(4003);
    let result = within(transport(&fake, Tokens::new(TOKEN)).connect()).await;

    expect_iap_error(result, IapError::BackendUnreachable);
    assert_eq!(fake.connect_count(), 1, "4003 must not be retried");
}

#[tokio::test]
async fn connect_should_fail_with_backend_unreachable_when_upstream_is_down() {
    let (listener, closed) = listen().await;
    drop(listener);
    let fake = FakeIap::start(closed, TOKEN).await.unwrap();
    let result = within(transport(&fake, Tokens::new(TOKEN)).connect()).await;

    expect_iap_error(result, IapError::BackendUnreachable);
}

#[tokio::test]
async fn tunnel_should_resume_without_loss_when_iap_closes_with_4004() {
    let fake = FakeIap::start(echo_upstream().await, TOKEN).await.unwrap();
    let tokens = Tokens::new(TOKEN);
    let mut stream = within(transport(&fake, tokens.clone()).connect())
        .await
        .unwrap();

    let first = pattern(32 * 1024, 1);
    stream.write_all(&first).await.unwrap();
    assert_eq!(read_exactly(&mut stream, first.len()).await, first);

    fake.close_next_with(4004);
    let second = pattern(32 * 1024, 2);
    stream.write_all(&second).await.unwrap();
    assert_eq!(read_exactly(&mut stream, second.len()).await, second);

    assert_eq!(fake.connect_count(), 2);
    let reconnect = &fake.received_uris()[1];
    assert!(reconnect.starts_with("/v4/reconnect?sid="), "{reconnect}");
    assert!(
        reconnect.ends_with("&ack=32768&newWebsocket=True&zone=z"),
        "{reconnect}"
    );
    assert_eq!(
        tokens.calls.load(Ordering::SeqCst),
        2,
        "a fresh token per connect"
    );
}

// russh flushes without reading: a caller that writes far more than the resend window before
// reading must not stall the tunnel. The echo it gets back sits in front of the ACKs that free
// the window, so inbound frames have to be consumed even while the caller is not reading.
#[tokio::test]
async fn tunnel_should_not_deadlock_when_caller_writes_a_lot_before_reading() {
    let fake = FakeIap::start(echo_upstream().await, TOKEN).await.unwrap();
    let mut stream = open(&fake).await;
    let data = pattern(4 * 1024 * 1024, 7);

    within(stream.write_all(&data)).await.unwrap();

    assert!(read_exactly(&mut stream, data.len()).await == data);
}

#[tokio::test]
async fn tunnel_should_end_without_reconnecting_when_iap_closes_with_4010() {
    let fake = FakeIap::start(echo_upstream().await, TOKEN).await.unwrap();
    let mut stream = open(&fake).await;

    fake.close_next_with(4010);
    let mut rest = Vec::new();
    within(stream.read_to_end(&mut rest)).await.unwrap();

    assert!(rest.is_empty());
    assert_eq!(fake.connect_count(), 1);
}

#[tokio::test]
async fn tunnel_should_close_upstream_when_client_half_is_dropped() {
    let (listener, addr) = listen().await;
    let (eof_tx, eof_rx) = oneshot::channel();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut sink = Vec::new();
        stream.read_to_end(&mut sink).await.unwrap();
        let _ = eof_tx.send(());
    });
    let fake = FakeIap::start(addr, TOKEN).await.unwrap();

    drop(open(&fake).await);

    within(eof_rx).await.unwrap();
}

#[tokio::test]
async fn downstream_should_send_one_ack_above_threshold_when_40_kib_arrive() {
    let fake = FakeIap::start(push_upstream(pattern(40 * 1024, 3)).await, TOKEN)
        .await
        .unwrap();
    let mut stream = open(&fake).await;

    read_exactly(&mut stream, 40 * 1024).await;
    let acks = wait_for_acks(&fake).await;

    assert_eq!(
        acks.len(),
        1,
        "idle flush must not have fired yet: {acks:?}"
    );
    assert!(acks[0] > 32_768 && acks[0] <= 40 * 1024, "{acks:?}");
}

#[tokio::test]
async fn idle_flush_should_ack_when_received_bytes_stay_below_threshold() {
    let fake = FakeIap::start(push_upstream(pattern(1000, 4)).await, TOKEN)
        .await
        .unwrap();
    let mut stream = open(&fake).await;

    read_exactly(&mut stream, 1000).await;
    assert!(fake.acks_received().is_empty());

    assert_eq!(wait_for_acks(&fake).await, [1000]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ten_mib_should_arrive_identical_when_connection_drops_mid_transfer() {
    const SIZE: usize = 10 * 1024 * 1024;
    let up = Arc::new(pattern(SIZE, 5));
    let down = Arc::new(pattern(SIZE, 6));

    let (listener, addr) = listen().await;
    let upstream: JoinHandle<Vec<u8>> = {
        let down = down.clone();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (mut read, mut write) = stream.into_split();
            tokio::spawn(async move {
                write.write_all(&down).await.unwrap();
                write
            });
            let mut received = vec![0u8; SIZE];
            read.read_exact(&mut received).await.unwrap();
            received
        })
    };
    let fake = Arc::new(FakeIap::start(addr, TOKEN).await.unwrap());
    let stream = open(&fake).await;
    let (mut read, mut write) = tokio::io::split(stream);

    let writer = {
        let up = up.clone();
        tokio::spawn(async move {
            write.write_all(&up).await.unwrap();
            write
        })
    };
    let reader = {
        let fake = fake.clone();
        tokio::spawn(async move {
            let mut received = Vec::with_capacity(SIZE);
            let mut chunk = vec![0u8; 64 * 1024];
            let mut dropped = false;
            while received.len() < SIZE {
                let n = read.read(&mut chunk).await.unwrap();
                assert!(n > 0, "stream ended after {} bytes", received.len());
                received.extend_from_slice(&chunk[..n]);
                if !dropped && received.len() >= 3 * 1024 * 1024 {
                    dropped = true;
                    fake.drop_connections();
                }
            }
            received
        })
    };

    let received_by_client = within(reader).await.unwrap();
    let received_by_upstream = within(upstream).await.unwrap();
    let _write_half = within(writer).await.unwrap();

    assert!(received_by_client == *down, "downstream bytes differ");
    assert!(received_by_upstream == *up, "upstream bytes differ");
    assert!(
        fake.connect_count() >= 2,
        "the drop must have caused a resume"
    );
}

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{
    AtomicU64,
    Ordering,
};
use std::time::{
    Duration,
    Instant,
};

use anyhow::Context;
use tokio::io::{
    AsyncRead,
    AsyncReadExt,
    AsyncWrite,
    AsyncWriteExt,
};
use tokio::net::UdpSocket as TokioUdpSocket;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{
    debug,
    error,
    info,
};

const BUFFER_SIZE: usize = 131072;
const MAX_SESSIONS: usize = 128;
const SESSION_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
const SESSION_SWEEP_INTERVAL: Duration = Duration::from_secs(5);
const SESSION_QUEUE_DEPTH: usize = 64;
const REPLY_QUEUE_DEPTH: usize = 256;
const SESSION_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const TUNNEL_RETRY_COOLDOWN: Duration = Duration::from_secs(1);

/// Opens one upstream tunnel per local UDP client.
///
/// The tunnel framing carries only a length prefix, so a single shared tunnel
/// cannot tell two clients apart and would return every reply to whichever
/// client sent last. The relay pairs each tunnel with its own UDP socket, so
/// giving every client its own tunnel keeps replies correlated without adding a
/// client identifier to the wire format.
pub trait UdpUpstream: Send + Sync + 'static {
    type Stream: AsyncRead + AsyncWrite + Unpin + Send + 'static;

    fn connect(&self) -> impl Future<Output = anyhow::Result<Self::Stream>> + Send;

    /// Called when a tunnel cannot be opened.
    fn on_connect_failure(&self, _error: &anyhow::Error) {}

    /// Called when an established tunnel ends without being cancelled. A relay
    /// that accepts a tunnel and immediately closes it is a failure the
    /// connect path never sees.
    fn on_session_failure(&self, _error: &anyhow::Error) {}

    /// Called the first time a tunnel actually moves a datagram. Only real
    /// traffic clears earlier failures; a connect that is accepted and dropped
    /// proves nothing.
    fn on_session_traffic(&self) {}
}

struct UdpSession {
    packets: mpsc::Sender<Vec<u8>>,
    cancellation: CancellationToken,
    task: tokio_util::task::AbortOnDropHandle<()>,
    last_seen: Instant,
    opened_at: Instant,
    /// Milliseconds since `opened_at` of the last byte the tunnel moved in
    /// either direction. A session waiting on slow replies is still working,
    /// even though no new local datagram has arrived.
    tunnel_activity: Arc<AtomicU64>,
}

impl UdpSession {
    /// A session is working while something actually progressed recently: a
    /// local datagram arrived, or the tunnel moved a frame in either direction.
    ///
    /// Queue depth alone is deliberately not progress. A tunnel that stops
    /// draining its writes would otherwise hold its session, and its queued
    /// payloads, forever, and the client could never open a replacement.
    fn is_working(&self, now: Instant) -> bool {
        let activity =
            self.opened_at + Duration::from_millis(self.tunnel_activity.load(Ordering::Relaxed));
        now.duration_since(self.last_seen.max(activity)) <= SESSION_IDLE_TIMEOUT
    }

    /// A tunnel that ended is retried only after a cooldown, so a relay that
    /// refuses connections cannot be hammered once per datagram.
    fn is_cooling_down(&self, now: Instant) -> bool {
        now.duration_since(self.opened_at) < TUNNEL_RETRY_COOLDOWN
    }
}

pub struct UdpForwarder;

impl UdpForwarder {
    pub async fn bind_and_forward<U: UdpUpstream>(
        local_address: String, local_port: u16, upstream: Arc<U>,
        cancellation_token: CancellationToken,
    ) -> anyhow::Result<(
        u16,
        impl Future<Output = anyhow::Result<()>> + Send + 'static,
    )> {
        let local_udp_addr = format!("{local_address}:{local_port}");

        let local_udp_socket = TokioUdpSocket::bind(&local_udp_addr)
            .await
            .context("Failed to bind local UDP socket")?;

        let local_port = local_udp_socket.local_addr()?.port();

        // Sessions open lazily, so probe the tunnel here to keep startup
        // failing fast when the relay is unreachable.
        drop(
            upstream
                .connect()
                .await
                .context("Failed to open the upstream UDP tunnel")?,
        );

        info!("Local UDP socket bound to {}", local_udp_addr);

        let forward_future = async move {
            let mut sessions: HashMap<SocketAddr, UdpSession> = HashMap::new();
            let mut datagram = vec![0u8; BUFFER_SIZE];
            let mut sweep = tokio::time::interval(SESSION_SWEEP_INTERVAL);
            sweep.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            // Replies travel back through this queue instead of a shared socket
            // handle: the socket stays owned by this future, so dropping it
            // releases the port even when session tasks outlive their abort.
            let (replies, mut incoming_replies) =
                mpsc::channel::<(SocketAddr, Vec<u8>)>(REPLY_QUEUE_DEPTH);
            let result: anyhow::Result<()> = loop {
                tokio::select! {
                    biased;
                    _ = cancellation_token.cancelled() => {
                        info!("UDP forwarder cancelled, shutting down");
                        break Ok(());
                    }
                    _ = sweep.tick() => {
                        Self::retire_sessions(&mut sessions);
                    }
                    Some((peer, packet)) = incoming_replies.recv() => {
                        if let Err(e) = local_udp_socket.send_to(&packet, &peer).await {
                            debug!("Failed to send a reply to {}: {:?}", peer, e);
                        }
                    }
                    received = local_udp_socket.recv_from(&mut datagram) => {
                        let (len, peer) = match received {
                            Ok(received) => received,
                            Err(e) => {
                                error!("Failed to receive from UDP socket: {:?}", e);
                                break Err(anyhow::anyhow!(
                                    "Failed to receive from UDP socket: {e}"
                                ));
                            }
                        };
                        Self::dispatch_datagram(
                            &mut sessions,
                            peer,
                            &replies,
                            &upstream,
                            &cancellation_token,
                            &datagram[..len],
                        );
                    }
                }
            };

            let tasks: Vec<_> = sessions
                .drain()
                .map(|(_, session)| {
                    session.cancellation.cancel();
                    session.task
                })
                .collect();
            // Handles abort on drop, so a timeout here still releases the
            // socket rather than leaving the local port bound.
            let drain = async move {
                for task in tasks {
                    let _ = task.await;
                }
            };
            let _ = tokio::time::timeout(SESSION_SHUTDOWN_TIMEOUT, drain).await;

            result
        };

        Ok((local_port, forward_future))
    }

    /// Hands a datagram to the session that owns `peer`, opening one when the
    /// peer is new or its previous tunnel has ended. The tunnel itself is
    /// opened inside the session task, so one slow handshake cannot stall the
    /// datagrams of every other client.
    fn dispatch_datagram<U: UdpUpstream>(
        sessions: &mut HashMap<SocketAddr, UdpSession>, peer: SocketAddr,
        replies: &mpsc::Sender<(SocketAddr, Vec<u8>)>, upstream: &Arc<U>,
        cancellation_token: &CancellationToken, payload: &[u8],
    ) {
        let Some(packets) =
            Self::session_for(sessions, peer, replies, upstream, cancellation_token)
        else {
            debug!("Dropping a datagram from {}: tunnel is cooling down", peer);
            return;
        };
        if packets.try_send(payload.to_vec()).is_err() {
            // Not counted as activity: a datagram the session could not accept
            // must not keep a stalled tunnel alive.
            debug!("Dropping a datagram from {}: tunnel is saturated", peer);
            return;
        }
        if let Some(session) = sessions.get_mut(&peer) {
            session.last_seen = Instant::now();
        }
    }

    fn session_for<U: UdpUpstream>(
        sessions: &mut HashMap<SocketAddr, UdpSession>, peer: SocketAddr,
        replies: &mpsc::Sender<(SocketAddr, Vec<u8>)>, upstream: &Arc<U>,
        cancellation_token: &CancellationToken,
    ) -> Option<mpsc::Sender<Vec<u8>>> {
        let now = Instant::now();
        if let Some(session) = sessions.get_mut(&peer) {
            if !session.packets.is_closed() {
                return Some(session.packets.clone());
            }
            if session.is_cooling_down(now) {
                return None;
            }
            sessions.remove(&peer);
        }

        Self::retire_sessions(sessions);
        while sessions.len() >= MAX_SESSIONS {
            let Some(oldest) = sessions
                .iter()
                .min_by_key(|(_, session)| session.last_seen)
                .map(|(peer, _)| *peer)
            else {
                break;
            };
            debug!(
                "Retiring the least recently used UDP session for {}",
                oldest
            );
            if let Some(session) = sessions.remove(&oldest) {
                session.cancellation.cancel();
            }
        }

        let (packets, mut queue) = mpsc::channel::<Vec<u8>>(SESSION_QUEUE_DEPTH);
        let cancellation = cancellation_token.child_token();
        let session_cancellation = cancellation.clone();
        let replies = replies.clone();
        let upstream = Arc::clone(upstream);

        let tunnel_activity = Arc::new(AtomicU64::new(0));
        let session_activity = Arc::clone(&tunnel_activity);

        let task = tokio::spawn(async move {
            let stream = tokio::select! {
                biased;
                _ = session_cancellation.cancelled() => return,
                connected = upstream.connect() => match connected {
                    Ok(stream) => stream,
                    Err(error) => {
                        upstream.on_connect_failure(&error);
                        return;
                    }
                },
            };

            let traffic_seen = std::sync::atomic::AtomicBool::new(false);
            let mark_activity = || {
                session_activity.store(
                    u64::try_from(now.elapsed().as_millis()).unwrap_or(u64::MAX),
                    Ordering::Relaxed,
                );
            };
            // Only a frame coming back from the relay proves the tunnel works.
            // A write that lands in transport buffers does not, so it must not
            // clear the failures that drive recovery.
            let mark_relay_response = || {
                mark_activity();
                if !traffic_seen.swap(true, Ordering::Relaxed) {
                    upstream.on_session_traffic();
                }
            };
            let (mut reader, mut writer) = tokio::io::split(stream);
            {
                let uplink = async {
                    while let Some(packet) = queue.recv().await {
                        let len = u32::try_from(packet.len())
                            .context("UDP datagram is larger than the tunnel framing allows")?;
                        writer.write_all(&len.to_be_bytes()).await?;
                        writer.write_all(&packet).await?;
                        writer.flush().await?;
                        mark_activity();
                    }
                    Ok::<(), anyhow::Error>(())
                };
                let downlink = async {
                    while let Some(packet) = Self::read_tcp_length_and_packet(&mut reader).await? {
                        // Counted before the empty check: the relay emits a
                        // zero-length frame when the target does not answer
                        // within its own timeout, and that is still progress.
                        mark_relay_response();
                        if packet.is_empty() {
                            continue;
                        }
                        if replies.try_send((peer, packet)).is_err() {
                            debug!("Dropping a reply for {}: the listener is saturated", peer);
                        }
                    }
                    Err(anyhow::anyhow!("the relay closed the tunnel"))
                };

                let result = tokio::select! {
                    biased;
                    _ = session_cancellation.cancelled() => Ok(()),
                    result = uplink => result,
                    result = downlink => result,
                };
                if let Err(error) = result {
                    debug!("UDP tunnel for {} ended: {:?}", peer, error);
                    // Cancellation is idle retirement or shutdown, not a fault.
                    if !session_cancellation.is_cancelled() {
                        upstream.on_session_failure(&error);
                    }
                }
            }
            let _ = writer.shutdown().await;
        });

        let queue = packets.clone();
        sessions.insert(
            peer,
            UdpSession {
                packets,
                cancellation,
                task: tokio_util::task::AbortOnDropHandle::new(task),
                last_seen: now,
                opened_at: now,
                tunnel_activity,
            },
        );
        Some(queue)
    }

    fn retire_sessions(sessions: &mut HashMap<SocketAddr, UdpSession>) {
        let now = Instant::now();
        sessions.retain(|peer, session| {
            if session.packets.is_closed() {
                // Kept until the cooldown expires: dropping the record now
                // would let the next datagram reconnect immediately.
                return session.is_cooling_down(now);
            }
            if session.is_working(now) {
                return true;
            }
            debug!("Retiring the idle UDP session for {}", peer);
            session.cancellation.cancel();
            false
        });
    }

    async fn read_tcp_length_and_packet(
        tcp_read: &mut (impl AsyncReadExt + Unpin),
    ) -> anyhow::Result<Option<Vec<u8>>> {
        let mut len_bytes = [0u8; 4];

        match tcp_read.read_exact(&mut len_bytes).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e.into()),
        }

        let len = u32::from_be_bytes(len_bytes) as usize;
        if len > BUFFER_SIZE {
            return Err(anyhow::anyhow!(
                "Upstream announced a {len} byte datagram, above the {BUFFER_SIZE} byte limit"
            ));
        }
        let mut packet = vec![0u8; len];

        match tcp_read.read_exact(&mut packet).await {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e.into()),
        }

        Ok(Some(packet))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use tokio::io::{
        DuplexStream,
        duplex,
    };
    use tokio::net::UdpSocket;

    use super::*;

    /// Hands every caller its own duplex pair and publishes the far end so a
    /// test can drive each tunnel independently.
    pub(crate) struct SpawningUpstream {
        opened: mpsc::UnboundedSender<DuplexStream>,
        capacity: usize,
    }

    impl SpawningUpstream {
        pub(crate) fn new(capacity: usize) -> (Arc<Self>, mpsc::UnboundedReceiver<DuplexStream>) {
            let (opened, receiver) = mpsc::unbounded_channel();
            (Arc::new(Self { opened, capacity }), receiver)
        }
    }

    impl UdpUpstream for SpawningUpstream {
        type Stream = DuplexStream;

        async fn connect(&self) -> anyhow::Result<Self::Stream> {
            let (near, far) = duplex(self.capacity);
            self.opened
                .send(far)
                .map_err(|_| anyhow::anyhow!("upstream receiver dropped"))?;
            Ok(near)
        }
    }

    struct UnreachableUpstream;

    impl UdpUpstream for UnreachableUpstream {
        type Stream = DuplexStream;

        async fn connect(&self) -> anyhow::Result<Self::Stream> {
            Err(anyhow::anyhow!("no relay pod"))
        }
    }

    /// Accepts every tunnel and immediately closes it, the shape a relay takes
    /// when its pod is being replaced.
    #[derive(Default)]
    struct ClosingUpstream {
        failures: Arc<std::sync::atomic::AtomicUsize>,
        traffic: Arc<std::sync::atomic::AtomicUsize>,
    }

    /// Consumes one frame and then closes, the shape a relay takes when the
    /// destination cannot be resolved: the write succeeds into transport
    /// buffers before the tunnel dies.
    #[derive(Default)]
    struct ConsumeThenCloseUpstream {
        failures: Arc<std::sync::atomic::AtomicUsize>,
        traffic: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl UdpUpstream for ConsumeThenCloseUpstream {
        type Stream = DuplexStream;

        async fn connect(&self) -> anyhow::Result<Self::Stream> {
            let (ours, mut theirs) = duplex(4096);
            tokio::spawn(async move {
                let mut frame = [0u8; 64];
                let _ = theirs.read(&mut frame).await;
                drop(theirs);
            });
            Ok(ours)
        }

        fn on_session_failure(&self, _error: &anyhow::Error) {
            self.failures
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

        fn on_session_traffic(&self) {
            self.traffic
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    impl UdpUpstream for ClosingUpstream {
        type Stream = DuplexStream;

        async fn connect(&self) -> anyhow::Result<Self::Stream> {
            let (ours, theirs) = duplex(64);
            drop(theirs);
            Ok(ours)
        }

        fn on_session_failure(&self, _error: &anyhow::Error) {
            self.failures
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

        fn on_session_traffic(&self) {
            self.traffic
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Serves `ready` connections normally, then never resolves again.
    struct StallingUpstream {
        inner: Arc<SpawningUpstream>,
        remaining: std::sync::atomic::AtomicUsize,
    }

    impl StallingUpstream {
        fn new(
            ready: usize, capacity: usize,
        ) -> (Arc<Self>, mpsc::UnboundedReceiver<DuplexStream>) {
            let (inner, opened) = SpawningUpstream::new(capacity);
            (
                Arc::new(Self {
                    inner,
                    remaining: std::sync::atomic::AtomicUsize::new(ready),
                }),
                opened,
            )
        }
    }

    impl UdpUpstream for StallingUpstream {
        type Stream = DuplexStream;

        async fn connect(&self) -> anyhow::Result<Self::Stream> {
            if self
                .remaining
                .fetch_update(
                    std::sync::atomic::Ordering::SeqCst,
                    std::sync::atomic::Ordering::SeqCst,
                    |remaining| remaining.checked_sub(1),
                )
                .is_err()
            {
                std::future::pending::<()>().await;
            }
            self.inner.connect().await
        }
    }

    async fn read_frame(stream: &mut DuplexStream) -> Vec<u8> {
        UdpForwarder::read_tcp_length_and_packet(stream)
            .await
            .unwrap()
            .unwrap()
    }

    async fn write_frame(stream: &mut DuplexStream, payload: &[u8]) {
        stream
            .write_all(&(payload.len() as u32).to_be_bytes())
            .await
            .unwrap();
        stream.write_all(payload).await.unwrap();
        stream.flush().await.unwrap();
    }

    fn closed_session(opened_at: Instant) -> UdpSession {
        let (packets, queue) = mpsc::channel::<Vec<u8>>(SESSION_QUEUE_DEPTH);
        drop(queue);
        UdpSession {
            packets,
            cancellation: CancellationToken::new(),
            task: tokio_util::task::AbortOnDropHandle::new(tokio::spawn(std::future::pending())),
            last_seen: opened_at,
            opened_at,
            tunnel_activity: Arc::new(AtomicU64::new(0)),
        }
    }

    #[tokio::test]
    async fn retirement_keeps_failed_sessions_until_their_cooldown_expires() {
        let peer: SocketAddr = "127.0.0.1:41007".parse().unwrap();
        let other: SocketAddr = "127.0.0.1:41008".parse().unwrap();
        let mut sessions = HashMap::new();
        sessions.insert(peer, closed_session(Instant::now()));
        sessions.insert(
            other,
            closed_session(Instant::now() - TUNNEL_RETRY_COOLDOWN * 2),
        );

        // A retirement pass runs whenever any peer opens a session and on every
        // sweep tick; it must not hand another peer a free reconnect.
        UdpForwarder::retire_sessions(&mut sessions);

        assert!(
            sessions.contains_key(&peer),
            "a tunnel that just failed must keep cooling down"
        );
        assert!(
            !sessions.contains_key(&other),
            "a tunnel past its cooldown is retried on the next datagram"
        );
    }

    #[tokio::test]
    async fn a_session_is_retired_on_progress_not_on_queue_depth() {
        let (packets, _queue) = mpsc::channel::<Vec<u8>>(SESSION_QUEUE_DEPTH);
        let idle_for_ages = Instant::now() - SESSION_IDLE_TIMEOUT * 2;
        let session = UdpSession {
            packets,
            cancellation: CancellationToken::new(),
            task: tokio_util::task::AbortOnDropHandle::new(tokio::spawn(std::future::pending())),
            last_seen: idle_for_ages,
            opened_at: idle_for_ages,
            tunnel_activity: Arc::new(AtomicU64::new(0)),
        };
        let now = Instant::now();

        assert!(
            !session.is_working(now),
            "no local traffic and no tunnel activity means idle"
        );

        // A tunnel that stopped draining its writes keeps its queue full. That
        // must not exempt it, or the client can never open a replacement.
        for _ in 0..SESSION_QUEUE_DEPTH {
            session.packets.try_send(b"queued".to_vec()).unwrap();
        }
        assert!(
            !session.is_working(now),
            "a stalled tunnel must be retired even with work still queued"
        );

        session.tunnel_activity.store(
            u64::try_from(now.duration_since(idle_for_ages).as_millis()).unwrap(),
            Ordering::Relaxed,
        );
        assert!(
            session.is_working(now),
            "a frame that just came back through the tunnel is progress"
        );
    }

    #[tokio::test]
    async fn a_relay_that_closes_every_tunnel_is_reported_as_a_failure() {
        let upstream = Arc::new(ClosingUpstream::default());
        let failures = Arc::clone(&upstream.failures);
        let traffic = Arc::clone(&upstream.traffic);
        let cancellation_token = CancellationToken::new();
        let (port, forward) = UdpForwarder::bind_and_forward(
            "127.0.0.1".to_owned(),
            0,
            Arc::clone(&upstream),
            cancellation_token.clone(),
        )
        .await
        .unwrap();
        let owner = tokio::spawn(forward);

        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        client.connect(("127.0.0.1", port)).await.unwrap();
        client.send(b"ping").await.unwrap();

        for _ in 0..50 {
            if failures.load(std::sync::atomic::Ordering::Relaxed) > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        assert!(
            failures.load(std::sync::atomic::Ordering::Relaxed) > 0,
            "a tunnel that is accepted and immediately closed must reach recovery"
        );
        assert_eq!(
            traffic.load(std::sync::atomic::Ordering::Relaxed),
            0,
            "a connect that never moved a datagram must not clear earlier failures"
        );

        cancellation_token.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(5), owner).await;
    }

    #[tokio::test]
    async fn an_uplink_write_alone_does_not_clear_recovery_failures() {
        let upstream = Arc::new(ConsumeThenCloseUpstream::default());
        let failures = Arc::clone(&upstream.failures);
        let traffic = Arc::clone(&upstream.traffic);
        let cancellation_token = CancellationToken::new();
        let (port, forward) = UdpForwarder::bind_and_forward(
            "127.0.0.1".to_owned(),
            0,
            Arc::clone(&upstream),
            cancellation_token.clone(),
        )
        .await
        .unwrap();
        let owner = tokio::spawn(forward);

        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        client.connect(("127.0.0.1", port)).await.unwrap();
        client.send(b"ping").await.unwrap();

        for _ in 0..50 {
            if failures.load(std::sync::atomic::Ordering::Relaxed) > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        assert!(
            failures.load(std::sync::atomic::Ordering::Relaxed) > 0,
            "a tunnel that accepts a frame and then dies must reach recovery"
        );
        assert_eq!(
            traffic.load(std::sync::atomic::Ordering::Relaxed),
            0,
            "a write that only reached transport buffers is not a relay response"
        );

        cancellation_token.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(5), owner).await;
    }

    #[tokio::test]
    async fn test_read_tcp_length_and_packet() {
        let (mut reader, mut writer) = duplex(1024);

        let packet = b"hello world";
        write_frame(&mut writer, packet).await;

        let data = UdpForwarder::read_tcp_length_and_packet(&mut reader)
            .await
            .unwrap();
        assert_eq!(data.unwrap(), packet);
    }

    #[tokio::test]
    async fn test_read_tcp_length_and_packet_empty() {
        let (mut reader, _) = duplex(0);

        let result = UdpForwarder::read_tcp_length_and_packet(&mut reader).await;
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn large_packet_survives_fragmented_tcp_reads() {
        let (mut reader, mut writer) = duplex(257);
        let payload: Vec<_> = (0..60 * 1024).map(|index| (index % 251) as u8).collect();
        let header = (payload.len() as u32).to_be_bytes();
        let send = async {
            writer.write_all(&header[..2]).await.unwrap();
            tokio::task::yield_now().await;
            writer.write_all(&header[2..]).await.unwrap();
            for chunk in payload.chunks(509) {
                writer.write_all(chunk).await.unwrap();
            }
        };
        let (_, received) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(send, UdpForwarder::read_tcp_length_and_packet(&mut reader))
        })
        .await
        .unwrap();
        assert_eq!(received.unwrap().unwrap(), payload);
    }

    #[tokio::test]
    async fn test_read_tcp_length_and_packet_partial() {
        let (mut reader, mut writer) = duplex(1024);

        let packet = b"hello world";
        writer
            .write_all(&(packet.len() as u32).to_be_bytes())
            .await
            .unwrap();
        writer.write_all(&packet[0..5]).await.unwrap();
        writer.flush().await.unwrap();

        let read_task =
            tokio::spawn(
                async move { UdpForwarder::read_tcp_length_and_packet(&mut reader).await },
            );

        drop(writer);

        let inner = tokio::time::timeout(Duration::from_secs(1), read_task)
            .await
            .expect("Test timed out")
            .expect("Task should not panic");
        assert!(
            inner.unwrap().is_none(),
            "Partial read should return Ok(None)"
        );
    }

    #[tokio::test]
    async fn oversized_length_prefix_is_rejected() {
        let (mut reader, mut writer) = duplex(64);

        writer.write_all(&u32::MAX.to_be_bytes()).await.unwrap();
        writer.flush().await.unwrap();

        let error = UdpForwarder::read_tcp_length_and_packet(&mut reader)
            .await
            .expect_err("an oversized length prefix must not allocate");
        assert!(error.to_string().contains("above the"), "{error}");
    }

    #[tokio::test]
    async fn startup_fails_when_no_tunnel_can_be_opened() {
        let started = UdpForwarder::bind_and_forward(
            "127.0.0.1".to_owned(),
            0,
            Arc::new(UnreachableUpstream),
            CancellationToken::new(),
        )
        .await;
        let error = match started {
            Ok(_) => panic!("an unreachable relay must fail the listener startup"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("upstream UDP tunnel"), "{error}");
    }

    #[tokio::test]
    async fn test_bind_and_forward_basic() {
        let (upstream, mut opened) = SpawningUpstream::new(64 * 1024);
        let cancellation_token = CancellationToken::new();
        let (port, forward) = UdpForwarder::bind_and_forward(
            "127.0.0.1".to_owned(),
            0,
            upstream,
            cancellation_token.clone(),
        )
        .await
        .unwrap();
        let owner = tokio::spawn(forward);
        let _probe = opened.recv().await.unwrap();

        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        socket2::SockRef::from(&client)
            .set_send_buffer_size(BUFFER_SIZE)
            .unwrap();
        client.connect(("127.0.0.1", port)).await.unwrap();
        let request: Vec<_> = (0..60 * 1024).map(|index| (index % 251) as u8).collect();
        client.send(&request).await.unwrap();

        tokio::time::timeout(Duration::from_secs(5), async {
            let mut tunnel = opened.recv().await.unwrap();
            assert_eq!(read_frame(&mut tunnel).await, request);
            write_frame(&mut tunnel, b"response").await;
            let mut response = [0; 8];
            let received = client.recv(&mut response).await.unwrap();
            assert_eq!(&response[..received], b"response");
        })
        .await
        .unwrap();

        cancellation_token.cancel();
        tokio::time::timeout(Duration::from_secs(5), owner)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        UdpSocket::bind(("127.0.0.1", port)).await.unwrap();
    }

    #[tokio::test]
    async fn aborting_the_owner_releases_the_port_with_a_blocked_session() {
        // A one-byte tunnel plus a reader that never drains it wedges the
        // session in the middle of a write.
        let (upstream, mut opened) = SpawningUpstream::new(1);
        let cancellation_token = CancellationToken::new();
        let (port, forward) = UdpForwarder::bind_and_forward(
            "127.0.0.1".to_owned(),
            0,
            upstream,
            cancellation_token.clone(),
        )
        .await
        .unwrap();
        let owner = tokio::spawn(forward);
        let _probe = opened.recv().await.unwrap();

        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        client.connect(("127.0.0.1", port)).await.unwrap();
        for _ in 0..8 {
            client.send(&[7u8; 512]).await.unwrap();
        }
        let stuck = tokio::time::timeout(Duration::from_secs(5), opened.recv())
            .await
            .unwrap()
            .unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        owner.abort();
        assert!(owner.await.unwrap_err().is_cancelled());
        drop(stuck);

        UdpSocket::bind(("127.0.0.1", port))
            .await
            .expect("aborting the owner must release the port even with a wedged session");
    }

    #[tokio::test]
    async fn a_stalled_tunnel_handshake_does_not_block_other_clients() {
        let (upstream, mut opened) = StallingUpstream::new(2, 4096);
        let cancellation_token = CancellationToken::new();
        let (port, forward) = UdpForwarder::bind_and_forward(
            "127.0.0.1".to_owned(),
            0,
            upstream,
            cancellation_token.clone(),
        )
        .await
        .unwrap();
        let owner = tokio::spawn(forward);
        let _probe = opened.recv().await.unwrap();

        let established = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        established.connect(("127.0.0.1", port)).await.unwrap();
        let stalling = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        stalling.connect(("127.0.0.1", port)).await.unwrap();

        tokio::time::timeout(Duration::from_secs(5), async {
            established.send(b"first").await.unwrap();
            let mut tunnel = opened.recv().await.unwrap();
            assert_eq!(read_frame(&mut tunnel).await, b"first");

            // This client's tunnel never opens.
            stalling.send(b"never-connects").await.unwrap();
            tokio::task::yield_now().await;

            established.send(b"second").await.unwrap();
            assert_eq!(read_frame(&mut tunnel).await, b"second");

            write_frame(&mut tunnel, b"reply").await;
            let mut buffer = [0u8; 32];
            let received = established.recv(&mut buffer).await.unwrap();
            assert_eq!(&buffer[..received], b"reply");
        })
        .await
        .expect("a stalled handshake must not stall an established client");

        cancellation_token.cancel();
        tokio::time::timeout(Duration::from_secs(5), owner)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn each_client_keeps_its_own_tunnel_and_replies() {
        let (upstream, mut opened) = SpawningUpstream::new(4096);
        let cancellation_token = CancellationToken::new();
        let (port, forward) = UdpForwarder::bind_and_forward(
            "127.0.0.1".to_owned(),
            0,
            upstream,
            cancellation_token.clone(),
        )
        .await
        .unwrap();
        let owner = tokio::spawn(forward);
        let _probe = opened.recv().await.unwrap();

        let first = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        first.connect(("127.0.0.1", port)).await.unwrap();
        let second = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        second.connect(("127.0.0.1", port)).await.unwrap();

        tokio::time::timeout(Duration::from_secs(5), async {
            first.send(b"first-request").await.unwrap();
            let mut first_tunnel = opened.recv().await.unwrap();
            assert_eq!(read_frame(&mut first_tunnel).await, b"first-request");

            second.send(b"second-request").await.unwrap();
            let mut second_tunnel = opened.recv().await.unwrap();
            assert_eq!(read_frame(&mut second_tunnel).await, b"second-request");

            // Answer the first client only after the second one has taken over
            // as the most recent sender.
            write_frame(&mut first_tunnel, b"first-reply").await;
            write_frame(&mut second_tunnel, b"second-reply").await;

            let mut buffer = [0u8; 32];
            let received = first.recv(&mut buffer).await.unwrap();
            assert_eq!(&buffer[..received], b"first-reply");
            let received = second.recv(&mut buffer).await.unwrap();
            assert_eq!(&buffer[..received], b"second-reply");
        })
        .await
        .unwrap();

        cancellation_token.cancel();
        tokio::time::timeout(Duration::from_secs(5), owner)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn one_client_reuses_a_single_tunnel() {
        let (upstream, mut opened) = SpawningUpstream::new(4096);
        let cancellation_token = CancellationToken::new();
        let (port, forward) = UdpForwarder::bind_and_forward(
            "127.0.0.1".to_owned(),
            0,
            upstream,
            cancellation_token.clone(),
        )
        .await
        .unwrap();
        let owner = tokio::spawn(forward);
        let _probe = opened.recv().await.unwrap();

        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        client.connect(("127.0.0.1", port)).await.unwrap();

        tokio::time::timeout(Duration::from_secs(5), async {
            client.send(b"one").await.unwrap();
            let mut tunnel = opened.recv().await.unwrap();
            assert_eq!(read_frame(&mut tunnel).await, b"one");

            client.send(b"two").await.unwrap();
            assert_eq!(read_frame(&mut tunnel).await, b"two");

            // A zero-length frame is the relay's idle timeout marker and must
            // not reach the client as an empty datagram.
            write_frame(&mut tunnel, b"").await;
            write_frame(&mut tunnel, b"reply").await;
            let mut buffer = [0u8; 32];
            let received = client.recv(&mut buffer).await.unwrap();
            assert_eq!(&buffer[..received], b"reply");
        })
        .await
        .unwrap();

        assert!(
            opened.try_recv().is_err(),
            "one client must open one tunnel"
        );

        cancellation_token.cancel();
        tokio::time::timeout(Duration::from_secs(5), owner)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn upstream_frame_survives_concurrent_uplink_traffic() {
        let (upstream, mut opened) = SpawningUpstream::new(4096);
        let cancellation_token = CancellationToken::new();
        let (port, forward) = UdpForwarder::bind_and_forward(
            "127.0.0.1".to_owned(),
            0,
            upstream,
            cancellation_token.clone(),
        )
        .await
        .unwrap();
        let owner = tokio::spawn(forward);
        let _probe = opened.recv().await.unwrap();

        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        client.connect(("127.0.0.1", port)).await.unwrap();

        client.send(b"hello").await.unwrap();
        let mut tunnel = opened.recv().await.unwrap();
        assert_eq!(read_frame(&mut tunnel).await, b"hello");

        let response_header = 8u32.to_be_bytes();
        tunnel.write_all(&response_header[..2]).await.unwrap();
        tunnel.flush().await.unwrap();
        tokio::task::yield_now().await;

        client.send(b"second").await.unwrap();
        assert_eq!(read_frame(&mut tunnel).await, b"second");

        tunnel.write_all(&response_header[2..]).await.unwrap();
        tunnel.write_all(b"response").await.unwrap();
        tunnel.flush().await.unwrap();

        let mut response = [0u8; 8];
        let received = tokio::time::timeout(Duration::from_secs(5), client.recv(&mut response))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&response[..received], b"response");

        cancellation_token.cancel();
        tokio::time::timeout(Duration::from_secs(5), owner)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}

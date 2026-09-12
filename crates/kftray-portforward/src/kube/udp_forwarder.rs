use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
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
}

struct UdpSession {
    packets: mpsc::Sender<Vec<u8>>,
    cancellation: CancellationToken,
    last_seen: Instant,
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

        let local_udp_socket = Arc::new(
            TokioUdpSocket::bind(&local_udp_addr)
                .await
                .context("Failed to bind local UDP socket")?,
        );

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
                        let packets = match Self::session_for(
                            &mut sessions,
                            peer,
                            &local_udp_socket,
                            &upstream,
                            &cancellation_token,
                        )
                        .await
                        {
                            Ok(packets) => packets,
                            Err(e) => {
                                error!("No UDP tunnel available for {}: {:?}", peer, e);
                                continue;
                            }
                        };
                        if packets.try_send(datagram[..len].to_vec()).is_err() {
                            debug!("Dropping a datagram from {}: tunnel is saturated", peer);
                        }
                    }
                }
            };

            for (_, session) in sessions.drain() {
                session.cancellation.cancel();
            }

            result
        };

        Ok((local_port, forward_future))
    }

    /// Returns the queue of the session that owns `peer`, opening one when the
    /// peer is new or its previous tunnel has ended.
    async fn session_for<U: UdpUpstream>(
        sessions: &mut HashMap<SocketAddr, UdpSession>, peer: SocketAddr,
        socket: &Arc<TokioUdpSocket>, upstream: &Arc<U>, cancellation_token: &CancellationToken,
    ) -> anyhow::Result<mpsc::Sender<Vec<u8>>> {
        if let Some(session) = sessions.get_mut(&peer) {
            if !session.packets.is_closed() {
                session.last_seen = Instant::now();
                return Ok(session.packets.clone());
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

        let stream = match upstream.connect().await {
            Ok(stream) => stream,
            Err(error) => {
                upstream.on_connect_failure(&error);
                return Err(error);
            }
        };

        let (packets, mut queue) = mpsc::channel::<Vec<u8>>(SESSION_QUEUE_DEPTH);
        let cancellation = cancellation_token.child_token();
        let session_cancellation = cancellation.clone();
        let socket = Arc::clone(socket);

        tokio::spawn(async move {
            let (mut reader, mut writer) = tokio::io::split(stream);
            {
                let uplink = async {
                    while let Some(packet) = queue.recv().await {
                        let len = u32::try_from(packet.len())
                            .context("UDP datagram is larger than the tunnel framing allows")?;
                        writer.write_all(&len.to_be_bytes()).await?;
                        writer.write_all(&packet).await?;
                        writer.flush().await?;
                    }
                    Ok::<(), anyhow::Error>(())
                };
                let downlink = async {
                    while let Some(packet) = Self::read_tcp_length_and_packet(&mut reader).await? {
                        // The relay emits a zero-length frame when the target
                        // does not answer within its own timeout.
                        if packet.is_empty() {
                            continue;
                        }
                        socket.send_to(&packet, &peer).await?;
                    }
                    Ok::<(), anyhow::Error>(())
                };

                let result = tokio::select! {
                    biased;
                    _ = session_cancellation.cancelled() => Ok(()),
                    result = uplink => result,
                    result = downlink => result,
                };
                if let Err(error) = result {
                    debug!("UDP tunnel for {} ended: {:?}", peer, error);
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
                last_seen: Instant::now(),
            },
        );
        Ok(queue)
    }

    fn retire_sessions(sessions: &mut HashMap<SocketAddr, UdpSession>) {
        let now = Instant::now();
        sessions.retain(|peer, session| {
            if session.packets.is_closed() {
                debug!("UDP tunnel for {} has ended", peer);
                return false;
            }
            if now.duration_since(session.last_seen) > SESSION_IDLE_TIMEOUT {
                debug!("Retiring the idle UDP session for {}", peer);
                session.cancellation.cancel();
                return false;
            }
            true
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

use std::collections::VecDeque;
use std::io::{
    self,
    SeekFrom,
};
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{
    Arc,
    Mutex,
    MutexGuard,
    PoisonError,
};
use std::task::{
    Context,
    Poll,
    Waker,
};

use async_trait::async_trait;
use bytes::Bytes;
use russh::client::{
    Handle,
    Msg,
};
use russh::{
    Channel,
    ChannelMsg,
    ChannelOpenFailure,
    ChannelReadHalf,
    ChannelWriteHalf,
};
use tokio::fs::File;
use tokio::io::{
    AsyncRead,
    AsyncReadExt,
    AsyncSeekExt,
    AsyncWrite,
    AsyncWriteExt,
    ReadBuf,
};
use tokio::sync::{
    Notify,
    OwnedSemaphorePermit,
    oneshot,
};
use tokio::time::{
    Instant,
    timeout_at,
};
use tokio_util::sync::{
    CancellationToken,
    DropGuard,
};

use super::connect::protocol_error;
use super::handler::ClientHandler;
use super::pool::SessionEntry;
use super::{
    SessionState,
    SshLease,
    Stage,
};
use crate::consts::{
    CHANNEL_OPEN_TIMEOUT,
    CHANNEL_READ_BUFFER,
};
use crate::error::SshError;
use crate::route::{
    BoxStream,
    StreamOpener,
};

/// Opens one `direct-tcpip` channel per local connection on a leased session.
pub struct SshChannelOpener {
    lease: SshLease,
    host: String,
    port: u16,
}

impl SshChannelOpener {
    pub(super) fn new(lease: SshLease, host: String, port: u16) -> Self {
        Self { lease, host, port }
    }
}

#[async_trait]
impl StreamOpener for SshChannelOpener {
    async fn open(&self, peer: SocketAddr) -> Result<BoxStream, SshError> {
        let entry = &self.lease.0;
        let label = entry.label.as_str();

        // Released with the channel; waits while the session is at its channel cap.
        let permit = Arc::clone(&entry.channels)
            .acquire_owned()
            .await
            .map_err(|_| session_closed(label))?;

        let deadline = Instant::now() + CHANNEL_OPEN_TIMEOUT;
        let handle = timeout_at(deadline, ready_handle(entry))
            .await
            .map_err(|_| SshError::Timeout {
                route: label.to_owned(),
                stage: Stage::Channel,
            })??;
        let stream = open_direct_tcpip(
            &handle,
            label,
            &self.host,
            self.port,
            peer,
            deadline,
            Some(permit),
        )
        .await?;

        log::debug!(
            "[ssh {label}] channel to {}:{} for {peer}",
            self.host,
            self.port
        );
        Ok(Box::new(stream))
    }
}

fn session_closed(label: &str) -> SshError {
    SshError::Transport {
        route: label.to_owned(),
        reason: "the session is closed".to_owned(),
    }
}

/// Waits out a reconnect, then returns the handle of the live session.
async fn ready_handle(entry: &SessionEntry) -> Result<Arc<Handle<ClientHandler>>, SshError> {
    let mut state = entry.state.subscribe();
    let failure = {
        let reached = state
            .wait_for(|s| matches!(s, SessionState::Ready | SessionState::Failed(_)))
            .await
            .map_err(|_| session_closed(&entry.label))?;
        match &*reached {
            SessionState::Failed(reason) => Some(reason.clone()),
            _ => None,
        }
    };
    if let Some(reason) = failure {
        return Err(SshError::Transport {
            route: entry.label.clone(),
            reason,
        });
    }
    Ok(Arc::clone(&*entry.handle.read().await))
}

/// Opens the channel and maps the sshd's refusals; only this connection
/// fails, the session stays.
pub(crate) async fn open_direct_tcpip(
    handle: &Arc<Handle<ClientHandler>>, label: &str, host: &str, port: u16, peer: SocketAddr,
    deadline: Instant, permit: Option<OwnedSemaphorePermit>,
) -> Result<SshStream, SshError> {
    let unreachable = || SshError::TargetUnreachable {
        route: label.to_owned(),
        host: host.to_owned(),
        port,
    };
    let (opened_tx, opened_rx) = oneshot::channel();
    let handle = Arc::clone(handle);
    let host = host.to_owned();
    tokio::spawn(async move {
        let opened = handle
            .channel_open_direct_tcpip(
                host,
                u32::from(port),
                peer.ip().to_string(),
                u32::from(peer.port()),
            )
            .await
            .map(|channel| SshStream::start(channel, permit));
        let _ = opened_tx.send(opened);
    });
    match timeout_at(deadline, opened_rx).await {
        Ok(Ok(Ok(stream))) => Ok(stream),
        Ok(Ok(Err(russh::Error::ChannelOpenFailure(
            ChannelOpenFailure::AdministrativelyProhibited,
        )))) => Err(SshError::ForwardingProhibited {
            route: label.to_owned(),
        }),
        Ok(Ok(Err(russh::Error::ChannelOpenFailure(ChannelOpenFailure::ConnectFailed))))
        | Err(_) => Err(unreachable()),
        Ok(Ok(Err(e))) => Err(protocol_error(label, e)),
        Ok(Err(_)) => Err(session_closed(label)),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum End {
    Eof,
    Reset,
}

#[derive(Default)]
struct Inbound {
    chunks: VecDeque<Bytes>,
    buffered: usize,
    end: Option<End>,
    waker: Option<Waker>,
}

impl Inbound {
    fn room(&self) -> usize {
        CHANNEL_READ_BUFFER - self.buffered
    }

    fn push(&mut self, data: Bytes) {
        if !data.is_empty() {
            self.buffered += data.len();
            self.chunks.push_back(data);
        }
        self.wake();
    }

    fn end(&mut self, end: End) {
        self.end.get_or_insert(end);
        self.wake();
    }

    fn wake(&mut self) {
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

#[derive(Default)]
struct Shared {
    inbound: Mutex<Inbound>,
    room_freed: Notify,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inbound> {
        self.inbound.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

struct Spill {
    file: File,
    read_at: u64,
    written: u64,
}

impl Spill {
    async fn create() -> io::Result<Self> {
        let file = tokio::task::spawn_blocking(tempfile::tempfile)
            .await
            .map_err(io::Error::other)??;
        Ok(Self {
            file: File::from_std(file),
            read_at: 0,
            written: 0,
        })
    }

    fn pending(&self) -> u64 {
        self.written - self.read_at
    }

    async fn append(&mut self, data: &[u8]) -> io::Result<()> {
        self.file.seek(SeekFrom::Start(self.written)).await?;
        self.file.write_all(data).await?;
        self.file.flush().await?;
        self.written += data.len() as u64;
        Ok(())
    }

    async fn take(&mut self, max: usize) -> io::Result<Bytes> {
        let len = usize::try_from(self.pending()).map_or(max, |pending| pending.min(max));
        let mut chunk = vec![0; len];
        self.file.seek(SeekFrom::Start(self.read_at)).await?;
        self.file.read_exact(&mut chunk).await?;
        self.read_at += len as u64;
        if self.read_at == self.written {
            self.file.set_len(0).await?;
            self.read_at = 0;
            self.written = 0;
        }
        Ok(Bytes::from(chunk))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Upstream {
    Open,
    Eof,
    Closed,
}

async fn spill(spill: &mut Option<Spill>, data: &[u8]) -> io::Result<()> {
    if let Some(spill) = spill {
        return spill.append(data).await;
    }
    spill.insert(Spill::create().await?).append(data).await
}

async fn pump(
    mut read: ChannelReadHalf, write: ChannelWriteHalf<Msg>, shared: Arc<Shared>,
    released: CancellationToken, _permit: Option<OwnedSemaphorePermit>,
) {
    let mut overflow: Option<Spill> = None;
    let mut upstream = Upstream::Open;
    loop {
        let spilled = overflow.as_ref().map_or(0, Spill::pending);
        if spilled > 0 {
            let room = shared.lock().room();
            if room > 0
                && let Some(spill) = overflow.as_mut()
            {
                match spill.take(room).await {
                    Ok(chunk) => shared.lock().push(chunk),
                    Err(e) => {
                        log::warn!("Could not read back SSH channel data kept on disk: {e}");
                        shared.lock().end(End::Reset);
                        break;
                    }
                }
                continue;
            }
        } else if upstream != Upstream::Open {
            shared.lock().end(End::Eof);
            if upstream == Upstream::Closed {
                return;
            }
        }
        tokio::select! {
            msg = read.wait(), if upstream != Upstream::Closed => match msg {
                Some(ChannelMsg::Data { data }) => {
                    let fits = spilled == 0 && data.len() <= shared.lock().room();
                    if fits {
                        shared.lock().push(data);
                    } else if let Err(e) = spill(&mut overflow, &data).await {
                        log::warn!("Could not keep SSH channel data on disk: {e}");
                        shared.lock().end(End::Reset);
                        break;
                    }
                }
                Some(ChannelMsg::Eof) => upstream = Upstream::Eof,
                Some(ChannelMsg::Close) | None => upstream = Upstream::Closed,
                Some(_) => {}
            },
            () = shared.room_freed.notified(), if spilled > 0 => {}
            () = released.cancelled() => break,
        }
    }
    drop(read);
    let _ = write.close().await;
}

pub(crate) struct SshStream {
    shared: Arc<Shared>,
    outbound: Pin<Box<dyn AsyncWrite + Send>>,
    _released: DropGuard,
}

impl SshStream {
    fn start(channel: Channel<Msg>, permit: Option<OwnedSemaphorePermit>) -> Self {
        let (read, write) = channel.split();
        let shared = Arc::new(Shared::default());
        let released = CancellationToken::new();
        let outbound = Box::pin(write.make_writer());
        tokio::spawn(pump(
            read,
            write,
            Arc::clone(&shared),
            released.clone(),
            permit,
        ));
        Self {
            shared,
            outbound,
            _released: released.drop_guard(),
        }
    }
}

impl AsyncRead for SshStream {
    fn poll_read(
        self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let mut inbound = self.shared.lock();
        if let Some(front) = inbound.chunks.front_mut() {
            let n = front.len().min(buf.remaining());
            buf.put_slice(&front.split_to(n));
            if front.is_empty() {
                inbound.chunks.pop_front();
            }
            inbound.buffered -= n;
            drop(inbound);
            self.shared.room_freed.notify_one();
            return Poll::Ready(Ok(()));
        }
        match inbound.end {
            Some(End::Eof) => Poll::Ready(Ok(())),
            Some(End::Reset) => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::ConnectionReset,
                "the SSH channel data kept on disk could not be stored or read back",
            ))),
            None => {
                inbound.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

impl AsyncWrite for SshStream {
    fn poll_write(
        mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.shared.lock().end == Some(End::Reset) {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        self.outbound.as_mut().poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.outbound.as_mut().poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.outbound.as_mut().poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::{
        AsyncReadExt,
        AsyncWriteExt,
    };
    use tokio::net::TcpListener;
    use tokio::time::timeout;

    use super::*;
    use crate::test_support::{
        Fixture,
        peer,
    };
    use crate::testing::MockSshdOptions;

    async fn echo_roundtrip(opener: &dyn StreamOpener, payload: &[u8]) {
        let mut stream = opener.open(peer()).await.unwrap();
        stream.write_all(payload).await.unwrap();
        let mut back = vec![0; payload.len()];
        stream.read_exact(&mut back).await.unwrap();
        assert_eq!(back, payload);
    }

    #[tokio::test]
    async fn open_should_fail_with_forwarding_prohibited_when_the_sshd_forbids_forwarding() {
        let fixture = Fixture::with(MockSshdOptions {
            forwarding: false,
            ..Default::default()
        })
        .await;
        fixture.authorize_key();
        let lease = fixture.acquire("no-forwarding").await.unwrap();

        let err = fixture
            .echo_opener(&lease)
            .open(peer())
            .await
            .err()
            .unwrap();

        assert!(
            matches!(err, SshError::ForwardingProhibited { .. }),
            "{err}"
        );
        // Only that connection failed.
        assert_eq!(*lease.state().borrow(), SessionState::Ready);
    }

    #[tokio::test]
    async fn open_should_fail_with_target_unreachable_when_the_port_is_closed() {
        let fixture = Fixture::new().await;
        let lease = fixture.acquire("closed-port").await.unwrap();
        let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);

        let err = lease
            .opener("127.0.0.1".to_owned(), port)
            .open(peer())
            .await
            .err()
            .unwrap();

        assert!(
            matches!(&err, SshError::TargetUnreachable { port: p, .. } if *p == port),
            "{err}"
        );
        // The session survives and still serves other channels.
        echo_roundtrip(&fixture.echo_opener(&lease), b"still alive").await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn open_should_serve_100_parallel_channels_through_one_lease() {
        let fixture = Fixture::new().await;
        let lease = fixture.acquire("parallel").await.unwrap();
        let opener = Arc::new(fixture.echo_opener(&lease));

        let tasks: Vec<_> = (0..100u8)
            .map(|i| {
                let opener = Arc::clone(&opener);
                tokio::spawn(async move {
                    // Small: russh flushes without reading, so a 64 KiB in-memory pipe
                    // full in both directions would stall the whole session.
                    echo_roundtrip(&*opener, &[i; 128]).await;
                })
            })
            .collect();
        for task in tasks {
            timeout(Duration::from_secs(60), task)
                .await
                .unwrap()
                .unwrap();
        }

        assert_eq!(fixture.mock.session_count(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn open_should_wait_for_a_free_slot_when_the_session_is_at_its_channel_cap() {
        let fixture = Fixture::new().await;
        let lease = fixture.acquire("cap").await.unwrap();
        let opener = Arc::new(fixture.echo_opener(&lease));
        let mut open = Vec::new();
        for _ in 0..crate::consts::MAX_CHANNELS_PER_SESSION {
            open.push(opener.open(peer()).await.unwrap());
        }
        assert_eq!(lease.0.channels.available_permits(), 0);

        let mut waiting = tokio::spawn({
            let opener = Arc::clone(&opener);
            async move { opener.open(peer()).await.map(drop) }
        });
        // Still blocked a while after the cap was reached.
        assert!(
            timeout(Duration::from_millis(300), &mut waiting)
                .await
                .is_err(),
            "the open beyond the cap did not wait"
        );

        // A freed slot lets it through.
        drop(open.pop());
        timeout(Duration::from_secs(10), waiting)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }

    const WAIT: Duration = Duration::from_secs(10);

    async fn flood_source(bytes: usize) -> (u16, oneshot::Receiver<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (sent_tx, sent_rx) = oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let _ = socket.write_all(&vec![7; bytes]).await;
            let _ = sent_tx.send(());
            let _ = socket.read(&mut [0; 1]).await;
        });
        (port, sent_rx)
    }

    async fn all_permits_back(lease: &SshLease) {
        timeout(WAIT, async {
            while lease.0.channels.available_permits() < crate::consts::MAX_CHANNELS_PER_SESSION {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("a channel kept its slot after it closed");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_lagging_reader_should_not_stall_the_other_channels() {
        let fixture = Fixture::new().await;
        let lease = fixture.acquire("lagging").await.unwrap();
        let flood = CHANNEL_READ_BUFFER / 2;
        let (port, sent) = flood_source(flood).await;
        let mut lagging = lease
            .opener("127.0.0.1".to_owned(), port)
            .open(peer())
            .await
            .unwrap();
        let _ = timeout(Duration::from_millis(500), sent).await;

        let opener = fixture.echo_opener(&lease);
        for _ in 0..3 {
            timeout(WAIT, echo_roundtrip(&opener, b"other"))
                .await
                .expect("a channel with a lagging reader stalled the session");
        }
        assert_eq!(*lease.state().borrow(), SessionState::Ready);

        let mut received = vec![0; flood];
        timeout(WAIT, lagging.read_exact(&mut received))
            .await
            .unwrap()
            .unwrap();
        assert!(received.iter().all(|byte| *byte == 7));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_reader_that_falls_far_behind_should_still_get_every_byte_in_order() {
        let fixture = Fixture::new().await;
        let lease = fixture.acquire("overflow").await.unwrap();
        let payload: Vec<u8> = (0..3 * CHANNEL_READ_BUFFER + 4096)
            .map(|i| (i % 251) as u8)
            .collect();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (sent_tx, sent) = oneshot::channel();
        tokio::spawn({
            let payload = payload.clone();
            async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                socket.write_all(&payload).await.unwrap();
                socket.shutdown().await.unwrap();
                let _ = sent_tx.send(());
            }
        });
        let mut lagging = lease
            .opener("127.0.0.1".to_owned(), port)
            .open(peer())
            .await
            .unwrap();
        timeout(WAIT, sent).await.unwrap().unwrap();

        let opener = fixture.echo_opener(&lease);
        timeout(WAIT, echo_roundtrip(&opener, b"other"))
            .await
            .expect("a channel with a lagging reader stalled the session");
        let mut received = Vec::new();
        timeout(WAIT, lagging.read_to_end(&mut received))
            .await
            .unwrap()
            .unwrap();

        assert!(
            received == payload,
            "the lagging reader lost or reordered bytes"
        );
        assert_eq!(*lease.state().borrow(), SessionState::Ready);
        drop(lagging);
        all_permits_back(&lease).await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn open_should_close_the_channel_a_late_confirmation_opens_after_the_caller_gave_up() {
        let fixture = Fixture::with(MockSshdOptions {
            hold_channel_opens: true,
            ..Default::default()
        })
        .await;
        fixture.authorize_key();
        let lease = fixture.acquire("late").await.unwrap();
        let opener = fixture.echo_opener(&lease);

        let outcome = timeout(Duration::from_millis(200), opener.open(peer())).await;
        assert!(outcome.is_err(), "the sshd answered a held open");
        assert!(fixture.mock.wait_held_open().await);
        assert_eq!(
            lease.0.channels.available_permits(),
            crate::consts::MAX_CHANNELS_PER_SESSION - 1,
            "the open the caller gave up on lost its slot of the cap"
        );

        fixture.mock.release_held_opens();

        all_permits_back(&lease).await;
        assert_eq!(fixture.mock.accepted_channels(), 1);
        assert!(
            fixture.mock.wait_channel_count(0).await,
            "the late channel stayed open on the sshd"
        );
    }
}

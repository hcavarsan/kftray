use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{
    Context,
    Poll,
};

use async_trait::async_trait;
use russh::client::{
    Handle,
    Msg,
};
use russh::{
    ChannelOpenFailure,
    ChannelStream,
};
use tokio::io::{
    AsyncRead,
    AsyncWrite,
    ReadBuf,
};
use tokio::sync::OwnedSemaphorePermit;
use tokio::time::{
    Instant,
    timeout_at,
};

use super::connect::protocol_error;
use super::handler::ClientHandler;
use super::pool::SessionEntry;
use super::{
    SessionState,
    SshLease,
    Stage,
};
use crate::consts::CHANNEL_OPEN_TIMEOUT;
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

        // Released with the stream; waits while the session is at its channel cap.
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
        let channel =
            open_direct_tcpip(&handle, label, &self.host, self.port, peer, deadline).await?;

        log::debug!(
            "[ssh {label}] channel to {}:{} for {peer}",
            self.host,
            self.port
        );
        Ok(Box::new(PermitStream {
            inner: Box::pin(channel.into_stream()),
            _permit: permit,
        }))
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
    handle: &Handle<ClientHandler>, label: &str, host: &str, port: u16, peer: SocketAddr,
    deadline: Instant,
) -> Result<russh::Channel<Msg>, SshError> {
    let unreachable = || SshError::TargetUnreachable {
        route: label.to_owned(),
        host: host.to_owned(),
        port,
    };
    let opening = handle.channel_open_direct_tcpip(
        host,
        u32::from(port),
        peer.ip().to_string(),
        u32::from(peer.port()),
    );
    match timeout_at(deadline, opening).await {
        Ok(Ok(channel)) => Ok(channel),
        Ok(Err(russh::Error::ChannelOpenFailure(
            ChannelOpenFailure::AdministrativelyProhibited,
        ))) => Err(SshError::ForwardingProhibited {
            route: label.to_owned(),
        }),
        Ok(Err(russh::Error::ChannelOpenFailure(ChannelOpenFailure::ConnectFailed))) | Err(_) => {
            Err(unreachable())
        }
        Ok(Err(e)) => Err(protocol_error(label, e)),
    }
}

/// A channel that keeps its slot of the session's channel cap until dropped.
struct PermitStream {
    inner: Pin<Box<ChannelStream<Msg>>>,
    _permit: OwnedSemaphorePermit,
}

impl AsyncRead for PermitStream {
    fn poll_read(
        mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        self.inner.as_mut().poll_read(cx, buf)
    }
}

impl AsyncWrite for PermitStream {
    fn poll_write(
        mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.inner.as_mut().poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.inner.as_mut().poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.inner.as_mut().poll_shutdown(cx)
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
}

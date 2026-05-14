use std::{
    io,
    pin::Pin,
    sync::{
        Arc,
        atomic::{
            AtomicBool,
            Ordering,
        },
    },
    task::{
        Context,
        Poll,
    },
};

use bytes::{
    Buf,
    Bytes,
};
use tokio::{
    io::{
        AsyncBufRead,
        AsyncRead,
        AsyncWrite,
        ReadBuf,
    },
    sync::mpsc,
};
use tokio_tungstenite::tungstenite;
use tokio_util::sync::PollSender;

use crate::frame;
use crate::session::ReleaseGuard;

pub(crate) struct ChannelHalf {
    rx: mpsc::Receiver<Bytes>,
    writer_mailbox: Option<PollSender<tungstenite::Message>>,
    channel_id: u8,
    read_buf: Option<Bytes>,
    read_eof: bool,
}

impl ChannelHalf {
    pub(crate) fn pair(
        channel_id: u8, writer_mailbox: mpsc::Sender<tungstenite::Message>,
    ) -> (Self, mpsc::Sender<Bytes>) {
        let (inbound_tx, inbound_rx) = mpsc::channel(64);
        let half = Self {
            rx: inbound_rx,
            writer_mailbox: Some(PollSender::new(writer_mailbox)),
            channel_id,
            read_buf: None,
            read_eof: false,
        };
        (half, inbound_tx)
    }
}

impl AsyncRead for ChannelHalf {
    fn poll_read(
        mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        // Drain buffered data first
        if let Some(mut bytes) = self.read_buf.take() {
            let n = bytes.len().min(buf.remaining());
            let dst = buf.initialize_unfilled_to(n);
            bytes.copy_to_slice(dst);
            buf.advance(n);
            if !bytes.is_empty() {
                self.read_buf = Some(bytes);
            }
            return Poll::Ready(Ok(()));
        }

        if self.read_eof {
            return Poll::Ready(Ok(()));
        }

        // No buffered data — poll the channel
        match self.rx.poll_recv(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => {
                self.read_eof = true;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Some(bytes)) => {
                self.read_buf = Some(bytes);
                // Re-enter to copy from the newly buffered data
                Pin::new(&mut *self).poll_read(cx, buf)
            }
        }
    }
}

impl AsyncBufRead for ChannelHalf {
    fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        let this = self.get_mut();
        loop {
            if this.read_buf.as_ref().is_some_and(|b| !b.is_empty()) {
                // as_deref() gives us &[u8] borrowed from this.read_buf, which
                // satisfies the 'self lifetime requirement of poll_fill_buf.
                // unwrap is safe: we just verified Some + non-empty above.
                return Poll::Ready(Ok(this.read_buf.as_deref().unwrap()));
            }
            if this.read_buf.is_some() {
                this.read_buf = None;
            }
            if this.read_eof {
                return Poll::Ready(Ok(&[]));
            }
            match this.rx.poll_recv(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => {
                    this.read_eof = true;
                    return Poll::Ready(Ok(&[]));
                }
                Poll::Ready(Some(b)) => {
                    this.read_buf = Some(b);
                }
            }
        }
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        let this = self.get_mut();
        if let Some(ref mut bytes) = this.read_buf {
            let consumed = amt.min(bytes.len());
            bytes.advance(consumed);
            if bytes.is_empty() {
                this.read_buf = None;
            }
        }
    }
}

impl AsyncWrite for ChannelHalf {
    fn poll_write(
        mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let reserve = match self.writer_mailbox.as_mut() {
            None => {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "channel closed",
                )));
            }
            Some(tx) => Pin::new(tx).poll_reserve(cx),
        };
        match reserve {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(_)) => {
                self.writer_mailbox = None;
                Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "channel closed",
                )))
            }
            Poll::Ready(Ok(())) => {
                let channel_id = self.channel_id;
                let item = frame::bytes_to_message(frame::encode_channel_frame(channel_id, buf));
                let send_outcome = match self.writer_mailbox.as_mut() {
                    None => Err(()),
                    Some(tx) => tx.send_item(item).map_err(|_| ()),
                };
                match send_outcome {
                    Ok(()) => Poll::Ready(Ok(buf.len())),
                    Err(()) => {
                        self.writer_mailbox = None;
                        Poll::Ready(Err(io::Error::new(
                            io::ErrorKind::BrokenPipe,
                            "channel closed",
                        )))
                    }
                }
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.writer_mailbox = None;
        Poll::Ready(Ok(()))
    }
}

/// Idempotent half-close signaller for a single channel pair.
///
/// On v5 sessions, `fire()` emits `[0xFF, data_channel_id]` and
/// `[0xFF, error_channel_id]` to the writer mailbox, telling the apiserver
/// the client is done writing on this pair. The server tears down its
/// backend pod connection and echoes `0xFF` back; our reader sees that and
/// removes the routing entry, which surfaces as EOF to the user's read
/// half — this is the chain that resolves CLOSE_WAIT on the kftray-side
/// listener.
///
/// On v4 sessions (`supports_close == false`), `fire()` is a no-op at the
/// protocol layer: v4 has no half-close frame, so CLOSE_WAIT on v4 cannot
/// be fixed from the client. v4 is the rare fallback for pre-1.30 clusters
/// (see KEP-4006).
pub(crate) struct ShutdownSignal {
    data_channel: u8,
    error_channel: u8,
    writer_mailbox: mpsc::Sender<tungstenite::Message>,
    supports_close: bool,
    sent: AtomicBool,
}

impl ShutdownSignal {
    pub(crate) fn new(
        data_channel: u8, error_channel: u8, writer_mailbox: mpsc::Sender<tungstenite::Message>,
        supports_close: bool,
    ) -> Arc<Self> {
        Arc::new(Self {
            data_channel,
            error_channel,
            writer_mailbox,
            supports_close,
            sent: AtomicBool::new(false),
        })
    }

    /// Idempotent. Sends 0xFF close signal for both channels on v5;
    /// no-op on v4. Uses `try_send` first to stay non-blocking; falls
    /// back to a spawned async send only if the mailbox is full.
    pub(crate) fn fire(&self) {
        if !self.supports_close {
            return;
        }
        if self.sent.swap(true, Ordering::AcqRel) {
            return;
        }
        let close_data = frame::bytes_to_message(frame::encode_close_signal(self.data_channel));
        let close_error = frame::bytes_to_message(frame::encode_close_signal(self.error_channel));
        for msg in [close_data, close_error] {
            if let Err(err) = self.writer_mailbox.try_send(msg) {
                match err {
                    mpsc::error::TrySendError::Full(msg) => {
                        // fire() can be called from ReleaseGuard::drop (sync context).
                        // tokio::spawn panics if no runtime is active, so guard with
                        // Handle::try_current().
                        match tokio::runtime::Handle::try_current() {
                            Ok(handle) => {
                                let mailbox = self.writer_mailbox.clone();
                                handle.spawn(async move {
                                    let _ = mailbox.send(msg).await;
                                });
                            }
                            Err(_) => {
                                // No runtime — best-effort drop; channel was
                                // full.
                            }
                        }
                    }
                    mpsc::error::TrySendError::Closed(_) => {
                        // Writer task is gone — session is tearing down,
                        // nothing to send.
                    }
                }
            }
        }
    }
}

/// Bidirectional port-forward stream backed by one (data, error) channel pair
/// in a multiplexed WebSocket session. Implements `AsyncRead` + `AsyncWrite`
/// on the data half. Dropping the `Stream` (or calling `poll_shutdown`)
/// emits a v5 close signal so the apiserver tears down the backing pod
/// connection promptly.
pub struct Stream {
    data: ChannelHalf,
    error: ChannelHalf,
    shutdown_signal: Arc<ShutdownSignal>,
    _guard: ReleaseGuard,
}

impl Stream {
    pub(crate) fn new(
        data: ChannelHalf, error: ChannelHalf, shutdown_signal: Arc<ShutdownSignal>,
        guard: ReleaseGuard,
    ) -> Self {
        Self {
            data,
            error,
            shutdown_signal,
            _guard: guard,
        }
    }

    /// Split the stream into its data half (`AsyncRead + AsyncWrite`) and
    /// error half (`AsyncRead`-only). Both halves share the same release
    /// guard; dropping both releases the channel pair. Only the data half
    /// can emit the v5 shutdown signal.
    pub fn split(self) -> (DataStream, ErrorStream) {
        let guard = Arc::new(self._guard);
        (
            DataStream {
                inner: self.data,
                shutdown_signal: self.shutdown_signal,
                _guard: Arc::clone(&guard),
            },
            ErrorStream {
                inner: self.error,
                _guard: guard,
            },
        )
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.data).poll_read(cx, buf)
    }
}

impl AsyncBufRead for Stream {
    fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        let this = self.get_mut();
        Pin::new(&mut this.data).poll_fill_buf(cx)
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        let this = self.get_mut();
        Pin::new(&mut this.data).consume(amt)
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.data).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.data).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.shutdown_signal.fire();
        Pin::new(&mut self.data).poll_shutdown(cx)
    }
}

pub struct DataStream {
    inner: ChannelHalf,
    shutdown_signal: Arc<ShutdownSignal>,
    _guard: Arc<ReleaseGuard>,
}

impl AsyncRead for DataStream {
    fn poll_read(
        mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncBufRead for DataStream {
    fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        let this = self.get_mut();
        Pin::new(&mut this.inner).poll_fill_buf(cx)
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        let this = self.get_mut();
        Pin::new(&mut this.inner).consume(amt)
    }
}

impl AsyncWrite for DataStream {
    fn poll_write(
        mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.shutdown_signal.fire();
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

pub struct ErrorStream {
    inner: ChannelHalf,
    _guard: Arc<ReleaseGuard>,
}

impl AsyncRead for ErrorStream {
    fn poll_read(
        mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

const _ASSERT_TRAITS: fn() = || {
    fn assert<T: AsyncRead + AsyncWrite + Unpin + Send + 'static>() {}
    assert::<Stream>();
    assert::<DataStream>();
};

#[cfg(test)]
mod tests {
    use tokio::io::{
        AsyncReadExt,
        AsyncWriteExt,
    };

    use super::*;

    #[tokio::test]
    async fn channel_half_round_trip() {
        let (writer_tx, mut writer_rx) = mpsc::channel::<tungstenite::Message>(8);
        let (mut half, inbound_tx) = ChannelHalf::pair(7, writer_tx);

        inbound_tx.send(Bytes::from_static(b"hello")).await.unwrap();
        let mut buf = vec![0u8; 5];
        half.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hello");

        half.write_all(b"world").await.unwrap();
        let got = writer_rx.recv().await.unwrap();
        let expected = frame::bytes_to_message(frame::encode_channel_frame(7, b"world"));
        assert_eq!(got, expected);
    }

    #[tokio::test]
    async fn shutdown_signal_v5_emits_close_frames_once() {
        let (writer_tx, mut writer_rx) = mpsc::channel::<tungstenite::Message>(8);
        let sig = ShutdownSignal::new(4, 5, writer_tx, true);

        sig.fire();
        sig.fire();

        let msg1 = writer_rx.recv().await.unwrap();
        let msg2 = writer_rx.recv().await.unwrap();
        assert!(
            writer_rx.try_recv().is_err(),
            "fire() must be idempotent and only emit one pair of close frames"
        );

        let data_close = frame::bytes_to_message(frame::encode_close_signal(4));
        let error_close = frame::bytes_to_message(frame::encode_close_signal(5));
        assert_eq!(msg1, data_close);
        assert_eq!(msg2, error_close);
    }

    #[tokio::test]
    async fn shutdown_signal_v4_is_noop() {
        let (writer_tx, mut writer_rx) = mpsc::channel::<tungstenite::Message>(8);
        let sig = ShutdownSignal::new(0, 1, writer_tx, false);

        sig.fire();

        assert!(writer_rx.try_recv().is_err());
    }
}

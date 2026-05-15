use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{
    Context,
    Poll,
};

use bytes::{
    Buf,
    Bytes,
};
use tokio::io::{
    AsyncBufRead,
    AsyncRead,
    AsyncWrite,
    ReadBuf,
};
use tokio::sync::mpsc;
use tokio_util::sync::PollSender;

use super::mux::{
    MuxCommand,
    MuxHandle,
};

/// Bidirectional SPDY port-forward stream backed by a (data, error) stream
/// pair.
///
/// Implements `AsyncRead + AsyncWrite` on the data half. The error half is
/// available via `split()`.
pub(crate) struct Stream {
    data_id: u32,
    data_rx: mpsc::Receiver<Bytes>,
    error_rx: mpsc::Receiver<Bytes>,
    mux: MuxHandle,
    write_tx: PollSender<MuxCommand>,
    read_buf: Option<Bytes>,
    read_eof: bool,
    /// Guard that sends RST_STREAM and decrements active count on drop.
    _guard: StreamGuard,
}

struct StreamGuard {
    data_id: u32,
    error_id: u32,
    mux: MuxHandle,
}

impl Drop for StreamGuard {
    fn drop(&mut self) {
        self.mux.close_stream(self.data_id);
        self.mux.close_stream(self.error_id);
        self.mux.release_pair();
    }
}

impl Stream {
    pub(crate) fn new(
        data_id: u32, error_id: u32, data_rx: mpsc::Receiver<Bytes>,
        error_rx: mpsc::Receiver<Bytes>, mux: MuxHandle,
    ) -> Self {
        let guard = StreamGuard {
            data_id,
            error_id,
            mux: mux.clone(),
        };
        let write_tx = PollSender::new(mux.cmd_sender());
        Self {
            data_id,
            data_rx,
            error_rx,
            mux,
            write_tx,
            read_buf: None,
            read_eof: false,
            _guard: guard,
        }
    }

    /// Split into data half (AsyncRead + AsyncWrite) and error half
    /// (AsyncRead).
    pub(crate) fn split(self) -> (DataStream, ErrorStream) {
        let guard = Arc::new(self._guard);

        (
            DataStream {
                data_id: self.data_id,
                data_rx: self.data_rx,
                mux: self.mux,
                write_tx: self.write_tx,
                read_buf: self.read_buf,
                read_eof: self.read_eof,
                _guard: Arc::clone(&guard),
            },
            ErrorStream {
                error_rx: self.error_rx,
                error_buf: None,
                error_eof: false,
                _guard: guard,
            },
        )
    }
}

impl Unpin for Stream {}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Shared `poll_read` logic for channel-backed streams.
fn poll_read_channel(
    rx: &mut mpsc::Receiver<Bytes>, read_buf: &mut Option<Bytes>, read_eof: &mut bool,
    cx: &mut Context<'_>, buf: &mut ReadBuf<'_>,
) -> Poll<io::Result<()>> {
    if *read_eof {
        return Poll::Ready(Ok(()));
    }

    // Drain buffered data first
    if let Some(ref mut remaining) = *read_buf {
        let to_copy = remaining.len().min(buf.remaining());
        buf.put_slice(&remaining[..to_copy]);
        if to_copy >= remaining.len() {
            *read_buf = None;
        } else {
            *remaining = remaining.slice(to_copy..);
        }
        return Poll::Ready(Ok(()));
    }

    // Poll channel for more data
    match rx.poll_recv(cx) {
        Poll::Ready(Some(data)) => {
            let to_copy = data.len().min(buf.remaining());
            buf.put_slice(&data[..to_copy]);
            if to_copy < data.len() {
                *read_buf = Some(data.slice(to_copy..));
            }
            Poll::Ready(Ok(()))
        }
        Poll::Ready(None) => {
            *read_eof = true;
            Poll::Ready(Ok(()))
        }
        Poll::Pending => Poll::Pending,
    }
}

/// Shared `consume` logic for `AsyncBufRead`.
fn consume_channel_buf(read_buf: &mut Option<Bytes>, amt: usize) {
    if let Some(ref mut bytes) = *read_buf {
        let consumed = amt.min(bytes.len());
        bytes.advance(consumed);
        if bytes.is_empty() {
            *read_buf = None;
        }
    }
}

/// Shared `poll_write` logic using `PollSender<MuxCommand>`.
fn poll_write_via_sender(
    write_tx: &mut PollSender<MuxCommand>, stream_id: u32, cx: &mut Context<'_>, buf: &[u8],
) -> Poll<io::Result<usize>> {
    match write_tx.poll_reserve(cx) {
        Poll::Ready(Ok(())) => {
            let cmd = MuxCommand::SendData {
                stream_id,
                payload: Bytes::copy_from_slice(buf),
                fin: false,
            };
            match write_tx.send_item(cmd) {
                Ok(()) => Poll::Ready(Ok(buf.len())),
                Err(_) => Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "mux closed",
                ))),
            }
        }
        Poll::Ready(Err(_)) => Poll::Ready(Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "mux closed",
        ))),
        Poll::Pending => Poll::Pending,
    }
}

// ---------------------------------------------------------------------------
// Stream impls
// ---------------------------------------------------------------------------

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        poll_read_channel(
            &mut this.data_rx,
            &mut this.read_buf,
            &mut this.read_eof,
            cx,
            buf,
        )
    }
}

impl AsyncBufRead for Stream {
    fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        let this = self.get_mut();
        loop {
            if this.read_buf.as_ref().is_some_and(|b| !b.is_empty()) {
                return Poll::Ready(Ok(this.read_buf.as_deref().unwrap()));
            }
            if this.read_buf.is_some() {
                this.read_buf = None;
            }
            if this.read_eof {
                return Poll::Ready(Ok(&[]));
            }
            match this.data_rx.poll_recv(cx) {
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
        consume_channel_buf(&mut self.get_mut().read_buf, amt);
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        poll_write_via_sender(&mut this.write_tx, this.data_id, cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        // Send empty DATA frame with FIN to signal write completion
        let _ = this
            .mux
            .send_data_nonblocking(this.data_id, Bytes::new(), true);
        Poll::Ready(Ok(()))
    }
}

// ---------------------------------------------------------------------------
// DataStream
// ---------------------------------------------------------------------------

/// Data half of a split SPDY stream: AsyncRead (from pod) + AsyncWrite (to
/// pod).
pub(crate) struct DataStream {
    data_id: u32,
    data_rx: mpsc::Receiver<Bytes>,
    mux: MuxHandle,
    write_tx: PollSender<MuxCommand>,
    read_buf: Option<Bytes>,
    read_eof: bool,
    _guard: Arc<StreamGuard>,
}

impl Unpin for DataStream {}

impl AsyncRead for DataStream {
    fn poll_read(
        self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        poll_read_channel(
            &mut this.data_rx,
            &mut this.read_buf,
            &mut this.read_eof,
            cx,
            buf,
        )
    }
}

impl AsyncBufRead for DataStream {
    fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        let this = self.get_mut();
        loop {
            if this.read_buf.as_ref().is_some_and(|b| !b.is_empty()) {
                return Poll::Ready(Ok(this.read_buf.as_deref().unwrap()));
            }
            if this.read_buf.is_some() {
                this.read_buf = None;
            }
            if this.read_eof {
                return Poll::Ready(Ok(&[]));
            }
            match this.data_rx.poll_recv(cx) {
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
        consume_channel_buf(&mut self.get_mut().read_buf, amt);
    }
}

impl AsyncWrite for DataStream {
    fn poll_write(
        self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        poll_write_via_sender(&mut this.write_tx, this.data_id, cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let _ = this
            .mux
            .send_data_nonblocking(this.data_id, Bytes::new(), true);
        Poll::Ready(Ok(()))
    }
}

// ---------------------------------------------------------------------------
// ErrorStream
// ---------------------------------------------------------------------------

/// Error half of a split SPDY stream: AsyncRead only (pod error messages).
pub(crate) struct ErrorStream {
    error_rx: mpsc::Receiver<Bytes>,
    error_buf: Option<Bytes>,
    error_eof: bool,
    _guard: Arc<StreamGuard>,
}

impl Unpin for ErrorStream {}

impl AsyncRead for ErrorStream {
    fn poll_read(
        self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        poll_read_channel(
            &mut this.error_rx,
            &mut this.error_buf,
            &mut this.error_eof,
            cx,
            buf,
        )
    }
}

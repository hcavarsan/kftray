use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc;

use super::mux::MuxHandle;

/// Bidirectional SPDY port-forward stream backed by a (data, error) stream pair.
///
/// Implements `AsyncRead + AsyncWrite` on the data half. The error half is
/// available via `split()`.
pub(crate) struct Stream {
    data_id: u32,
    error_id: u32,
    data_rx: mpsc::Receiver<Bytes>,
    error_rx: mpsc::Receiver<Bytes>,
    mux: MuxHandle,
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
        Self {
            data_id,
            error_id,
            data_rx,
            error_rx,
            mux,
            read_buf: None,
            read_eof: false,
            _guard: guard,
        }
    }

    /// Split into data half (AsyncRead + AsyncWrite) and error half (AsyncRead).
    pub(crate) fn split(self) -> (DataStream, ErrorStream) {
        // Prevent the guard from firing — the halves will manage cleanup.
        let guard = SharedGuard::new(self._guard);
        let guard_clone = guard.clone();

        (
            DataStream {
                data_id: self.data_id,
                data_rx: self.data_rx,
                mux: self.mux,
                read_buf: self.read_buf,
                read_eof: self.read_eof,
                _guard: guard,
            },
            ErrorStream {
                error_rx: self.error_rx,
                error_buf: None,
                error_eof: false,
                _guard: guard_clone,
            },
        )
    }
}

// Share the StreamGuard between DataStream and ErrorStream.
// The guard fires when BOTH halves are dropped.
#[derive(Clone)]
struct SharedGuard {
    _inner: std::sync::Arc<StreamGuard>,
}

impl SharedGuard {
    fn new(guard: StreamGuard) -> Self {
        Self {
            _inner: std::sync::Arc::new(guard),
        }
    }
}

impl Unpin for Stream {}

impl AsyncRead for Stream {
    fn poll_read(
        mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.read_eof {
            return Poll::Ready(Ok(()));
        }

        // Drain buffered data first
        if let Some(ref mut remaining) = self.read_buf {
            let to_copy = remaining.len().min(buf.remaining());
            buf.put_slice(&remaining[..to_copy]);
            if to_copy >= remaining.len() {
                self.read_buf = None;
            } else {
                *remaining = remaining.slice(to_copy..);
            }
            return Poll::Ready(Ok(()));
        }

        // Poll channel for more data
        match self.data_rx.poll_recv(cx) {
            Poll::Ready(Some(data)) => {
                let to_copy = data.len().min(buf.remaining());
                buf.put_slice(&data[..to_copy]);
                if to_copy < data.len() {
                    self.read_buf = Some(data.slice(to_copy..));
                }
                Poll::Ready(Ok(()))
            }
            Poll::Ready(None) => {
                self.read_eof = true;
                Poll::Ready(Ok(()))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>, _cx: &mut Context<'_>, buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let data = Bytes::copy_from_slice(buf);
        // Use try_send for non-blocking; if channel is full, we'll report WouldBlock.
        match this.mux.send_data_nonblocking(this.data_id, data, false) {
            Ok(()) => Poll::Ready(Ok(buf.len())),
            Err(_) => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "mux closed",
            ))),
        }
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

/// Data half of a split SPDY stream: AsyncRead (from pod) + AsyncWrite (to pod).
pub(crate) struct DataStream {
    data_id: u32,
    data_rx: mpsc::Receiver<Bytes>,
    mux: MuxHandle,
    read_buf: Option<Bytes>,
    read_eof: bool,
    _guard: SharedGuard,
}

impl Unpin for DataStream {}

impl AsyncRead for DataStream {
    fn poll_read(
        mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.read_eof {
            return Poll::Ready(Ok(()));
        }

        if let Some(ref mut remaining) = self.read_buf {
            let to_copy = remaining.len().min(buf.remaining());
            buf.put_slice(&remaining[..to_copy]);
            if to_copy >= remaining.len() {
                self.read_buf = None;
            } else {
                *remaining = remaining.slice(to_copy..);
            }
            return Poll::Ready(Ok(()));
        }

        match self.data_rx.poll_recv(cx) {
            Poll::Ready(Some(data)) => {
                let to_copy = data.len().min(buf.remaining());
                buf.put_slice(&data[..to_copy]);
                if to_copy < data.len() {
                    self.read_buf = Some(data.slice(to_copy..));
                }
                Poll::Ready(Ok(()))
            }
            Poll::Ready(None) => {
                self.read_eof = true;
                Poll::Ready(Ok(()))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl AsyncWrite for DataStream {
    fn poll_write(
        self: Pin<&mut Self>, _cx: &mut Context<'_>, buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let data = Bytes::copy_from_slice(buf);
        match this.mux.send_data_nonblocking(this.data_id, data, false) {
            Ok(()) => Poll::Ready(Ok(buf.len())),
            Err(_) => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "mux closed",
            ))),
        }
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

/// Error half of a split SPDY stream: AsyncRead only (pod error messages).
pub(crate) struct ErrorStream {
    error_rx: mpsc::Receiver<Bytes>,
    error_buf: Option<Bytes>,
    error_eof: bool,
    _guard: SharedGuard,
}

impl Unpin for ErrorStream {}

impl AsyncRead for ErrorStream {
    fn poll_read(
        mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.error_eof {
            return Poll::Ready(Ok(()));
        }

        if let Some(ref mut remaining) = self.error_buf {
            let to_copy = remaining.len().min(buf.remaining());
            buf.put_slice(&remaining[..to_copy]);
            if to_copy >= remaining.len() {
                self.error_buf = None;
            } else {
                *remaining = remaining.slice(to_copy..);
            }
            return Poll::Ready(Ok(()));
        }

        match self.error_rx.poll_recv(cx) {
            Poll::Ready(Some(data)) => {
                let to_copy = data.len().min(buf.remaining());
                buf.put_slice(&data[..to_copy]);
                if to_copy < data.len() {
                    self.error_buf = Some(data.slice(to_copy..));
                }
                Poll::Ready(Ok(()))
            }
            Poll::Ready(None) => {
                self.error_eof = true;
                Poll::Ready(Ok(()))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

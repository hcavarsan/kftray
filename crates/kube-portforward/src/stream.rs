use std::{
    io,
    pin::Pin,
    task::{
        Context,
        Poll,
    },
};

use tokio::io::{
    AsyncBufRead,
    AsyncRead,
    AsyncWrite,
    ReadBuf,
};

/// Bidirectional port-forward stream backed by one SPDY/3.1 stream pair on
/// a multiplexed connection to the apiserver. Implements
/// `AsyncRead + AsyncWrite` on the data half. Dropping the `Stream` (or
/// calling `poll_shutdown`) sends a SPDY FIN frame so the apiserver tears
/// down the backing pod connection promptly.
pub struct Stream {
    inner: Box<spdy_mux::Stream>,
}

impl Stream {
    pub(crate) fn from_spdy(stream: spdy_mux::Stream) -> Self {
        Self {
            inner: Box::new(stream),
        }
    }

    /// Returns true if the remote has already closed this stream's read
    /// side. Used by spare-stream checkout to discard stale pre-opened
    /// streams before handing them to the relay.
    pub fn is_read_closed(&self) -> bool {
        self.inner.is_read_closed()
    }

    /// Split the stream into its data half (`AsyncRead + AsyncWrite`) and
    /// error half (`AsyncRead`-only).
    pub fn split(self) -> (DataStream, ErrorStream) {
        let (d, e) = (*self.inner).split();
        (DataStream { inner: d }, ErrorStream { inner: e })
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
    }
}

impl AsyncBufRead for Stream {
    fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        Pin::new(&mut self.get_mut().inner).poll_fill_buf(cx)
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        Pin::new(&mut self.get_mut().inner).consume(amt);
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

pub struct DataStream {
    inner: spdy_mux::DataStream,
}

impl AsyncRead for DataStream {
    fn poll_read(
        self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
    }
}

impl AsyncBufRead for DataStream {
    fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        Pin::new(&mut self.get_mut().inner).poll_fill_buf(cx)
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        Pin::new(&mut self.get_mut().inner).consume(amt);
    }
}

impl AsyncWrite for DataStream {
    fn poll_write(
        self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

pub struct ErrorStream {
    inner: spdy_mux::ErrorStream,
}

impl AsyncRead for ErrorStream {
    fn poll_read(
        self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
    }
}

const _ASSERT_TRAITS: fn() = || {
    const fn assert<T: AsyncRead + AsyncWrite + Unpin + Send + 'static>() {}
    assert::<Stream>();
    assert::<DataStream>();
};

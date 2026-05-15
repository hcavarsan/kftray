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

/// Bidirectional port-forward stream backed by one (data, error) channel pair
/// in a multiplexed WebSocket session. Implements `AsyncRead` + `AsyncWrite`
/// on the data half. Dropping the `Stream` (or calling `poll_shutdown`)
/// emits a v5 close signal so the apiserver tears down the backing pod
/// connection promptly.
pub struct Stream {
    inner: StreamInner,
}

enum StreamInner {
    Channel(crate::channel::stream::Stream),
    #[cfg(feature = "spdy-tunnel")]
    Spdy(crate::spdy_tunnel::Stream),
}

impl Stream {
    pub(crate) fn from_channel(stream: crate::channel::stream::Stream) -> Self {
        Self {
            inner: StreamInner::Channel(stream),
        }
    }

    #[cfg(feature = "spdy-tunnel")]
    pub(crate) fn from_spdy(stream: crate::spdy_tunnel::Stream) -> Self {
        Self {
            inner: StreamInner::Spdy(stream),
        }
    }

    /// Split the stream into its data half (`AsyncRead + AsyncWrite`) and
    /// error half (`AsyncRead`-only). Both halves share the same release
    /// guard; dropping both releases the channel pair. Only the data half
    /// can emit the v5 shutdown signal.
    pub fn split(self) -> (DataStream, ErrorStream) {
        match self.inner {
            StreamInner::Channel(s) => {
                let (d, e) = s.split();
                (
                    DataStream {
                        inner: DataStreamInner::Channel(d),
                    },
                    ErrorStream {
                        inner: ErrorStreamInner::Channel(e),
                    },
                )
            }
            #[cfg(feature = "spdy-tunnel")]
            StreamInner::Spdy(s) => {
                let (d, e) = s.split();
                (
                    DataStream {
                        inner: DataStreamInner::Spdy(d),
                    },
                    ErrorStream {
                        inner: ErrorStreamInner::Spdy(e),
                    },
                )
            }
        }
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match &mut self.get_mut().inner {
            StreamInner::Channel(s) => Pin::new(s).poll_read(cx, buf),
            #[cfg(feature = "spdy-tunnel")]
            StreamInner::Spdy(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncBufRead for Stream {
    fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        match &mut self.get_mut().inner {
            StreamInner::Channel(s) => Pin::new(s).poll_fill_buf(cx),
            #[cfg(feature = "spdy-tunnel")]
            StreamInner::Spdy(s) => Pin::new(s).poll_fill_buf(cx),
        }
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        match &mut self.get_mut().inner {
            StreamInner::Channel(s) => Pin::new(s).consume(amt),
            #[cfg(feature = "spdy-tunnel")]
            StreamInner::Spdy(s) => Pin::new(s).consume(amt),
        }
    }
}

impl AsyncWrite for Stream {
    fn poll_write(
        self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match &mut self.get_mut().inner {
            StreamInner::Channel(s) => Pin::new(s).poll_write(cx, buf),
            #[cfg(feature = "spdy-tunnel")]
            StreamInner::Spdy(s) => Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.get_mut().inner {
            StreamInner::Channel(s) => Pin::new(s).poll_flush(cx),
            #[cfg(feature = "spdy-tunnel")]
            StreamInner::Spdy(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.get_mut().inner {
            StreamInner::Channel(s) => Pin::new(s).poll_shutdown(cx),
            #[cfg(feature = "spdy-tunnel")]
            StreamInner::Spdy(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}

pub struct DataStream {
    inner: DataStreamInner,
}

enum DataStreamInner {
    Channel(crate::channel::stream::DataStream),
    #[cfg(feature = "spdy-tunnel")]
    Spdy(crate::spdy_tunnel::DataStream),
}

impl AsyncRead for DataStream {
    fn poll_read(
        self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match &mut self.get_mut().inner {
            DataStreamInner::Channel(s) => Pin::new(s).poll_read(cx, buf),
            #[cfg(feature = "spdy-tunnel")]
            DataStreamInner::Spdy(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncBufRead for DataStream {
    fn poll_fill_buf(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<&[u8]>> {
        match &mut self.get_mut().inner {
            DataStreamInner::Channel(s) => Pin::new(s).poll_fill_buf(cx),
            #[cfg(feature = "spdy-tunnel")]
            DataStreamInner::Spdy(s) => Pin::new(s).poll_fill_buf(cx),
        }
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        match &mut self.get_mut().inner {
            DataStreamInner::Channel(s) => Pin::new(s).consume(amt),
            #[cfg(feature = "spdy-tunnel")]
            DataStreamInner::Spdy(s) => Pin::new(s).consume(amt),
        }
    }
}

impl AsyncWrite for DataStream {
    fn poll_write(
        self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match &mut self.get_mut().inner {
            DataStreamInner::Channel(s) => Pin::new(s).poll_write(cx, buf),
            #[cfg(feature = "spdy-tunnel")]
            DataStreamInner::Spdy(s) => Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.get_mut().inner {
            DataStreamInner::Channel(s) => Pin::new(s).poll_flush(cx),
            #[cfg(feature = "spdy-tunnel")]
            DataStreamInner::Spdy(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match &mut self.get_mut().inner {
            DataStreamInner::Channel(s) => Pin::new(s).poll_shutdown(cx),
            #[cfg(feature = "spdy-tunnel")]
            DataStreamInner::Spdy(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}

pub struct ErrorStream {
    inner: ErrorStreamInner,
}

enum ErrorStreamInner {
    Channel(crate::channel::stream::ErrorStream),
    #[cfg(feature = "spdy-tunnel")]
    Spdy(crate::spdy_tunnel::ErrorStream),
}

impl AsyncRead for ErrorStream {
    fn poll_read(
        self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match &mut self.get_mut().inner {
            ErrorStreamInner::Channel(s) => Pin::new(s).poll_read(cx, buf),
            #[cfg(feature = "spdy-tunnel")]
            ErrorStreamInner::Spdy(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

const _ASSERT_TRAITS: fn() = || {
    fn assert<T: AsyncRead + AsyncWrite + Unpin + Send + 'static>() {}
    assert::<Stream>();
    assert::<DataStream>();
};

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use tokio::io::{
        AsyncReadExt,
        AsyncWriteExt,
    };
    use tokio::sync::mpsc;
    use tokio_tungstenite::tungstenite;

    use crate::channel::frame;
    use crate::channel::stream::{
        ChannelHalf,
        ShutdownSignal,
    };

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

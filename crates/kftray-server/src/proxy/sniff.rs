//! Non-destructive protocol sniffing for the TCP proxy dispatcher.
//!
//! Uses `TcpStream::peek` so the inbound stream is left untouched and can be
//! handed to a hyper server (HTTP) or a raw byte-pump (TLS / unknown) without
//! a buffered-prefix wrapper.

use std::io;

use tokio::net::TcpStream;

/// Result of sniffing the first bytes of a freshly accepted TCP connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// HTTP/1.x request (detected by a known method token).
    Http1,
    /// HTTP/2 cleartext preface (`PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n`).
    Http2,
    /// TLS ClientHello (handshake record, version >= TLS 1.0).
    Tls,
    /// Anything else, including empty / slow openers.
    Unknown,
}

const PEEK_LEN: usize = 32;
const HTTP2_PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

/// Classify the protocol of an inbound connection by peeking at up to 32 bytes.
///
/// Blocks until the client sends its first byte (or closes). This is critical
/// for the K8s WS port-forward case, where the apiserver pre-allocates many
/// inbound TCPs at handshake but data only flows on them later as the client
/// routes browser TCPs through multiplexed channels. A premature timeout would
/// misclassify every idle pre-allocated slot as Unknown and bypass HTTP-aware
/// dispatch entirely.
///
/// Returns the detected protocol. The stream is not consumed; the caller may
/// hand the same `TcpStream` to hyper or to a raw relay without any wrapper.
pub async fn classify(stream: &TcpStream) -> io::Result<Protocol> {
    let mut buf = [0u8; PEEK_LEN];
    let n = stream.peek(&mut buf).await?;
    let kind = classify_bytes(&buf[..n]);
    tracing::trace!(?kind, peek_bytes = n, "sniff classified");
    Ok(kind)
}

fn classify_bytes(b: &[u8]) -> Protocol {
    if b.starts_with(HTTP2_PREFACE) {
        return Protocol::Http2;
    }
    if is_http1_request_line(b) {
        return Protocol::Http1;
    }
    if b.len() >= 3 && b[0] == 0x16 && b[1] == 0x03 {
        return Protocol::Tls;
    }
    Protocol::Unknown
}

fn is_http1_request_line(b: &[u8]) -> bool {
    const METHODS: &[&[u8]] = &[
        b"GET ",
        b"POST ",
        b"PUT ",
        b"HEAD ",
        b"DELETE ",
        b"OPTIONS ",
        b"PATCH ",
        b"CONNECT ",
        b"TRACE ",
    ];
    METHODS.iter().any(|m| b.starts_with(m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_get_returns_http1() {
        assert_eq!(classify_bytes(b"GET /foo HTTP/1.1\r\n"), Protocol::Http1);
    }

    #[test]
    fn classify_post_returns_http1() {
        assert_eq!(classify_bytes(b"POST /x HTTP/1.1\r\n"), Protocol::Http1);
    }

    #[test]
    fn classify_http2_preface_returns_http2() {
        assert_eq!(classify_bytes(HTTP2_PREFACE), Protocol::Http2);
    }

    #[test]
    fn classify_tls_clienthello_returns_tls() {
        let bytes = [0x16, 0x03, 0x01, 0x00, 0xa0];
        assert_eq!(classify_bytes(&bytes), Protocol::Tls);
    }

    #[test]
    fn classify_random_bytes_returns_unknown() {
        assert_eq!(classify_bytes(b"\x00\x01\x02\x03random"), Protocol::Unknown);
    }

    #[test]
    fn classify_empty_returns_unknown() {
        assert_eq!(classify_bytes(&[]), Protocol::Unknown);
    }

    #[tokio::test]
    async fn classify_live_stream_http1() {
        use tokio::io::AsyncWriteExt;
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            classify(&stream).await.unwrap()
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        client.write_all(b"GET / HTTP/1.1\r\n").await.unwrap();

        let result = server.await.unwrap();
        assert_eq!(result, Protocol::Http1);
    }
}

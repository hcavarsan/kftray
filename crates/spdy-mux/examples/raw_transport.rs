//! Paired spdy/3.1 stream over a raw (non-websocket) transport.
//!
//! this won't actually connect to anything: the `tokio::io::duplex` below
//! has no spdy peer on the other end, and `Session::with_config` does an
//! initial PING and waits for a reply, so the example hangs against the
//! loopback. swap the duplex for the `hyper::upgrade::Upgraded` your http
//! client returns after a successful upgrade handshake.

use spdy_mux::{
    MuxConfig,
    Session,
    split_raw_spdy,
};
use tokio::io::{
    AsyncReadExt,
    AsyncWriteExt,
    duplex,
};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Replace `upgraded` with the connection your HTTP client returns
    // after a successful raw HTTP/1.1 upgrade to your SPDY peer.
    let (upgraded, _peer) = duplex(64 * 1024);
    let (writer, reader) = split_raw_spdy(upgraded);

    let cancel = CancellationToken::new();

    let session =
        Session::with_config(vec![(writer, reader)], cancel.clone(), MuxConfig::default()).await?;

    // Build whatever SYN_STREAM headers your peer expects. The example
    // headers below match Kubernetes `portforward.k8s.io v1`: a paired
    // error/data stream keyed by `requestid`, with the target pod port
    // in `port`. Your peer's convention may differ.
    let error_headers = vec![
        ("streamtype".into(), "error".into()),
        ("port".into(), "8080".into()),
        ("requestid".into(), "0".into()),
    ];
    let data_headers = vec![
        ("streamtype".into(), "data".into()),
        ("port".into(), "8080".into()),
        ("requestid".into(), "0".into()),
    ];

    let mut stream = session.open_stream_pair(error_headers, data_headers).await?;
    stream
        .write_all(b"GET /health HTTP/1.0\r\nHost: peer\r\n\r\n")
        .await?;

    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await?;
    println!("read {n} bytes from peer");

    session.close().await?;
    Ok(())
}

//! Raw SPDY/3.1 over an HTTP/1.1 upgraded connection.
//!
//! Shape demo: this file will not actually connect to anything because the
//! placeholder `tokio::io::duplex` has no SPDY peer on the other end.
//! `Session::with_config` performs an initial PING and waits for a reply,
//! so the example will hang against the loopback duplex. Swap the duplex
//! pair for a `hyper::upgrade::Upgraded` returned by your HTTP client
//! after a successful `Upgrade: SPDY/3.1` handshake.

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
    // Replace this with the upgraded connection your HTTP client returns
    // after sending the legacy SPDY upgrade:
    //   POST /api/v1/namespaces/{ns}/pods/{pod}/portforward
    //   Connection: Upgrade
    //   Upgrade: SPDY/3.1
    //   X-Stream-Protocol-Version: portforward.k8s.io
    let (upgraded, _peer) = duplex(64 * 1024);
    let (writer, reader) = split_raw_spdy(upgraded);

    let cancel = CancellationToken::new();
    let target_port: u16 = 8080;

    let session = Session::with_config(
        vec![(writer, reader)],
        target_port,
        cancel.clone(),
        MuxConfig::default(),
    )
    .await?;

    let mut stream = session.connect().await?;
    stream
        .write_all(b"GET /health HTTP/1.0\r\nHost: pod\r\n\r\n")
        .await?;

    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await?;
    println!("read {n} bytes from pod {target_port}");

    session.close().await?;
    Ok(())
}

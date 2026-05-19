//! SPDY/3.1 frames carried inside WebSocket binary messages.
//!
//! Shape demo: like `raw_transport.rs`, this file will not connect to a
//! real peer. Swap the duplex placeholder for the
//! `hyper::upgrade::Upgraded` returned after a successful WebSocket
//! handshake that negotiated `Sec-WebSocket-Protocol: SPDY/3.1+portforward.k8s.io`.
//!
//! Pool sizing: pass several `(writer, reader)` pairs to
//! `Session::with_config` to run multiple WebSocket connections behind
//! one session. The mux distributes streams across them with
//! power-of-two-choices routing.

use spdy_mux::{
    MuxConfig,
    Session,
    split_fastws,
};
use tokio::io::{
    AsyncReadExt,
    AsyncWriteExt,
    duplex,
};
use tokio_util::sync::CancellationToken;

const POOL_SIZE: usize = 4;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Open `POOL_SIZE` parallel WebSocket upgrades in your HTTP client,
    // then hand each `Upgraded` connection to `split_fastws`. The duplex
    // pairs below stand in for them.
    let mut pairs = Vec::with_capacity(POOL_SIZE);
    for _ in 0..POOL_SIZE {
        let (upgraded, _peer) = duplex(64 * 1024);
        pairs.push(split_fastws(upgraded));
    }

    let cancel = CancellationToken::new();
    let target_port: u16 = 9090;

    let session = Session::with_config(
        pairs,
        target_port,
        cancel.clone(),
        MuxConfig {
            pool_size: POOL_SIZE,
            ..Default::default()
        },
    )
    .await?;

    // Fan a handful of concurrent requests through the pool. Each
    // `connect()` returns a fresh SPDY stream; the mux picks the
    // least-loaded pool member for you.
    let mut tasks = Vec::new();
    for i in 0..8 {
        let session = &session;
        tasks.push(async move {
            let mut stream = session.connect().await?;
            let req = format!("GET /?q={i} HTTP/1.0\r\nHost: pod\r\n\r\n");
            stream.write_all(req.as_bytes()).await?;
            let mut buf = vec![0u8; 4096];
            let n = stream.read(&mut buf).await?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(n)
        });
    }

    for (i, t) in tasks.into_iter().enumerate() {
        match t.await {
            Ok(n) => println!("stream {i}: {n} bytes"),
            Err(e) => eprintln!("stream {i}: {e}"),
        }
    }

    session.close().await?;
    Ok(())
}

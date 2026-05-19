//! Open paired SPDY/3.1 streams over a pool of WebSocket transports.
//!
//! Shape demo, same caveat as `raw_transport.rs`: replace the duplex
//! placeholders with `hyper::upgrade::Upgraded` connections returned by
//! your HTTP client after a successful WebSocket handshake.
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

fn headers_for(stream_kind: &str, request_id: u32) -> Vec<(String, String)> {
    vec![
        ("streamtype".into(), stream_kind.into()),
        ("port".into(), "9090".into()),
        ("requestid".into(), request_id.to_string()),
    ]
}

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

    let session = Session::with_config(
        pairs,
        cancel.clone(),
        MuxConfig {
            pool_size: POOL_SIZE,
            ..Default::default()
        },
    )
    .await?;

    // Fan a handful of concurrent paired streams through the pool. Each
    // `open_stream_pair` picks the least-loaded pool member and emits
    // SYN_STREAMs atomically.
    let mut tasks = Vec::new();
    for i in 0..8 {
        let session = &session;
        tasks.push(async move {
            let mut stream = session
                .open_stream_pair(headers_for("error", i), headers_for("data", i))
                .await?;
            let req = format!("GET /?q={i} HTTP/1.0\r\nHost: peer\r\n\r\n");
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

# spdy-mux

SPDY/3.1 stream multiplexer in Rust.

## Why

The Rust ecosystem doesn't have a working SPDY/3.1 multiplexer. The ones I found are either unmaintained, implement only the byte-channel framing that runs inside a SPDY stream, or stub out the protocol path with a `todo!()` pointing at the Go reference implementation.

This crate fills that gap. It speaks SPDY/3.1 framing with the standard zlib dictionary, runs a small pool of parallel transports with power-of-two-choices load balancing, and handles per-stream and session-level flow control plus PING keepalive.

It knows nothing about Kubernetes. Headers are caller-supplied, and the codec sends them verbatim.

## Who needs it

Mostly people doing Kubernetes port-forward, or talking to a CRI runtime like containerd or CRI-O. SPDY is no longer used in browsers or in the proxy ecosystem, and Kubernetes itself is migrating off it (KEP-4006: WebSocket-tunneled streaming, Beta since 1.31, kubelet leg Beta in 1.36).

If you're picking a streaming protocol from scratch in 2026, use HTTP/2 or QUIC. SPDY is here because Kubernetes still uses it, and that migration will take years.

## The shape

Every SPDY/3.1 peer I've encountered uses the same pattern: open two streams together, one for data and one for errors, with the error stream half-closed at open time. The API enforces this. You call `open_stream_pair(error_headers, data_headers)` and get back a `Stream` that wraps the two together.

If your peer wants single streams or a different pair convention, this crate won't fit. The codec layer (`codec.rs`, `dictionary.rs`, `transport.rs`) is pure SPDY/3.1 framing and you could build a different session type on top of it, but the multiplexer API is opinionated.

## Tradeoffs

No community. Nobody else is fuzz-testing Rust SPDY code. The Go reference implementation still receives security fixes in 2026 (header accounting, frame length enforcement), and those bug classes apply to any SPDY/3.1 implementation. I track upstream commits for relevant fixes.

Lazy open is built in. The SYN_STREAM frame doesn't go on the wire until the consumer writes its first byte. The kubelet dials the upstream eagerly on SYN_STREAM, and a fast-closing target server will close the idle TCP before the consumer ever uses it. Lazy open avoids that race.

Zlib header compression carries the CRIME attack surface. SPDY/3.1 was the original target of CVE-2012-4930. Inside a Kubernetes API server connection over TLS this is low risk, but the attack class is real.

## Quick start

```rust,no_run
use spdy_mux::{MuxConfig, Session, split_raw_spdy};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

# async fn run<S>(upgraded: S) -> Result<(), Box<dyn std::error::Error>>
# where S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static {
let (writer, reader) = split_raw_spdy(upgraded);
let cancel = CancellationToken::new();

let session =
    Session::with_config(vec![(writer, reader)], cancel, MuxConfig::default()).await?;

// Headers are caller-supplied. The codec sends them verbatim.
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
stream.write_all(b"GET / HTTP/1.0\r\n\r\n").await?;
# Ok(()) }
```

For a pool of parallel transports, pass them all to `with_config`:

```rust,ignore
let pairs: Vec<_> = upgrades.into_iter().map(split_raw_spdy).collect();
let session = Session::with_config(pairs, cancel, MuxConfig::default()).await?;
```

Dead pool members self-evict and the session keeps serving from the remaining transports.

## Architecture

```
Session::open_stream_pair(error_headers, data_headers)
  |
  v
Pool of MuxHandles (P2C routing on inflight x rtt estimate)
  |
  v
MuxHandle: 1 reader + 5 worker tasks + 1 writer + 1 supervisor
  |
  v
WsFrameReader / WsFrameWriter (transport adapter)
fastwebsockets, or raw AsyncRead + AsyncWrite
```

Each connection runs one reader task, five frame workers partitioned by `stream_id % 5`, one writer task, and a supervisor that cancels the session if any task exits unexpectedly. The codec is shared across all tasks on a connection.

## The longer story

This exists for the same reason `kube-portforward` exists one layer up. I was building a port-forward desktop tool that fell over under wrk and vegeta load. The fix was multiplexing, and there was nothing in the Rust ecosystem to multiplex with.

Reading the Go reference implementation and tracing kubectl wire bytes against a real cluster was the only way to get to a working codec. Most of it followed the SPDY/3.1 spec directly. The hard parts were wire-order quirks that aren't in the spec but the kubelet enforces anyway. The `writer.rs` comments call those out where I found them.

## Examples

`examples/raw_transport.rs` and `examples/websocket_transport.rs` show the setup shape. Neither runs standalone, both need a real upgraded transport from your HTTP client. They compile, and the duplex placeholder makes the API surface obvious.

## License

GPL-3.0.

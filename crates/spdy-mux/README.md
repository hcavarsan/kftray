# spdy-mux

SPDY/3.1 stream multiplexer over async transports.

kftray uses this crate to multiplex Kubernetes port-forward streams through
the apiserver. The codec, mux, and stream layers know nothing about
Kubernetes; they speak SPDY/3.1 over any `AsyncRead + AsyncWrite` transport.

## What you get

- Full SPDY/3.1 frame codec (SYN_STREAM, SYN_REPLY, DATA, RST_STREAM, PING,
  SETTINGS, GOAWAY, WINDOW_UPDATE) with the standard zlib header compression
  dictionary
- Connection pool with power-of-two-choices routing and peak-EWMA RTT
  tracking, so streams flow through the least-loaded socket
- Flow control with per-stream and session-level send windows
- Idle ping watchdog that tears down the session if the peer goes silent
- Two ready-made transport adapters:
  - `FastWsReader` / `FastWsWriter`: SPDY frames carried inside WebSocket
    binary messages (the `SPDY/3.1+portforward.k8s.io` path)
  - `RawSpdyReader` / `RawSpdyWriter`: SPDY frames written directly to an
    upgraded HTTP/1.1 connection (the legacy `Upgrade: SPDY/3.1` path)

Plug in your own transport by implementing the `WsFrameReader` and
`WsFrameWriter` traits.

## When you want it

You want this if you are talking to a Kubernetes apiserver and need
real-world throughput on top of port-forward. `kube::Api::portforward`
opens a fresh WebSocket per stream pair and gives you no PING/PONG. This
crate keeps one upgrade alive and lets you fan dozens of concurrent local
TCP connections through it.

If your only need is a single short-lived port-forward, `kube::Api::portforward`
is enough.

## Quick start

The crate consumes an already-upgraded transport. You bring the HTTP
upgrade; `spdy-mux` takes it from there:

```rust,no_run
use spdy_mux::{MuxConfig, Session, split_raw_spdy};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

# async fn run<S>(upgraded: S, target_port: u16) -> Result<(), Box<dyn std::error::Error>>
# where S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static {
let (writer, reader) = split_raw_spdy(upgraded);
let cancel = CancellationToken::new();

let session = Session::with_config(
    vec![(writer, reader)],
    target_port,
    cancel,
    MuxConfig::default(),
)
.await?;

let mut stream = session.connect().await?;
stream.write_all(b"GET / HTTP/1.0\r\n\r\n").await?;
# Ok(()) }
```

To run the pool with several parallel upgraded connections, pass them
all to `with_config`:

```rust,ignore
let pairs: Vec<_> = upgrades.into_iter().map(split_raw_spdy).collect();
let session = Session::with_config(pairs, port, cancel, MuxConfig::default()).await?;
```

Streams open round-robin across the pool with P2C load balancing on top.
A dead pool member self-evicts; the session keeps serving from the
survivors.

## Architecture

```
┌──────────────────────────────────────────────────────────┐
│  Session::connect()  ── opens a SPDY stream              │
└──────────────────────────────────────────────────────────┘
                            │
                            ▼
┌──────────────────────────────────────────────────────────┐
│  Pool: N MuxHandles, P2C routing on (inflight × RTT)     │
└──────────────────────────────────────────────────────────┘
                            │
            ┌───────────────┴───────────────┐
            ▼                               ▼
┌────────────────────────┐      ┌────────────────────────┐
│  MuxHandle             │ ...  │  MuxHandle             │
│  ┌──────────────────┐  │      │                        │
│  │ reader task      │  │      │                        │
│  │ 5 worker tasks   │  │      │                        │
│  │ writer task      │  │      │                        │
│  │ supervisor task  │  │      │                        │
│  └──────────────────┘  │      │                        │
└─────────┬──────────────┘      └────────────────────────┘
          │
          ▼
┌──────────────────────────────────────────────────────────┐
│  WsFrameReader / WsFrameWriter (transport adapter)       │
│  fastwebsockets   OR   raw SPDY-over-TCP                 │
└──────────────────────────────────────────────────────────┘
```

Each connection runs one reader, five frame workers partitioned by
`stream_id % 5`, one writer, and a supervisor. The reader decodes SPDY
frames from the wire and routes stream-keyed frames to workers; the
writer encodes and sends frames; the supervisor cancels the session
when any task exits unexpectedly.

## Transport contract

When the underlying socket closes or errors, **every** stream on that
pool member receives `BrokenPipe`. No transparent reconnection. The
layer above (`kube-portforward::Forwarder` in our case) handles
reconnection by opening a fresh session.

## Examples

See `examples/` for compile-checked setup demos:

- `websocket_transport.rs`: SPDY over a WebSocket upgrade
- `raw_transport.rs`: SPDY over a raw HTTP/1.1 upgrade

Neither runs standalone; both want a real upgraded transport from your
HTTP client. Copy the shape, swap the duplex placeholder for your
`hyper::upgrade::Upgraded`.

## License

GPL-3.0, matching the rest of the kftray project.

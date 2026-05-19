# spdy-mux

SPDY/3.1 stream multiplexer over async transports.

The crate gives you a SPDY/3.1 codec, a connection pool with P2C
load-balancing, flow control, an idle ping watchdog, and two transport
adapters for the wire. It does not know what protocol your peer speaks
on top of SPDY: you choose the SYN_STREAM headers, the codec sends them
verbatim.

kftray uses this to multiplex Kubernetes port-forward streams through
the apiserver. Any SPDY/3.1 peer that uses paired data/error streams
works the same way; the K8s headers (`streamtype`, `port`, `requestid`)
are built by the caller, not the multiplexer.

## What you get

- SPDY/3.1 frame codec (SYN_STREAM, SYN_REPLY, DATA, RST_STREAM, PING,
  SETTINGS, GOAWAY, WINDOW_UPDATE) with the standard zlib header
  compression dictionary
- Connection pool with power-of-two-choices routing and peak-EWMA RTT
  tracking, so streams flow through the least-loaded socket
- Flow control with per-stream and session-level send windows
- Idle ping watchdog that tears down the session if the peer goes silent
- Two ready-made transport adapters:
  - `FastWsReader` / `FastWsWriter`: SPDY frames carried inside WebSocket
    binary messages
  - `RawSpdyReader` / `RawSpdyWriter`: SPDY frames written directly to an
    upgraded HTTP/1.1 connection

Plug in your own transport by implementing the `WsFrameReader` and
`WsFrameWriter` traits.

## Stream shape

Each `Session::open_stream_pair` returns a `Stream` that is two SPDY
streams glued together: a writable "data" stream plus an "error" stream
the multiplexer half-closes at open time (empty `DATA + FIN` right after
the two SYN_STREAMs hit the wire). The peer keeps the error stream's
read direction open so it can deliver out-of-band error messages.

The pair shape exists because every paired-stream SPDY peer I know of
uses it (Kubernetes port-forward, the old SPDY sub-resource RPCs). If
your peer wants single streams, this crate is not for you.

## Quick start

The crate consumes an already-upgraded transport. You bring the HTTP
upgrade; `spdy-mux` takes it from there.

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

For a parallel pool of connections, pass them all to `with_config`:

```rust,ignore
let pairs: Vec<_> = upgrades.into_iter().map(split_raw_spdy).collect();
let session = Session::with_config(pairs, cancel, MuxConfig::default()).await?;
```

Streams open via P2C across the pool. Dead pool members self-evict and
the session keeps serving from the survivors.

## Architecture

```
┌──────────────────────────────────────────────────────────┐
│  Session::open_stream_pair(error_headers, data_headers)  │
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

When the underlying socket closes or errors, every stream on that pool
member receives `BrokenPipe`. The session does not reconnect. The layer
above (a forwarder, typically) opens a fresh session on transport
failure.

## Lazy open

`open_stream_pair` reserves a slot and returns immediately, but the
SYN_STREAM frames stay buffered until the consumer writes its first
byte. Peers that dial an upstream connection eagerly on SYN_STREAM
(Kubernetes kubelet does this) would otherwise close the idle upstream
before the consumer can use it. Lazy open emits SYN_STREAM and the
first DATA frame as one atomic batch.

## Examples

`examples/` has two compile-checked setup demos:

- `websocket_transport.rs`: SPDY over a WebSocket upgrade
- `raw_transport.rs`: SPDY over a raw HTTP/1.1 upgrade

Neither runs standalone; both want a real upgraded transport from your
HTTP client. Copy the shape, swap the duplex placeholder for your
`hyper::upgrade::Upgraded`.

## License

GPL-3.0, matching the rest of the kftray project.

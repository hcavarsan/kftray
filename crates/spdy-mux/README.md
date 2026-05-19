# spdy-mux

spdy/3.1 stream multiplexer in rust.

## why

i needed spdy/3.1 in rust and the ecosystem doesn't have it. mio-spdy hasn't been touched since 2016. one project on github (`rusternetes`) has a `spdy.rs` module but actually implements the byte-channel framing that runs *inside* a spdy stream, not the protocol itself. another (`containers/conmon-rs`) has a literal `todo!()` in the port-forward path with a comment that reads "Requires SPDY protocol implementation from https://github.com/moby/spdystream".

so this is the missing piece. zlib header compression with the standard spdy dictionary, the full frame set (syn_stream, syn_reply, data, rst_stream, ping, settings, goaway, window_update), per-stream and session-level flow control, and a small pool of parallel transports with power-of-two-choices load balancing.

it knows nothing about kubernetes. headers are caller-supplied, the codec sends them verbatim.

## who needs it

mostly people doing kubernetes port-forward, or talking to a CRI runtime like containerd or CRI-O. spdy is dead in browsers, dead in the proxy ecosystem, and kubernetes itself is migrating off it (KEP-4006 puts websocket-tunneled streaming beta since 1.31, kubelet leg beta in 1.36).

if you're writing a new spdy peer in 2026 you're probably making a mistake, use h2 or quinn instead. but the kubernetes migration is going to take years, and most clusters in production still expect raw spdy upgrades, so the protocol isn't gone yet.

## the shape

every spdy/3.1 peer i've seen uses the same pattern: open two streams together, one for data and one for errors, with the error stream half-closed at open time. the api enforces this. you call `open_stream_pair(error_headers, data_headers)` and get back a `Stream` that's the two glued together.

if your peer wants single streams or a different pair convention, this crate won't fit. the codec layer (`codec.rs`, `dictionary.rs`, `transport.rs`) is pure spdy/3.1 and you could build a different session type on top of it, but the multiplexer api is opinionated.

## tradeoffs

a few things to know up front.

no community. nobody else is fuzz-testing rust spdy code. that's me, alone, with whatever tests i write. moby/spdystream in go still gets security fixes in 2026 (header accounting, frame length enforcement), and those bugs exist in any spdy/3.1 implementation. i fix them by reading their commits.

lazy open is built in. SYN_STREAM doesn't go on the wire until the consumer writes its first byte. this is because some peers (the kubelet, for sure) dial the upstream eagerly when they see SYN_STREAM, and a fast-closing target server will close the idle TCP before the consumer ever uses it. if your peer doesn't do this, lazy open is harmless but not useful.

zlib header compression has CRIME-style attack surface. spdy/3.1 was the original target of CVE-2012-4930. inside a kubernetes apiserver connection over TLS this is low risk, but it's a real attack class you carry with you.

## quick start

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

// you pick the headers. the codec sends them as-is.
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

for a pool of parallel transports, pass them all to `with_config`:

```rust,ignore
let pairs: Vec<_> = upgrades.into_iter().map(split_raw_spdy).collect();
let session = Session::with_config(pairs, cancel, MuxConfig::default()).await?;
```

dead pool members self-evict and the session keeps serving from whoever's left.

## architecture

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
fastwebsockets, or raw async-read/async-write
```

each connection runs one reader, five frame workers (partitioned by `stream_id % 5`), one writer, and a supervisor that cancels the session if any task exits unexpectedly. the codec is shared.

## the longer story

the reason this exists is the same reason `kube-portforward` exists, one layer up. i was trying to make a port-forward desktop tool that didn't fall over under wrk and vegeta loads, the fix was multiplexing, and there wasn't anything in the rust ecosystem to multiplex with.

reading moby/spdystream and tracing kubectl wire bytes was the only way to get to a working implementation. maybe 70% of the code was straightforward: frame layout, zlib codec, mux dispatch. the other 30% was wire-order quirks the spec doesn't document but the kubelet enforces. the `writer.rs` comments call those out where i found them.

if you're considering writing a spdy codec for the experience, don't. read this one instead. but if you have a real reason and the ecosystem doesn't have what you need, it's not that bad. a couple of weekends if you read other implementations carefully, longer if you don't.

## examples

`examples/raw_transport.rs` and `examples/websocket_transport.rs` show the setup shape. neither runs standalone, both need a real upgraded transport from your http client. they compile and the duplex placeholder makes the api surface obvious.

## license

GPL-3.0.

# kube-portforward

Kubernetes port-forward over a multiplexed SPDY/3.1 connection, with the legacy upgrade as a fallback.

## Why

A naive port-forward against a real cluster from a desktop client opens one TCP upgrade per local connection. Every new client triggers a fresh TLS handshake to the API server. At 30 concurrent connections you spend half your time in handshakes.

This crate opens one upgrade per session, or a small pool, and multiplexes every local TCP connection through it. The primary wire format is `SPDY/3.1+portforward.k8s.io`, the WebSocket-tunneled SPDY from KEP-4006 (Beta since Kubernetes 1.31). Against older clusters that reject the subprotocol, the dialer falls back to the original `Upgrade: SPDY/3.1` wire format kubectl has used since the beginning.

Both paths carry the same SPDY frames. The difference is the envelope.

## What's in it

A small pool of upgrades per session instead of one upgrade per stream. A PING watchdog that catches API server idle timeouts before they silently kill the connection. Automatic fallback to the legacy `Upgrade: SPDY/3.1` path for clusters that reject the modern subprotocol. Typed errors. A forwarder layer with pod watching, graceful drain, and recovery callbacks.

The streams implement `tokio::io::AsyncRead + AsyncWrite`, so `tokio::io::copy_bidirectional` works as the local TCP relay loop without any glue code.

## Tradeoffs

SPDY is on its way out. The whole stack sits on SPDY/3.1, which Kubernetes is migrating off (KEP-4006). WebSocket-to-kubelet went Beta in 1.36 and is on track for GA. When that completes, the codec underneath becomes redundant. The WebSocket transport, the pool, the keepalive, and the fallback dialer survive. The codec does not. Plan accordingly.

Only kubectl-shaped peers work. The wire pattern is the kubectl one: `SYN_STREAM(error)`, `SYN_STREAM(data)`, `DATA+FIN(error)`, in that order. It matches what the kubelet expects. Pointing this at a non-kubelet peer that interprets stream pairs differently will not work.

This is a streaming layer, not a Kubernetes client. You bring your own Kubernetes client to handle auth and kubeconfig, then pass in the resulting cluster URL.

## The story

I was building a desktop port-forward tool. Under wrk and vegeta load it fell over, and I wanted to find out why.

For one or two concurrent connections per pod, anything works. At 30+ concurrent connections, the cost of a fresh TLS handshake on every new local TCP client dominates everything else. The structural fix is to stop opening one upgrade per stream. SPDY/3.1 was designed for exactly this kind of multiplexing.

Implementing it took longer than I expected. Zlib header compression with the standard SPDY dictionary is annoying to debug; mostly wire traces and hex dumps. The hardest bug was a wire-order quirk: the kubelet only takes the success path if you emit `SYN_STREAM(error)`, then `SYN_STREAM(data)`, then an empty `DATA+FIN` on the error stream, in that exact order. Set `fin=true` on the SYN_STREAM directly and the kubelet rejects the forwarding with an error message written back on the error stream. Matching kubectl's exact pattern was the difference between "works" and "mysteriously fails halfway through the first request".

This is not the right shape forever. Once Kubernetes finishes the migration to WebSockets through the entire path, half of this code becomes redundant. The plan is to port the multiplexer and pool on top of a pure WebSocket transport and delete the codec.

## Quick start

```rust,no_run
use kube_portforward::Client;
use tokio::io::AsyncWriteExt;

# async fn run(kube: kube::Client, url: http::Uri) -> Result<(), Box<dyn std::error::Error>> {
let client = Client::new(kube, url);
let session = client.session("default", "nginx", 80).open().await?;
let mut stream = session.connect().await?;
stream.write_all(b"GET / HTTP/1.0\r\n\r\n").await?;
session.close().await?;
# Ok(()) }
```

## What's underneath

The multiplexer lives in a separate crate, `spdy-mux`. It does not know anything about Kubernetes. This crate adds the Kubernetes part: upgrade negotiation, header construction, fallback, and the forwarder layer.

## Examples

See `examples/` for end-to-end usage. You will need a real cluster to run them.

## License

GPL-3.0.

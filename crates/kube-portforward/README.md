# kube-portforward

Kubernetes port-forward over SPDY/3.1 with kubectl-style fallback.

Speaks `SPDY/3.1+portforward.k8s.io` (SPDY frames tunnelled inside WebSocket
binary messages, per [KEP-4006](https://github.com/kubernetes/enhancements/issues/4006))
by default. When an apiserver rejects the WebSocket subprotocol — older
clusters, `PortForwardWebsockets` disabled, fronted by a proxy that doesn't
understand the subprotocol — the dialer transparently falls back to the
legacy raw `Upgrade: SPDY/3.1` path (the original `kubectl port-forward`
wire protocol).

Both paths deliver the same SPDY/3.1 frames to the apiserver. The multiplexer
(`spdy-mux`) opens a small pool of upgraded connections per session and
distributes streams across them via power-of-two-choices with peak-EWMA
load estimation.

## Why not `kube::Api::portforward`?

| Feature                              | `kube::Api::portforward`  | `kube-portforward`   |
|--------------------------------------|---------------------------|----------------------|
| Stream multiplexing                  | No (one upgrade per pair) | Yes (single pool)    |
| Keepalive Ping + watchdog            | No                        | Yes                  |
| Graceful shutdown drain              | No                        | Yes                  |
| Legacy SPDY fallback                 | No                        | Yes (automatic)      |
| Structured `thiserror` errors        | No                        | Yes                  |
| Builder-pattern config               | No                        | Yes                  |
| Pluggable recovery callback          | No                        | Yes                  |

## Fallback flow

1. Send `GET .../portforward` with `Sec-WebSocket-Protocol: SPDY/3.1+portforward.k8s.io`.
2. On `101 Switching Protocols` with the matching subprotocol echo, wrap the
   upgraded connection in a WebSocket and feed SPDY frames in/out of binary
   messages.
3. On any non-network failure (HTTP 4xx/5xx, missing or mismatched
   subprotocol header), retry with `POST .../portforward` carrying
   `Connection: Upgrade`, `Upgrade: SPDY/3.1`, and
   `X-Stream-Protocol-Version: portforward.k8s.io`. Write SPDY frames
   directly to the upgraded connection — no WebSocket envelope.

Real transport errors (TLS, DNS, connection refused) propagate without
fallback so callers see the actual reason.

## Quick Start

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

See `examples/` for end-to-end usage including pool sizing, custom keepalive
timings, and graceful shutdown.

## License

GPL-3.0, matching the rest of the kftray project.

# kube-portforward

Production-ready Kubernetes port-forward over WebSocket with channel multiplexing.

Speaks `v5.channel.k8s.io` (preferred) and `v4.channel.k8s.io` (fallback) per
[KEP-4006](https://github.com/kubernetes/enhancements/issues/4006). Multiplexes
N concurrent local TCP connections over ONE WebSocket upgrade by encoding the
target pod port N times in the URL.

## Why not `kube::Api::portforward`?

| Feature                                | `kube::Api::portforward` | `kube-portforward` |
|----------------------------------------|--------------------------|--------------------|
| WebSocket transport                    | Yes                      | Yes                |
| Channel multiplexing over one WS       | No (one upgrade per pair)| Yes (N pairs)      |
| `v5.channel.k8s.io` half-close support | Limited                  | Yes (ID reuse)     |
| Keepalive Ping + watchdog              | No                       | Yes (15s/30s)      |
| Graceful shutdown drain                | No                       | Yes (configurable) |
| Pre-1.30 detection with KEP-4006 hint  | Opaque `kube::Error`     | `ServerVersionTooOld` |
| Structured `thiserror` errors          | No                       | Yes                |
| Builder-pattern config                 | No                       | Yes                |
| Pluggable recovery callback            | No                       | Yes                |

## Option B Multiplexing

For each session, the URL encodes the target port N times:

```
?ports=9200&ports=9200&...&ports=9200
```

The apiserver and kubelet allocate 2N channel IDs at handshake (a data/error
pair per URL occurrence). Channel pair `(2i, 2i+1)` corresponds to URL
position `i`. The allocator hands pairs out in URL order — first call
returns `(0, 1)`, second `(2, 3)`, and so on.

On `v5`, the half-close signal `[0xFF, channel]` releases an ID pair back
to the free-list, letting one session sustain a high turnover of short-lived
local connections. On `v4`, released pairs stay reserved for the session
lifetime.

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

See `examples/` for end-to-end usage including multiplexing, custom keepalive
timings, and graceful shutdown.

## License

Dual-licensed under MIT or Apache-2.0, at your option.

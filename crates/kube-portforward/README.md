# kube-portforward

kubernetes port-forward over a multiplexed spdy/3.1 connection, with the legacy upgrade as a fallback.

## why

if you do port-forward against a real cluster from a desktop client, the naive pattern (one tcp/tls upgrade per local connection) hurts. every new local client triggers a fresh tls handshake to the apiserver. with 30 concurrent connections you're spending half your time in handshakes. users notice.

this crate opens one upgrade per session, or a small pool, and multiplexes every local tcp connection through it. behind the scenes it speaks `SPDY/3.1+portforward.k8s.io` (the websocket-tunneled spdy from KEP-4006, beta since kubernetes 1.31). against older clusters where that subprotocol gets rejected, it falls back to the original `Upgrade: SPDY/3.1` wire format kubectl has used forever.

both paths carry the same spdy frames. the difference is just the envelope.

## what's in it

a small pool of upgrades per session instead of one upgrade per stream. a ping watchdog that catches apiserver idle timeouts before they silently kill the connection (this one bit me for a long time before i added it). automatic fallback to the legacy `Upgrade: SPDY/3.1` path for clusters that reject the modern subprotocol. typed errors. a forwarder layer with pod watching, graceful drain, recovery callbacks.

the streams implement `tokio::io::AsyncRead + AsyncWrite`, so `tokio::io::copy_bidirectional` works as the local TCP relay loop without any glue code.

## tradeoffs

spdy is on its way out. the whole stack sits on spdy/3.1, which kubernetes is migrating off (KEP-4006). websocket-to-kubelet went beta in 1.36 and will probably go GA in a release or two. when that happens, the codec underneath turns into dead weight. the websocket transport, the pool, the keepalive, the fallback dialer all survive. the codec doesn't. plan accordingly.

only kubectl-shaped peers work. the wire pattern is the kubectl one: SYN_STREAM error first, then data, then empty DATA+FIN on the error stream. matches what the kubelet expects. point it at a non-kubelet peer that interprets stream pairs differently and it'll break.

it's a streaming layer, not a kube client. you bring your own kube client to handle auth and kubeconfig, and pass in the resulting cluster URL.

## the story

i build a desktop port-forward tool. wrk and vegeta were destroying it under load and i wanted to figure out why.

for one or two concurrent connections per pod anything works fine. the moment you get to 30+, the cost of a fresh tls handshake on every new local tcp client dominates everything else. that's the structural fix, you stop opening one upgrade per stream. spdy/3.1 was designed for exactly this kind of multiplexing back when browsers used it.

implementing it took longer than i expected. zlib header compression with the standard spdy dictionary is annoying to debug, mostly wire traces and hex dumps. the worst bug was a wire-order quirk: the kubelet only takes the success path if you emit SYN_STREAM(error), then SYN_STREAM(data), then an empty DATA+FIN on the error stream, in that exact order. set fin=true on the SYN_STREAM directly and the kubelet rejects the forwarding with an error message written back on the error stream. matching kubectl's exact pattern was the difference between "works" and "mysteriously fails halfway through the first request". i lost most of a weekend to that one.

i'm not under any illusion this is the right shape forever. once kubernetes finishes the migration to websockets-all-the-way-down, half of this code becomes a tribute act. the plan is to port the multiplexer and pool on top of a pure websocket transport and delete the codec. soon, but not yet.

## quick start

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

## what's underneath

the multiplexer lives in a separate crate, `spdy-mux`. it doesn't know anything about kubernetes. this crate adds the kubernetes part: upgrade negotiation, header construction, fallback, the forwarder layer.

## examples

see `examples/` for end-to-end usage. you'll need a real cluster to run them.

## license

GPL-3.0.

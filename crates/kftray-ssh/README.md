# kftray-ssh

SSH port-forwarding engine behind kftray's `ssh-port-forward` workload. A
local connection becomes a `direct-tcpip` channel inside one SSH session.
Routes to the same bastion share that session. The crate is pure Rust
([russh](https://crates.io/crates/russh)) and needs no `ssh` or `gcloud`
binary at runtime.

## Layers

```text
SshRuntime ── SshPool ── one session per RouteKey (supervised, reconnects)
                 │
SshRoute { Transport, Authenticator, HostKeySource }   ← built by a provider
                 │
providers/gcp ── IapTransport (transport/iap) + OS Login + guest-attribute host keys
```

- **`SshRoute`**: how to reach and log in to one sshd. A provider builds it
  from three pieces:
  - `Transport`: the byte stream to sshd, for example an IAP WebSocket tunnel.
  - `Authenticator`: the user name and either kftray's key or a certificate.
    It is called for every new session.
  - `HostKeySource`: the host keys the provider vouches for. An empty list
    means trust on first use, stored through the caller's `HostKeyStore`.
- **`SshRuntime`**: holds kftray's ed25519 key (`<config dir>/ssh/id_ed25519`)
  and the session pool.
  - `acquire` returns an `SshLease` that keeps the session alive.
    `SshLease::opener(host, port)` opens one channel per local connection.
  - `probe` runs every stage on a fresh session for the connection test.
- **Sessions**:
  - Keepalives run every 30 s.
  - A dropped session reconnects with a 1–30 s backoff and fresh credentials.
    An error that is not retryable moves the session to `Failed`.
  - The session closes when the last lease drops.
- **IAP transport**: a resumable tunnel that follows gcloud's protocol
  (`/v4/connect`, `/v4/reconnect`, ACKs, close codes). Its frame decoder is
  fuzzed by `fuzz/fuzz_targets/fuzz_iap_frame_decode.rs`.

Feature flags:

| Flag | Enables |
|---|---|
| `iap` | The IAP transport. |
| `provider-gcp` | The `gcp-bastion` provider. It is on by default. |
| `testing` | `MockSshd`, `DuplexTransport`, `MemoryHostKeyStore`, `route_conformance`, `FakeIap` and `FakeGcp`. |
| `e2e` | Lets kftray point the GCP provider at fakes, through `KFTRAY_GCP_ENDPOINTS` and `KFTRAY_GCP_STATIC_TOKEN`. |

The crate does not depend on `kftray-commons` and enables no `aws-lc` feature.
TLS uses rustls with the `ring` provider and the platform's root certificates.

## Adding a provider

| # | Where | What |
|---|---|---|
| 1 | `kftray-commons/src/models/ssh_provider.rs` | Add a variant to `SshProviderKind`. Add its arms to `type_name`, `label`, `validate` and `summary`, and its name to `TYPE_NAMES`. |
| 2 | `kftray-ssh/src/transport/`, or the provider module | Add any `Transport`, `Authenticator` or `HostKeySource` that does not exist yet. Reuse the existing ones when they fit. |
| 3 | `kftray-ssh/src/providers/<name>/` | Add the route builder and an error enum that implements `ProviderError`. Prefix its `RouteKey` and host-key alias with `<name>:`. |
| 4 | `kftray-ssh/Cargo.toml` | Add a `provider-<name>` feature and list it in `default`. |
| 5 | `kftray-portforward/src/ssh_forward/providers.rs` | Add the `build_route` arm. This is the only `match` on the provider type in kftray. |
| 6 | `kftray-ssh/tests/` | Add a test that runs `route_conformance` with a builder for the new route. |
| 7 | `frontend/src/components/AddConfigModal/ssh/providers/` | Add an entry to `SSH_PROVIDERS`: the form, the validation (same messages as Rust) and the summary. |
| 8 | `kftray-mcp/src/tools/config.rs` | Add the provider's JSON schema to `ssh_provider_schemas()`. |

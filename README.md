
<div align="center">  <br>
  <img src="https://raw.githubusercontent.com/hcavarsan/kftray-blog/main/img/logo.png" width="128px" alt="kftray Logo" />
  <br><br>
  <a href="https://kftray.app"><strong>Website</strong></a> |  <a href="https://kftray.app/downloads"><strong>Downloads</strong></a> |  <a href="https://kftray.app/blog"><strong>Blog</strong></a>
  <br><br>
  <a href="https://join.slack.com/t/kftray/shared_invite/zt-2q6lwn15f-Y8Mi_4NlenH9TuEDMjxPUA">
    <img src="https://img.shields.io/badge/Slack-Join%20our%20Slack-blue?style=for-the-badge&logo=slack" alt="Join Slack">
  </a>
  <a href="https://github.com/hcavarsan/kftray/releases">
    <img src="https://img.shields.io/github/v/release/hcavarsan/kftray?style=for-the-badge" alt="Latest Release">
  </a>
  <a href="https://github.com/hcavarsan/kftray">
    <img src="https://img.shields.io/github/downloads/hcavarsan/kftray/total?style=for-the-badge" alt="Total Downloads">
  </a>
  <a href="https://codecov.io/gh/hcavarsan/kftray">
    <img src="https://img.shields.io/codecov/c/github/hcavarsan/kftray?style=for-the-badge&logo=codecov" alt="Codecov Coverage">
  </a>
  <a href="https://crates.io/crates/kftui">
    <img src="https://img.shields.io/crates/v/kftui?style=for-the-badge&logo=rust" alt="Crates.io">
  </a>
  <br><br>
</div>

<p align="center">
<div align="center">
<img src="https://raw.githubusercontent.com/hcavarsan/kftray-blog/refs/heads/main/public/img/kftools.webp" alt="Kftray github"/>
</div>
</p>


## About

kftray and kftui are Kubernetes port forwarding tools that actually work the way you'd expect them to. While `kubectl port-forward` is fine for quick tasks, it falls apart when pods restart or connections drop – and you're stuck manually reconnecting.

Both kftray (desktop app with tray integration) and kftui (terminal UI) share the same Rust backend and configuration files. They use the Kubernetes watch API to detect when pods come and go, automatically reconnecting your forwards without you having to babysit them. They handle TCP and UDP through a proxy relay in your cluster, support multiple forwards at once, and can even log HTTP traffic for debugging.

To download apps, you can check the [download page](https://kftray.app/downloads) on the kftray website.

### Why Another Port Forwarding Tool?

There are plenty of Kubernetes tools out there, but port forwarding has always been weirdly neglected. The main issues with `kubectl port-forward`:

- **Connections break** when pods restart or get rescheduled
- **No automatic reconnection** – you have to manually restart everything
- **Multiple forwards** means multiple terminal windows
- **No UDP support** out of the box
- **No way to debug HTTP traffic** flowing through the tunnel

The tools monitor pod lifecycle events and automatically reconnect to healthy pods when things go sideways. You can manage dozens of forwards from a single interface, forward UDP traffic through a proxy relay, and inspect HTTP requests/responses when you need to debug.

check out our blog post at [kftray.app/blog/posts/13-kftray-manage-all-k8s-port-forward](https://kftray.app/blog/posts/13-kftray-manage-all-k8s-port-forward).

<br>

<div align="center">
  <table>
    <tr>
      <td>
        <a href="https://youtu.be/3pIDGB6Tx_o">
          <img src="https://img.youtube.com/vi/3pIDGB6Tx_o/maxresdefault.jpg" alt="Watch the video" width="800px">
        </a>
      </td>
      <td>
        <a href="https://www.youtube.com/watch?v=Zvv9gIhLaSM">
          <img src="https://img.youtube.com/vi/Zvv9gIhLaSM/maxresdefault.jpg" alt="Watch the video" width="800px">
        </a>
      </td>
    </tr>
  </table>
</div>

<br>




## Features Matrix


<div align="center">

| Feature | kftray (Desktop) | kftui (Terminal) |
|---------|------------------|------------------|
| **Auto-reconnection** – Reconnects when pods restart | ✅ | ✅ |
| **Multiple forwards** – Start/stop many at once | ✅ | ✅ |
| **No kubectl needed** – Direct K8s API integration | ✅ | ✅ |
| **TCP/UDP support** – Via cluster proxy relay | ✅ | ✅ |
| **HTTP traffic logs** – Inspect requests/responses | ✅ | ✅ |
| **Pod health tracking** – Shows which pod you're connected to | ✅ | ✅ |
| **Network recovery** – Auto-reconnects after sleep/disconnect | ✅ | ✅ |
| **GitHub sync** – Share configs with your team | ✅ | ✅ |
| **Auto-import** – Discover services via K8s annotations | ✅ | ✅ |
| **Custom kubeconfig** – Use any kubeconfig path | ✅ | ✅ |
| **Port-forward timeouts** – Auto-close after time limit | ✅ | ✅ |
| **Hosts file management** – Auto-update /etc/hosts entries | ✅ | ✅ |
| **Auto SSL** – Automatic SSL certificate generation for port forwards | ✅ | ✅ |
| **Expose local services** – Reverse tunnel local apps to cluster/internet (like ngrok) | ✅ | ✅ |
| **System tray integration** – Quick access from tray | ✅ | ❌ |
| **Request replay** – Replay HTTP requests for debugging | ❌ | ✅ |

<sub>Notes: (1) Hosts file updates may require admin privileges and vary by OS. (2) HTTP logs/replay can expose sensitive data—opt-in and sanitize where needed.</sub>

</div>

## kftray - Desktop App

The desktop app runs in your system tray and provides a GUI for managing port forwards.

- [Installation](https://github.com/hcavarsan/kftray/tree/main/docs/kftray/INSTALL.md) – Download and install
- [Usage Guide](https://github.com/hcavarsan/kftray/tree/main/docs/kftray/USAGE.md) – How to use kftray
- [Building from Source](https://github.com/hcavarsan/kftray/tree/main/docs/kftray/BUILD.md) – Build it yourself

## kftui - Terminal UI

The terminal interface for those who prefer staying in the console.

- [Installation](https://github.com/hcavarsan/kftray/tree/main/docs/kftui/INSTALL.md) – Install via Homebrew, Cargo, or download
- [Usage Guide](https://github.com/hcavarsan/kftray/tree/main/docs/kftui/USAGE.md) – Terminal shortcuts and features
- [Building from Source](https://github.com/hcavarsan/kftray/tree/main/docs/kftui/BUILD.md) – Build instructions

## kftray-server - Proxy Relay

The proxy relay that runs in your cluster to handle TCP/UDP forwarding.

- [Architecture Docs](https://github.com/hcavarsan/kftray/tree/main/docs/ARCH.md) – How it all works

## Configuration

Both tools share the same JSON configuration format. Here's a example:

```json
[
  {
    "alias": "argocd",
    "context": "kind-kftray-cluster",
    "kubeconfig": "/Users/henrique/.kube/kind-config-kftray-cluster",
    "local_port": 16080,
    "namespace": "argocd",
    "protocol": "tcp",
    "remote_port": 8080,
    "service": "argocd-server",
    "workload_type": "service",
    "http_logs_enabled": true
  }
]
```

You can import configs from:
- Local JSON files
- GitHub repositories (public or private)
- Direct from your cluster using service annotations
- Command line (kftui supports `--json` and `--stdin`)

### Workload Types

kftray supports multiple workload types for different use cases:

<img src="https://raw.githubusercontent.com/hcavarsan/homebrew-kftray/main/img/workload_types.png" alt="Workload Types" />

- **service** - Forward to a Kubernetes service (TCP/UDP)
- **pod** - Forward directly to pods using label selectors (TCP/UDP)
- **proxy** - Tunnel to external resources via the cluster (TCP/UDP)
- **expose** - Reverse tunnel your local services to the cluster or internet

### Expose: Reverse Tunneling

The **expose** workload type lets you share your local development server with your team or expose it to the internet through your Kubernetes cluster. This is useful for:

- Testing webhooks locally with external services
- Sharing work-in-progress features with teammates
- Running local services that need to be accessible from the cluster

**Example: Expose local service to the internet**
```json
{
  "alias": "myapp.example.com",
  "namespace": "production",
  "local_port": 3000,
  "local_address": "localhost",
  "context": "my-k8s-cluster",
  "workload_type": "expose",
  "protocol": "tcp",
  "domain_enabled": true,
  "exposure_type": "public",
  "cert_manager_enabled": true,
  "cert_issuer": "letsencrypt-prod",
  "cert_issuer_kind": "ClusterIssuer",
  "ingress_class": "nginx"
}
```

**Example: Expose to cluster internal network only**
```json
{
  "alias": "internal-api",
  "namespace": "development",
  "local_port": 8080,
  "local_address": "localhost",
  "context": "my-k8s-cluster",
  "workload_type": "expose",
  "protocol": "tcp",
  "domain_enabled": true,
  "exposure_type": "internal"
}
```

For more examples, see the [examples directory](./examples/).

## Under the hood

The tools use a shared Rust core that handles all the Kubernetes interaction. Here's the basic flow:

1. **Config Management** – Load port forward configs from files/GitHub/K8s annotations
2. **Pod Discovery** – Find target pods using label selectors or service definitions
3. **Connection Setup** – Establish websocket connection to K8s API
4. **Traffic Relay** – Forward traffic between local ports and pod ports
5. **Health Monitoring** – Watch for pod changes and reconnect as needed

For UDP or when you need to reach external services, we deploy a small relay pod in your cluster that handles the actual forwarding.

## Recent Updates

Check the [releases page](https://github.com/hcavarsan/kftray/releases) for the full changelog.

## Development

Want to contribute or build from source? We use [mise](https://mise.jdx.dev) to manage the development environment.

**Quick start:**
```bash
# Install mise
curl https://mise.run | sh

# Clone and setup
git clone https://github.com/hcavarsan/kftray.git
cd kftray
mise install        # Install all tools
mise run setup      # Setup dependencies
mise run dev        # Start development
```

**Available commands:**
- `mise run dev` - Start development mode
- `mise run build` - Build production app
- `mise run format` - Format code
- `mise run lint` - Lint with auto-fix
- `mise run test:back` - Run tests

See [DEVELOPMENT.md](DEVELOPMENT.md) for the complete development guide.

## Contributing

We're always looking for contributions. Whether it's bug fixes, new features, or just ideas, we'd love to hear from you.

- **Pull Requests** – Fork, code, and submit
- **Issues** – Report bugs or request features
- **Discussions** – Share ideas and feedback

Check out [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines

## License

kftray is available under the [GPL 3.0 License](LICENSE.md).

## Star History

<a href="https://star-history.com/#hcavarsan/kftray&Date">
 <picture>
   <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=hcavarsan/kftray&type=Date&theme=dark" />
   <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/svg?repos=hcavarsan/kftray&type=Date" />
   <img alt="Star History Chart" src="https://api.star-history.com/svg?repos=hcavarsan/kftray&type=Date" />
 </picture>
</a>

## Contributors

Thanks goes to these wonderful people ([emoji key](https://allcontributors.org/docs/en/emoji-key)):

<!-- ALL-CONTRIBUTORS-LIST:START - Do not remove or modify this section -->
<!-- prettier-ignore-start -->
<!-- markdownlint-disable -->
<table>
  <tbody>
    <tr>
      <td align="center" valign="top" width="14.28%"><a href="https://github.com/hcavarsan"><img src="https://avatars.githubusercontent.com/u/30353685?v=4?s=100" width="100px;" alt="Henrique Cavarsan"/><br /><sub><b>Henrique Cavarsan</b></sub></a><br /><a href="https://github.com/hcavarsan/kftray/commits?author=hcavarsan" title="Code">💻</a></td>
      <td align="center" valign="top" width="14.28%"><a href="http://fandujar.dev"><img src="https://avatars.githubusercontent.com/u/6901387?v=4?s=100" width="100px;" alt="Filipe Andujar"/><br /><sub><b>Filipe Andujar</b></sub></a><br /><a href="https://github.com/hcavarsan/kftray/commits?author=fandujar" title="Code">💻</a></td>
      <td align="center" valign="top" width="14.28%"><a href="https://speakerdeck.com/eltociear"><img src="https://avatars.githubusercontent.com/u/22633385?v=4?s=100" width="100px;" alt="Ikko Eltociear Ashimine"/><br /><sub><b>Ikko Eltociear Ashimine</b></sub></a><br /><a href="https://github.com/hcavarsan/kftray/commits?author=eltociear" title="Code">💻</a></td>
      <td align="center" valign="top" width="14.28%"><a href="https://github.com/honsunrise"><img src="https://avatars.githubusercontent.com/u/3882656?v=4?s=100" width="100px;" alt="Honsun Zhu"/><br /><sub><b>Honsun Zhu</b></sub></a><br /><a href="https://github.com/hcavarsan/kftray/commits?author=honsunrise" title="Code">💻</a></td>
      <td align="center" valign="top" width="14.28%"><a href="https://www.linkedin.com/in/peter-hansson-07939a231"><img src="https://avatars.githubusercontent.com/u/9850798?v=4?s=100" width="100px;" alt="Peter Hansson"/><br /><sub><b>Peter Hansson</b></sub></a><br /><a href="https://github.com/hcavarsan/kftray/commits?author=Lunkentuss" title="Code">💻</a></td>
      <td align="center" valign="top" width="14.28%"><a href="https://github.com/FabijanZulj"><img src="https://avatars.githubusercontent.com/u/38249221?v=4?s=100" width="100px;" alt="FabijanZulj"/><br /><sub><b>FabijanZulj</b></sub></a><br /><a href="https://github.com/hcavarsan/kftray/commits?author=FabijanZulj" title="Code">💻</a></td>
      <td align="center" valign="top" width="14.28%"><a href="https://github.com/skht"><img src="https://avatars.githubusercontent.com/u/1878554?v=4?s=100" width="100px;" alt="skht"/><br /><sub><b>skht</b></sub></a><br /><a href="https://github.com/hcavarsan/kftray/commits?author=skht" title="Code">💻</a></td>
    </tr>
  </tbody>
</table>

<!-- markdownlint-restore -->
<!-- prettier-ignore-end -->

<!-- ALL-CONTRIBUTORS-LIST:END -->

This project follows the [all-contributors](https://github.com/all-contributors/all-contributors) specification. Contributions of any kind welcome!

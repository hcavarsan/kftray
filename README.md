<div align="center">
  <img src="./logo.png" alt="KFtray Logo" width="200" />
  <h1>KFtray</h1>
  <p>
<h3>A cross-platform system tray app made with Tauri (Rust and TypeScript) for Kubernetes users. It simplifies setting up multiple `kubectl port forward` configurations through a user-friendly interface. Easily store and manage all configurations from local files or GitHub repositories.</h3>
  </p> 
  <br>

  <!-- Badges -->
  <a href="https://nodejs.org/en/">
    <img src="https://img.shields.io/badge/Node-v20.11.0-brightgreen.svg" alt="Node.js version" />
  </a>
  <a href="https://tauri.app/">
    <img src="https://img.shields.io/badge/Tauri-v1.5.8-brightgreen.svg" alt="Tauri version" />
  </a>
  <a href="https://react.dev">
    <img src="https://img.shields.io/badge/React-v18.2.0-brightgreen.svg" alt="React version" />
  </a>
  <a href="https://www.rust-lang.org/">
    <img src="https://img.shields.io/badge/Rust-v1.75.0-brightgreen.svg" alt="Rust version" />
  </a>

  <!-- Download Links and Stats -->
  <a href="https://github.com/hcavarsan/kftray/releases/latest">
    <img src="https://img.shields.io/github/downloads/hcavarsan/kftray/total.svg" alt="downloads" />
  </a>
  <a href="https://github.com/hcavarsan/kftray/actions">
    <img src="https://img.shields.io/github/actions/workflow/status/hcavarsan/kftray/main.yml" alt="release=">
  </a>
  </br>
  </br>
  <!-- Demo GIF -->
  <img src="https://raw.githubusercontent.com/hcavarsan/homebrew-kftray/main/img/demo.gif" alt="KFtray Demo" width="1500" />

  <!-- Download Buttons -->
  <p>
    <a href="https://github.com/hcavarsan/kftray/releases/latest/download/kftray_0.7.0_universal.dmg">
      Download for macOS
    </a> ·
    <a href="https://github.com/hcavarsan/kftray/releases/latest/download/kftray_0.7.0_x64-setup.exe">
      Download for Windows
    </a> ·
    <a href="https://github.com/hcavarsan/kftray/releases/latest/download/kftray_0.7.0_amd64.AppImage">
      Download for Linux
    </a>
  </p>
</div>

## Table of Contents

- [Features](#-features)
- [Installation](#-installation)
- [Usage](#-usage)
- [Architecture](#-architecture)
- [Contributing](#-contributing)
- [License](#-license)

---

## 🚀 Features

- **Resilient Port Forwarding Connection:** Ensures continuous service even if a pod dies, by reconnecting to another running pod automatically.
- **One-Click Multiple Port Forwards:** Allows for the setup of several port forwarding instances at the same time with a single click.
- **Independent of Kubectl:** Directly interfaces with the Kubernetes API, eliminating the need for `kubectl`.
- **Multi-Protocol Support:** Enables access to internal or external servers through a Proxy Relay server deployed in a Kubernetes cluster, including TCP and UDP port forwarding.
- **Import Configs from Git:** Store and import configurations directly from Git repositories with a few clicks.

## 📦 Installation

#### Homebrew on macOS and Linux

Install kftray with ease using Homebrew by tapping into the custom repository. Run the following commands:

For Linux:

```bash
brew tap hcavarsan/kftray
brew install --HEAD kftray
```

For macOS:

```bash
brew tap hcavarsan/kftray
brew install --HEAD kftray
```

_Please check the caveats section for global app creation instructions after installation._

#### Building from Source

##### Requirements

- Node.js and pnpm or yarn for building the frontend.
- Rust for building the backend.

To compile `kftray`, these steps should be followed:

1. Clone the repository:
   ```bash
   git clone https://github.com/hcavarsan/kftray.git
   ```
2. Navigate to the cloned directory:
   ```bash
   cd kftray
   ```
3. Install dependencies:
   ```bash
   pnpm install
   ```
4. Launch the application in development mode:
   ```bash
   pnpm run tauri dev
   ```

## 🧭 Usage

Below is an intuitive guide to getting started with KFtray.

### 🎛 Configure Port Forwards

Use the UI to add new port forward settings. Necessary details include:

- `Workload Type`: Proxy or Service.
- `Alias`: A unique name for the settings.
- `Context`, `Namespace`, `Service`: As per Kubernetes configuration.
- `Remote Address`: For Proxy type workload.
- `Protocol`: TCP or UDP.
- `Local and Remote Ports`: Endpoint details.

<details>
<summary><b>Create Service Configuration Screenshot</b></summary>
<p align="center">
<img src="img/createservice.png" alt="Create Service Configuration"/>
</p>
</details>

### ▶️ Activate Configurations

- **Single Configuration:** Click to initiate a single port forward.
- **All Configurations:** Start multiple port forward simultaneously.

<details>
<summary><b>Start Single Configuration Screenshot</b></summary>
<p align="center">
<img src="img/single.png" alt="Start Single Configuration"/>
</p>
</details>

### 🗂 Configuration Management

Manage and share port forward settings:

- **Export and Import**: Quickly transfer configurations using JSON files.
- **Git Synchronization**: Seamlessly fetch configurations from a Git repository.
- **Local Storage**: Securely save configurations at `$HOME/.kftray/configs.db`.
- **Server Pod Manifest**: Tailor the Proxy Relay server manifests stored at `$HOME/.kftray/proxy_manifest.json`.


#### Configuration JSON Sample

Below is an example of an exported JSON configuration:

```json
[
  {
    "alias": "consul-ui",
    "context": "kind-7",
    "local_port": 8500,
    "namespace": "consul",
    "protocol": "tcp",
    "remote_port": 8500,
    "service": "consul-ui",
    "workload_type": "service"
  },
  {
    "alias": "redis-gcp",
    "context": "kind-6",
    "local_port": 26379,
    "namespace": "default",
    "protocol": "udp",
    "remote_address": "redis-prod.gcp.internal",
    "remote_port": 6379,
    "workload_type": "proxy"
  }
]
```

## 🏗 Architecture

### Server

KFtray Server is a Rust application that relays UDP/TCP traffic to an upstream server. Check the source code [here](https://github.com/hcavarsan/kftray/tree/main/kftray-server).

### Forwarding Flows

- **TCP Forwarding:** A local TCP socket, similar to kubectl, can be used to communicate with a Kubernetes pod. This approach offers parallel execution and improved resilience.

```mermaid
sequenceDiagram
Application->>Kubernetes Pod: Opens TCP socket, starts port-forwarding
Kubernetes Pod-->>Application: Responds with TCP Packet
```

- **Proxy TCP Forwarding:** The local TCP connects to the kftray-server pod, which then sends TCP packet to the upstream server.

```mermaid
sequenceDiagram
Application->>Kubernetes Pod: Socket to kftray-server, facilitates TCP relay
Kubernetes Pod->>Remote Service: Relays TCP Packet
Remote Service-->>Kubernetes Pod: Responds
Kubernetes Pod-->>Application: Returns TCP Packet
```

- **UDP Forwarding:** The KFtray client opens a local UDP socket and connects a local TCP socket to the kftray-server pod. The TCP socket sends UDP packets over TCP, which are then forwarded to the upstream server.

```mermaid
sequenceDiagram
Application->>Kubernetes Pod: UDP socket, TCP port-forward to kftray-server
Kubernetes Pod->>Service/Remote: Converts to UDP, sends packet
Service/Remote-->>Kubernetes Pod: Responds with UDP Packet
Kubernetes Pod-->>Application: Relays as TCP
```

## 👥 Contributing

- 🛠 **Pull Requests**: Feel free to create pull requests for bug fixes, new features, or improvements.
- 📝 **Issues**: Report bugs, suggest new features, or ask questions.
- 💡 **Feedback**: Your feedback helps improve kftray.

## 📄 License

KFtray is available under the [MIT License](LICENSE.md), which is included in the repository. See the LICENSE file for full details.

## Stargazers over time

[![Stargazers over time](https://starchart.cc/hcavarsan/kftray.svg?variant=dark)](https://starchart.cc/hcavarsan/kftray)



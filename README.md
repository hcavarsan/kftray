<div align="center">

<img src="./img/logo.png" alt="KFtray Demo" width="300" />
  <p>
    <a href="https://github.com/hcavarsan/kftray/releases/latest/download/kftray_0.9.0_universal.dmg">
      Download for macOS
    </a> ·
    <a href="https://github.com/hcavarsan/kftray/releases/latest/download/kftray_0.9.0_x64-setup.exe">
      Download for Windows
    </a> ·
    <a href="https://github.com/hcavarsan/kftray/releases/latest/download/kftray_0.9.0_amd64.AppImage">
      Download for Linux
    </a>
  </p>


  <!-- Badges -->
  <a href="https://nodejs.org/en/">
    <img src="https://img.shields.io/badge/Node-v21.5.0-brightgreen.svg" alt="Node.js version" />
  </a>
  <a href="https://tauri.app/">
    <img src="https://img.shields.io/badge/Tauri-v1.6.1-brightgreen.svg" alt="Tauri version" />
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
    <a href="https://github.com/hcavarsan/kftray">
    <img src="https://img.shields.io/github/all-contributors/hcavarsan/kftray" alt="contributors" />
  </a>

  <br>

<br/>




</div>

## What Is Kftray?

KFtray is a cross-platform system tray app made with Tauri (Rust and TypeScript) for Kubernetes users. It simplifies setting up multiple kubectl port forward configurations through a user-friendly interface. Easily store and manage all configurations from local files or GitHub repositories.

**Blog Post**:
[Kubernetes Debugging: Handling Multiple kubectl port-forward from System Tray](https://kftray.hashnode.dev/kubernetes-debugging-handling-multiple-kubectl-port-forward-from-tray)

  <table align="center">
    <tr>
      <td align="center"><strong>Demo: GitHub Sync</strong></td>
      <td align="center"><strong>Demo: Adding a New Configuration</strong></td>
    </tr>
    <tr>
      <td align="center">
        <a href="https://www.youtube.com/watch?v=BAdL7IzaEh8">
          <img src="https://img.youtube.com/vi/BAdL7IzaEh8/0.jpg" alt="Kftray Demo: Github Sync" width="256"/>
        </a>
      </td>
      <td align="center">
        <a href="https://www.youtube.com/watch?v=nqEhmcKeCc4">
          <img src="https://img.youtube.com/vi/nqEhmcKeCc4/0.jpg" alt="Kftray Demo: Adding a new configuration" width="280"/>
        </a>
      </td>
    </tr>
  </table>


## Table of Contents


- [Features](#-features)
- [Installation](#-installation)
- [Usage](#-usage)
- [Architecture](#-architecture)
- [Contributing](#-contributing)
- [License](#-license)


## 🚀 Features

- **Resilient Port Forwarding Connection:** Ensures continuous service even if a pod dies, by reconnecting to another running pod automatically.
- **One-Click Multiple Port Forwards:** Allows for the setup of several port forwarding instances at the same time with a single click.
- **Independent of Kubectl:** Directly interfaces with the Kubernetes API, eliminating the need for `kubectl`.
- **Multi-Protocol Support:** Enables access to internal or external servers through a Proxy Relay server deployed in a Kubernetes cluster, including TCP and UDP port forwarding.
- **Import Configs from Git:** Store and import configurations directly from Git repositories with a few clicks.

## 📦 Installation

KFtray is available for macOS and Linux users via Homebrew, and directly from the GitHub releases page for other systems. Here's how you can get started:

**For macOS and Linux:**

```bash

brew tap hcavarsan/kftray

brew install --HEAD kftray
```

For other systems, visit the [GitHub releases page](https://github.com/hcavarsan/kftray/releases) for downloadable binaries.

_Please check the caveats section for global app creation instructions after installation._


## 🧭 Usage

## 🎛 Configuring Your First Port Forward

In a few simple steps, you can configure your first port forward:

1.  **Launch the application**
2.  **Open the configuration panel from the tray icon**
3.  **Add a new configuration:**

    *   Give it a unique alias and set if you want to set the alias as domain to your forward
    *    Indicate if the configuration is for a port forward for a service (common use) or a proxy (port forward to an endpoint via a Kubernetes cluster).
    *   Specify the Kubernetes context
    *   Define the namespace housing your service
    *   Enter the service name
    *   Choose TCP or UDP
    *   Set the local and remote port numbers
    *   Configure a custom local IP address (optional)

4. **Activate Your Configuration**: With your configuration saved, simply click on the switch button in the main menu to start the port forward in a single por forward or in Start All to start all configurations at the same time .



## Export configurations to a JSON file

1.  Open the main menu in the footer
2.  Select the `Export Local File` option
3.  Choose a file name and location to save the JSON file
4.  The JSON file will contain all your current configurations

You can then import this JSON file at any time to restore your configurations.

Example Json configuration File:

```json
[
 {
  "service": "argocd-server",
  "namespace": "argocd",
  "local_port": 8888,
  "remote_port": 8080,
  "context": "test-cluster",
  "workload_type": "service",
  "protocol": "tcp",
  "remote_address": "",
  "local_address": "127.0.0.1",
  "alias": "argocd",
  "domain_enabled": true
 }
]
```



## Sharing the configurations through Git

now, with the local json saved, you can share your configurations with your team members by committing the JSON file to a Github repository. This allows for easy collaboration and synchronization of KFtray configurations across your team.

To import and sync your github configs in kftray:

1.  Open the application's main menu
2.  Select the button with github icon in the footer menu
4.  Enter the URL of your Git repository and path containing the JSON file
5.  If your GitHub repository is private, you will need to enter the private token. Credentials are securely saved in the SO keyring (Keychain on macOS). Kftray does not store or save credentials in any local file; they are only stored in the local keyring.
6.  Select the polling time for when Kftray will synchronize configurations and retrieve them from GitHub.

7.  KFtray will now sync with the Git repository to automatically import any new configurations or changes committed to the JSON file.

This allows you to quickly deploy any port forward changes to all team members. And if someone on your team adds a new configuration, it will be automatically synced to everyone else's KFtray.



## Building from Source

#### Requirements

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




## Contributors ✨

Thanks goes to these wonderful people ([emoji key](https://allcontributors.org/docs/en/emoji-key)):

<!-- ALL-CONTRIBUTORS-LIST:START - Do not remove or modify this section -->
<!-- prettier-ignore-start -->
<!-- markdownlint-disable -->
<table>
  <tbody>
    <tr>
      <td align="center" valign="top" width="14.28%"><a href="https://github.com/hcavarsan"><img src="https://avatars.githubusercontent.com/u/30353685?v=4?s=100" width="100px;" alt="Henrique Cavarsan"/><br /><sub><b>Henrique Cavarsan</b></sub></a><br /><a href="https://github.com/hcavarsan/kftray/commits?author=hcavarsan" title="Code">💻</a></td>
      <td align="center" valign="top" width="14.28%"><a href="http://fandujar.dev"><img src="https://avatars.githubusercontent.com/u/6901387?v=4?s=100" width="100px;" alt="Filipe Andujar"/><br /><sub><b>Filipe Andujar</b></sub></a><br /><a href="https://github.com/hcavarsan/kftray/commits?author=fandujar" title="Code">💻</a></td>
    </tr>
  </tbody>
</table>

<!-- markdownlint-restore -->
<!-- prettier-ignore-end -->

<!-- ALL-CONTRIBUTORS-LIST:END -->

This project follows the [all-contributors](https://github.com/all-contributors/all-contributors) specification. Contributions of any kind welcome!

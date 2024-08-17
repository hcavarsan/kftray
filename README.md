<div align="center">
   <br />
   <img align="center" width="128px" src="https://raw.githubusercontent.com/hcavarsan/kftray-blog/main/img/logo.png" />
   <h1 align="center"><b>kftray</b></h1>
   <p align="center">
      KFtray and KFtui are independent, cross-platform applications. They help you set up and manage multiple port-forwarding settings easily. Both apps are part of the same open-source project and aim to make working with Kubernetes easier. KFtray has a desktop interface, while KFtui has a terminal interface, so you can choose the one that suits you best.
   </p>
   <p align="center">
      <a href="https://kftray.app"><strong>Visit kftray.app »</strong></a>
   </p>
</div>


## Desktop App

KFtray is a system tray application built with Tauri (Rust and TypeScript), offering a user-friendly interface to manage configurations from local files or GitHub repositories.

<br>

<div align="center">
   <img src="https://raw.githubusercontent.com/hcavarsan/kftray-blog/main/img/ss3.png" alt="kftray" width="500px" />
</div>

## TUI

KFtui is a terminal user interface (TUI) application made with Rust using the Ratatui framework, providing an intuitive terminal interface to manage configurations from local JSON files (GitHub repository support coming soon).

<br>

<div align="center">
   <img src="https://raw.githubusercontent.com/hcavarsan/kftray-blog/main/img/ss3.png" alt="kftray" width="500px" />
</div>

<br>

## Features

- **Resilient Port Forwarding Connection:** Ensures continuous service even if a pod dies by reconnecting to another running pod automatically.
- **One-Click Multiple Port Forwards:** Allows for the setup of several port forwarding instances simultaneously with a single click.
- **Independent of Kubectl:** Directly interfaces with the Kubernetes API, eliminating the need for `kubectl`.
- **Multi-Protocol Support:** Enables access to internal or external servers through a Proxy Relay server deployed in a Kubernetes cluster, including TCP and UDP port forwarding.
- **HTTP Logs Tracing:** Enable or disable HTTP logs for specific configurations to save the requests and responses in a local log file. _(Currently available only in the kftray desktop app)_ - [Blog Post](https://kftray.app/blog/posts/6-debug-http-traffics-kftray)
- **GitHub Sync:** Keep your configurations saved on GitHub and share or synchronize them in a GitOps style. _(Currently available only in the kftray desktop app)_

## Features Matrix

| Feature                                      | KFtray (Desktop App) | KFtui (Terminal UI) |
|----------------------------------------------|----------------------|---------------------|
| Resilient Port Forwarding Connection         | ✔️                   | ✔️                  |
| One-Click Multiple Port Forwards             | ✔️                   | ✔️                  |
| Independent of Kubectl                       | ✔️                   | ✔️                  |
| Multi-Protocol Support (TCP/UDP)             | ✔️                   | ✔️                  |
| HTTP Logs Tracing                            | ✔️                   | ❌    (Coming Soon)              |
| GitHub Sync                                  | ✔️                   | ❌  (Coming Soon)                  |
| Local JSON File Configuration                | ✔️                   | ✔️                  |



## Installation

- **KFtray Desktop App:** Check [INSTALL.md](https://github.com/hcavarsan/kftray/tree/main/docs/kftray/INSTALL.md).
- **KFtui:** Check [INSTALL.md](https://github.com/hcavarsan/kftray/tree/main/docs/kftui/INSTALL.md).

## Building from Source

- **KFtray Desktop App:** Check [BUILD.md](https://github.com/hcavarsan/kftray/tree/main/docs/kftray/BUILD.md).
- **KFtui:** Check [BUILD.md](https://github.com/hcavarsan/kftray/tree/main/docs/kftui/BUILD.md).


## 🏗 Architecture

For an overall architectural review, check [ARCH.md](https://github.com/hcavarsan/kftray/tree/main/docs/ARCH.md).



## 👥 Contributing

- **Pull Requests:** Feel free to create pull requests for bug fixes, new features, or improvements.
- **Issues:** Report bugs, suggest new features, or ask questions.
- **Feedback:** Your feedback helps improve kftray.



## 📄 License

KFtray is available under the [MIT License](LICENSE.md). See the LICENSE file for full details.


## Star History

<a href="https://star-history.com/#hcavarsan/kftray&Date">
 <picture>
   <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=hcavarsan/kftray&type=Date&theme=dark" />
   <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/svg?repos=hcavarsan/kftray&type=Date" />
   <img alt="Star History Chart" src="https://api.star-history.com/svg?repos=hcavarsan/kftray&type=Date" />
 </picture>
</a>

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
      <td align="center" valign="top" width="14.28%"><a href="https://speakerdeck.com/eltociear"><img src="https://avatars.githubusercontent.com/u/22633385?v=4?s=100" width="100px;" alt="Ikko Eltociear Ashimine"/><br /><sub><b>Ikko Eltociear Ashimine</b></sub></a><br /><a href="https://github.com/hcavarsan/kftray/commits?author=eltociear" title="Code">💻</a></td>
    </tr>
  </tbody>
</table>

<!-- markdownlint-restore -->
<!-- prettier-ignore-end -->

<!-- ALL-CONTRIBUTORS-LIST:END -->

This project follows the [all-contributors](https://github.com/all-contributors/all-contributors) specification. Contributions of any kind welcome!

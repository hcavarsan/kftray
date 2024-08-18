<div align="center">
  <br>
  <img src="https://raw.githubusercontent.com/hcavarsan/kftray-blog/main/img/logo.png" width="128px" alt="KFtray Logo" />
  <br><br>
  <a href="https://kftray.app"><strong>Visit kftray.app »</strong></a>
  <br><br>
</div>

<p align="center">
  KFtray and KFtui are independent, cross-platform applications. They help you set up and manage multiple port-forwarding settings easily. Both apps are part of the same open-source project and aim to make working with Kubernetes easier. KFtray has a desktop interface, while KFtui has a terminal interface, so you can choose the one that suits you best.
</p>

<div align="center"> <h2>KFtray</h2> <img src="https://raw.githubusercontent.com/hcavarsan/kftray-blog/main/img/ss3.png" alt="KFtray Screenshot" width="600px" style="margin: 0 10px; border-radius: 15px;" /> </div>

<div align="center"> <h2>KFtui</h2> <img src="https://raw.githubusercontent.com/hcavarsan/kftray-blog/main/img/sstui2.png" alt="KFtui Screenshot" width="600px" style="margin: 0 10px; border-radius: 15px;" /> </div>

## ❓ Why

KFtray and KFtui were made to fix common problems with Kubernetes port forwarding. The usual kubectl port-forward command can be unreliable. For example, if a pod dies, it needs manual reconnection. It also has trouble setting up many port forwards at once and doesn't support UDP services.

KFtray and KFtui solve these issues by being more reliable and easier to use. They automatically reconnect to running pods if one fails, allow setting up multiple port forwards with one click, and support both TCP and UDP protocols. KFtray also has extra features like HTTP logs tracing and GitHub sync to make workflows smoother.

## 📑 Table of Contents

- [✨ Features](#-features)
- [🗂 Features Matrix](#-features-matrix)
- [🛠 Installation](#-installation)
- [📚 Usage](#-usage)
- [🏗 Architecture](#-architecture)
- [👥 Contributing](#-contributing)
- [📄 License](#-license)

## ✨ Features

- **Resilient Port Forwarding Connection:** Ensures continuous service even if a pod dies by reconnecting to another running pod automatically.
- **One-Click Multiple Port Forwards:** Allows for the setup of several port forwarding instances simultaneously with a single click.
- **Independent of Kubectl:** Directly interfaces with the Kubernetes API, eliminating the need for `kubectl`.
- **Multi-Protocol Support:** Enables access to internal or external servers through a Proxy Relay server deployed in a Kubernetes cluster, including TCP and UDP port forwarding.
- **HTTP Logs Tracing:** Enable or disable HTTP logs for specific configurations to save the requests and responses in a local log file. _(Currently available only in the KFtray desktop app)_ - [Blog Post](https://kftray.app/blog/posts/6-debug-http-traffics-kftray)
- **GitHub Sync:** Keep your configurations saved on GitHub and share or synchronize them in a GitOps style. _(Currently available only in the KFtray desktop app)_

## 🗂 Features Matrix

<div align="center">

| Feature                                      | KFtray (Desktop App) | KFtui (Terminal UI) |
|----------------------------------------------|----------------------|---------------------|
| Resilient Port Forwarding Connection         | ✔️                   | ✔️                  |
| One-Click Multiple Port Forwards             | ✔️                   | ✔️                  |
| Independent of Kubectl                       | ✔️                   | ✔️                  |
| Multi-Protocol Support (TCP/UDP)             | ✔️                   | ✔️                  |
| HTTP Logs Tracing                            | ✔️                   | ❌ (Coming Soon)    |
| GitHub Sync                                  | ✔️                   | ❌ (Coming Soon)    |
| Local JSON File Configuration                | ✔️                   | ✔️                  |

</div>

## 🛠 Installation

- **KFtray Desktop App:** Check [INSTALL.md](https://github.com/hcavarsan/kftray/tree/main/docs/kftray/INSTALL.md).
- **KFtui:** Check [INSTALL.md](https://github.com/hcavarsan/kftray/tree/main/docs/kftui/INSTALL.md).

## 📚 Usage

- **KFtray Desktop App:** Check [USAGE.md](https://github.com/hcavarsan/kftray/tree/main/docs/kftray/USAGE.md).
- **KFtui:** Check [USAGE.md](https://github.com/hcavarsan/kftray/tree/main/docs/kftui/USAGE.md).

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

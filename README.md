<div align="center">
  <br>
  <img src="https://raw.githubusercontent.com/hcavarsan/kftray-blog/main/img/logo.png" width="128px" alt="kftray Logo" />
  <br><br>
  <a href="https://kftray.app"><strong>Visit kftray.app »</strong></a>
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
  <br><br>
</div>


<p align="center">

  kftray and kftui are independent, cross-platform applications. They help you set up and manage multiple port-forwarding settings easily. Both apps are part of the same open-source project and aim to make working with Kubernetes easier. kftray has a desktop interface, while kftui has a terminal interface, so you can choose the one that suits you best.

</p>

## Why

Both apps were made to fix common problems with Kubernetes port forwarding. The usual kubectl port-forward command can be unreliable. For example, if a pod dies, it needs manual reconnection. It also has trouble setting up many port forwards at once and doesn't support UDP services.

They automatically reconnect to running pods if one fails, also it allow setting up multiple port forwards with one click, and support both TCP and UDP protocols. kftray also has extra features like HTTP logs tracing and GitHub sync to make workflows smoother.

<br>

<br>

<div align="center">
  <table>
    <tr>
      <td>
        <a href="https://youtu.be/H6UJCfUr8yE">
          <img src="https://img.youtube.com/vi/H6UJCfUr8yE/maxresdefault.jpg" alt="Watch the video" width="800px">
        </a>
      </td>
      <td>
        <a href="https://youtu.be/d-Je34Hy5Lo">
          <img src="https://img.youtube.com/vi/d-Je34Hy5Lo/maxresdefault.jpg" alt="Watch the video" width="800px">
        </a>
      </td>
    </tr>
  </table>
</div>

<br>
<br>


## Features

- **Resilient Port Forwarding Connection:** Ensures continuous service even if a pod dies by reconnecting to another running pod automatically.
- **One-Click Multiple Port Forwards:** Allows for the setup of several port forwarding instances simultaneously with a single click.
- **Independent of Kubectl:** Directly interfaces with the Kubernetes API, eliminating the need for `kubectl`.
- **Multi-Protocol Support:** Enables access to internal or external servers through a Proxy Relay server deployed in a Kubernetes cluster, including TCP and UDP port forwarding.
- **HTTP Logs Tracing:** Enable or disable HTTP logs for specific configurations to save the requests and responses in a local log file. _(Currently available only in the kftray desktop app)_ - [Blog Post](https://kftray.app/blog/posts/6-debug-http-traffics-kftray)
- **GitHub Sync:** Keep your configurations saved on GitHub and share or synchronize them in a GitOps style. _(Currently available only in the kftray desktop app)_
- **Auto Import:** Automatically import Kubernetes service configurations based on specific annotations. An example with an explanation can be found in this repo: https://github.com/hcavarsan/kftray-k8s-tf-example

<br>

<div align="center">

| Feature                                      | kftray (Desktop App) | kftui (Terminal UI) |
|----------------------------------------------|----------------------|---------------------|
| Resilient Port Forwarding Connection         | ✔️                   | ✔️                  |
| One-Click Multiple Port Forwards             | ✔️                   | ✔️                  |
| Independent of Kubectl                       | ✔️                   | ✔️                  |
| Multi-Protocol Support (TCP/UDP)             | ✔️                   | ✔️                  |
| HTTP Logs Tracing                            | ✔️                   | ❌ (Coming Soon)    |
| GitHub Sync                                  | ✔️                   | ❌ (Coming Soon)    |
| Local JSON File Configuration                | ✔️                   | ✔️                  |
| Auto Import with k8s Annotations             | ✔️                   | ✔️                  |

</div>

<br>

## kftray - Desktop App

- [INSTALL.md](https://github.com/hcavarsan/kftray/tree/main/docs/kftray/INSTALL.md)
- [USAGE.md](https://github.com/hcavarsan/kftray/tree/main/docs/kftray/USAGE.md)
- [BUILD.md](https://github.com/hcavarsan/kftray/tree/main/docs/kftray/BUILD.md)

## kftui - Terminal User Interface

- [INSTALL.md](https://github.com/hcavarsan/kftray/tree/main/docs/kftui/INSTALL.md)
- [USAGE.md](https://github.com/hcavarsan/kftray/tree/main/docs/kftui/USAGE.md)
- [BUILD.md](https://github.com/hcavarsan/kftray/tree/main/docs/kftui/BUILD.md)

## kftray server - Proxy Relay Server

- [ARCH.md](https://github.com/hcavarsan/kftray/tree/main/docs/ARCH.md).

## Contributing

- **Pull Requests:** Feel free to create pull requests for bug fixes, new features, or improvements.
- **Issues:** Report bugs, suggest new features, or ask questions.
- **Feedback:** Your feedback helps improve kftray.

##  License

kftray is available under the [MIT License](LICENSE.md). See the LICENSE file for full details.

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

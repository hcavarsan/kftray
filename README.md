<div align="center">  <br>
  <img src="https://raw.githubusercontent.com/hcavarsan/kftray-blog/main/img/logo.png" width="128px" alt="kftray Logo" />
  <br><br>
  <a href="https://kftray.app">Website</a> | <a href="https://kftray.app/downloads">Downloads</a> | <a href="https://kftray.app/docs">Docs</a> | <a href="https://kftray.app/blog">Blog</a>
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
    <img src="https://img.shields.io/codecov/c/github/hcavarsan/kftray/main?style=for-the-badge&logo=codecov" alt="Codecov Coverage">
  </a>
  <a href="https://crates.io/crates/kftui">
    <img src="https://img.shields.io/crates/v/kftui?style=for-the-badge&logo=rust" alt="Crates.io">
  </a>
  <a href="https://scorecard.dev/viewer/?uri=github.com/hcavarsan/kftray">
    <img src="https://img.shields.io/ossf-scorecard/github.com/hcavarsan/kftray?style=for-the-badge&label=openssf%20scorecard" alt="OpenSSF Scorecard">
  </a>
  <a href="https://www.bestpractices.dev/projects/11850">
    <img src="https://img.shields.io/cii/percentage/11850?style=for-the-badge&label=openssf%20best%20practices" alt="OpenSSF Best Practices">
  </a>
  <br><br>
</div>

<p align="center">
  <a href="https://kftray.app">
    <img src="https://raw.githubusercontent.com/hcavarsan/kftray-blog/main/public/video/kftray-intro-v1-readme.webp" alt="kftray and kftui demo" width="960"/>
  </a>
</p>

## About

kftray and kftui manage Kubernetes port forwards from your desktop or terminal. Both apps share configurations and a Rust backend that reconnects forwards when pods restart or connections drop.

- [kftray](https://kftray.app/docs/interfaces/desktop) runs as a desktop app with system tray integration.
- [kftui](https://kftray.app/docs/interfaces/terminal) runs in the terminal, with a CLI for scripts and headless use.

Watch the [app overview](https://www.youtube.com/watch?v=j2NnhIIFYHw) or the [HTTP logging demo](https://www.youtube.com/watch?v=73PyGbMnNSY).

## Features

- Run multiple forwards across Kubernetes contexts without `kubectl`.
- Forward TCP and UDP traffic to services or pods, or reach other hosts through a cluster proxy.
- Expose local services to the cluster or through an ingress.
- Inspect HTTP traffic and replay requests in kftui.
- Share configs through GitHub or discover services from Kubernetes annotations.
- Organize configs with tags, filters and grouping.

See the [documentation](https://kftray.app/docs) for configuration options and app behavior.

## Get started

Download [kftray or kftui](https://kftray.app/downloads), follow the [installation guide](https://kftray.app/docs/getting-started/installation) and create your [first port forward](https://kftray.app/docs/getting-started/quick-start).

- [Configuration](https://kftray.app/docs/configuration): fields, workload types and shared settings.
- [CLI reference](https://kftray.app/docs/resources/cli-reference): flags and commands for kftui.
- [Architecture](https://kftray.app/docs/resources/architecture): the shared core and cluster relay.
- [Security](https://kftray.app/docs/resources/security): vulnerability reporting and release verification.
- [Releases](https://github.com/hcavarsan/kftray/releases): changes and release assets.

## Contributing

Read the [contribution guide](https://kftray.app/docs/resources/contributing) and [code of conduct](https://kftray.app/docs/resources/code-of-conduct). For local builds, see [development](https://kftray.app/docs/resources/development) and [building from source](https://kftray.app/docs/resources/building-from-source).

Report bugs or request features through [GitHub issues](https://github.com/hcavarsan/kftray/issues). Use [Discussions](https://github.com/hcavarsan/kftray/discussions) or [Slack](https://join.slack.com/t/kftray/shared_invite/zt-2q6lwn15f-Y8Mi_4NlenH9TuEDMjxPUA) for questions.

## License

kftray is licensed under [GPL-3.0](LICENSE).

## Star History

<a href="https://star-history.com/#hcavarsan/kftray&Date">
 <picture>
   <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=hcavarsan/kftray&type=Date&theme=dark" />
   <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/svg?repos=hcavarsan/kftray&type=Date" />
   <img alt="Star History Chart" src="https://api.star-history.com/svg?repos=hcavarsan/kftray&type=Date" />
 </picture>
</a>

## Contributors

Contributors are listed below using the [all-contributors](https://allcontributors.org/docs/en/emoji-key) contribution types.

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
    <tr>
      <td align="center" valign="top" width="14.28%"><a href="https://github.com/kgara"><img src="https://avatars.githubusercontent.com/u/14247772?v=4?s=100" width="100px;" alt="kgara"/><br /><sub><b>kgara</b></sub></a><br /><a href="https://github.com/hcavarsan/kftray/commits?author=kgara" title="Code">💻</a></td>
      <td align="center" valign="top" width="14.28%"><a href="https://github.com/ramezbenaribia"><img src="https://avatars.githubusercontent.com/u/55209480?v=4?s=100" width="100px;" alt="Ramez Ben Aribia"/><br /><sub><b>Ramez Ben Aribia</b></sub></a><br /><a href="https://github.com/hcavarsan/kftray/commits?author=ramezbenaribia" title="Code">💻</a></td>
      <td align="center" valign="top" width="14.28%"><a href="https://knktc.com/"><img src="https://avatars.githubusercontent.com/u/863586?v=4?s=100" width="100px;" alt="knktc"/><br /><sub><b>knktc</b></sub></a><br /><a href="https://github.com/hcavarsan/kftray/commits?author=knktc" title="Code">💻</a></td>
      <td align="center" valign="top" width="14.28%"><a href="http://www.xrow.de/"><img src="https://avatars.githubusercontent.com/u/295491?v=4?s=100" width="100px;" alt="Björn Dieding"/><br /><sub><b>Björn Dieding</b></sub></a><br /><a href="https://github.com/hcavarsan/kftray/commits?author=xrow" title="Code">💻</a></td>
    </tr>
  </tbody>
</table>

<!-- markdownlint-restore -->
<!-- prettier-ignore-end -->

<!-- ALL-CONTRIBUTORS-LIST:END -->

# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.15.6] - 2025-01-15

### Fixed

- Bump ubuntu action with workaround

## [0.15.6] - 2025-01-15

### Fixed

- Bump deps and version

## [0.15.5] - 2025-01-13

### Fixed

- Bump version

## [0.15.4] - 2025-01-13

### Fixed

- Bump version
- Script bump also root package

## [0.15.3] - 2025-01-13

### Added

- Add homebrew update script

### Changed

- Update pnpm lock
- Update pnpm lock
- Update all non-major dependencies (#343)
- Update dependency vite to v6 (#345)
- Update react monorepo to v19 (#346)
- Update dependency jsdom to v26 (#349)
- Lock file maintenance (#348)
- Lock file maintenance (#347)
- Readme changes

### Fixed

- Tslints
- Tui-explorer pinned
- Tui-logger pinned
- Fix libwebkit2gtk version (#350)

## [0.15.2] - 2024-11-27

### Added

- Add pin window to systray menu (#344)
- Add server readme
- Add demo img

### Changed

- Bump version
- Bump rustls in the cargo group across 1 directory (#342)
- Update rust crate thiserror to v2 (#334)
- New server structure (#340)
- Update README.md
- Update configs.json
- Update examples

## [0.15.1] - 2024-11-21

### Changed

- Create new options for obtaining git creds using libgit or a github token (#339)

## [0.15.0] - 2024-11-21

### Changed

- New ui with chakra ui v3 (#338)
- Update all non-major dependencies (#335)
- Lock file maintenance (#337)
- Update rust crate keyring to 3.6.0 (#333)
- Update all non-major dependencies (#329)
- Update all non-major dependencies (#306)

### Fixed

- Fix link (#331)

## [0.14.9] - 2024-10-20

### Added

- Add close window button with port forward stop logic (#327)
- Add check and manage ports to start port forward after app restart (#325)

### Changed

- Bump version
- Optimize stop_all_port_forward function (#326)
- Change License to GPLv3 (#322)
- Update dependency eslint-plugin-react-hooks to v5 (#321)
- Update readme
- Update readme
- Update dependency ubuntu to v24 (#317)
- Improve readme
- Organize readme better
- Link example repo in README for Auto Import feature

### Fixed

- Gplv3 copyright sections

## [0.14.8] - 2024-09-29

### Changed

- Update some packages also use mise (rust 1.81 and node 22)  (#316)
- Replace custom logger with tui-logger (#314)
- Improve AutoImportModal with custom kubeconfig support (#315)

## [0.14.7] - 2024-09-23

### Changed

- If local_port is not set, get a random tcp unused port
- If local_port is not set, get a random tcp unused port (#313)

## [0.14.6] - 2024-09-23

### Changed

- Release patch

### Fixed

- Test client with api version instead list namespace (#312)

## [0.14.5] - 2024-09-21

### Added

- Add buildx setup in ci to have a multi-platform kftray container (#309)

### Changed

- Release patch

## [0.14.4] - 2024-09-21

### Changed

- Release patch
- Some minor improvements in add/import/export configs (#307)

### Fixed

- Some state handling wrong and also improve logic to set default values

## [0.14.3] - 2024-09-20

### Changed

- Release patch

### Fixed

- Pass custom kubeconfig to list_ports backend func

## [0.14.2] - 2024-09-18

### Fixed

- Eks auth failed (#303)

## [0.14.1] - 2024-09-14

### Changed

- Improve kube client to handle more cases before fails (#302)

### Fixed

- Duplicating clients in use
- Duplicating clients in use

## [0.14.0] - 2024-09-08

### Added

- Add auto import based on kube annotations (#300)
- Add readme badges and slack channel
- Add readme badges and slack channel
- Add readme badges and slack channel
- Add readme badges and slack channel

### Changed

- Export configs with pretty json (#301)
- Update all non-major dependencies (#297)
- Update dependency jsdom to v25 (#296)
- Update all non-major dependencies (#295)
- Update all non-major dependencies (#290)

## [0.13.3] - 2024-08-26

### Changed

- Update main.yml
- Update main.yml
- Update main.yml
- Update main.yml
- Update main.yml
- Update main.yml
- Update main.yml
- Update version package
- Update rust crate sqlx to v0.8.1 [security] (#291)

### Fixed

- Config perl
- Configure perl
- Configure perl
- Configure perl
- Configure perl
- Install perl
- Use OpenSSL vendored
- Kube client when have multiple kubeconfig and fix stop port forward (#293)
- Install script must use tmp dir to download app
- Install script must use tmp dir to download app

### Removed

- Remove hyper

## [0.13.2] - 2024-08-20

### Changed

- Bump version
- Improve readme
- Improve readme
- Improve readme

### Fixed

- Handle cases where the kubeconfig contains multiple paths (#288)

## [0.13.1] - 2024-08-18

### Added

- Add job to deploy kftray-tui artifacts

### Changed

- Adjust docs
- Adjust docs

### Fixed

- Kftui details not working properly (#287)

## [0.13.0] - 2024-08-17

### Added

- Add job to deploy kftray-tui artifacts (#285)

### Changed

- Lock file maintenance (#281)
- Update rust docker tag to v1.80.1 (#280)
- Update typescript-eslint monorepo to v8 (#279)
- Update rust docker tag to v1.80.0 (#277)
- Update all non-major dependencies (#276)
- Upgrade dependencies and rollback from farm to vite (#274)
- Update rust crate openssl to v0.10.66 [security] (#273)
- Lock file maintenance (#272)
- Update rust crate open to 5.2.0 (#270)

## [0.12.2] - 2024-07-02

### Changed

- Update version
- Update rust crate dashmap to v6 (#267)

### Fixed

- Port forwarding stuck when http redirect to https (#269)

## [0.12.1] - 2024-06-21

### Added

- Add workflow to stale prs and issues

### Changed

- Refactor configs dirs (#265)

### Fixed

- Fix dialog message

## [0.12.0] - 2024-06-20

### Added

- Add new workload pod and fixed orphan tcp connections
- Add new workload pod and fixed orphan tcp connections
- Add new workload pod and fixed orphan tcp connections (#263)
- Add eltociear as a contributor for code (#259)

### Changed

- Update all non-major dependencies (#256)
- Only build image in ci workflow (#262)
- Set version in package

## [0.11.7] - 2024-06-12

### Changed

- New release
- Improve http logging and tcp connections (#257)
- Update all deps and fix some errors (#251)
- Update README.md (#253)
- Inspect HTTP logs for service forwarding (#249)
- Update rust crate anyhow to 1.0.86 (#248)

## [0.11.6] - 2024-06-02

### Added

- Add pin button to set always visible and focused (#247)

### Changed

- New release

### Removed

- Remove hover and active effects - pin window button

## [0.11.5] - 2024-06-01

### Changed

- Migrate from vite to farm (#243)

### Fixed

- Handle better window position size with multiple monitors and scalefactorchanged (#245)
- Bug when the kubeconfig location is default (#244)
- Don't close the app window when the position is invalid. (#241)

## [0.11.3] - 2024-05-28

### Changed

- Bump version
- Codesign and notarize macos bundle (#240)
- Update all non-major dependencies (#239)

## [0.11.2] - 2024-05-25

### Added

- Sponsor info

### Changed

- Improved stop logic and minify frontend (#237)
- Update rust crate tauri to v1.6.7 [security] (#233)
- Update FUNDING.yml
- Update readme
- Update readme

### Fixed

- Bug in stop forward
- Bug in stop forward

## [0.11.1] - 2024-05-22

### Added

- Add button to change window position with saved state (#229)

### Changed

- Update readme

### Fixed

- Menulist popover position and also the initial window position (#231)

## [0.10.7] - 2024-05-19

### Added

- Add tag to layout of context accordion item

### Changed

- Improve window position
- Improve tagicons layout

## [0.10.6] - 2024-05-19

### Changed

- Improve tagicons layout
- Release new version
- New release ss
- Trying to improve the build and reduce app bundle size (#226)
- Improve layout to be more consistent (#227)
- Improve readme
- Set logo size
- Improve readme layout (#225)
- Improve Readme header logo and screenshot (#224)

## [0.10.5] - 2024-05-18

### Changed

- New code organization to better usability (#221)

### Fixed

- Bug in gitsyncmodal related to useeffect loop (#222)

## [0.10.3] - 2024-05-15

### Changed

- Update rust crate anyhow to 1.0.83 (#218)
- Frontend code overall organization (#217)
- Improve frontend code by splitting some components  (#216)

### Fixed

- Use enigo crate for linux mouse position (#220)

## [0.10.2] - 2024-05-08

### Changed

- Bump version

### Fixed

- Disable the switch until the port forwarding operation is complete

## [0.10.1] - 2024-05-08

### Changed

- Update version
- Improve error handling and create default toast component (#214)
- Improve error handling to start or stop port forwarding if the … (#213)
- Update all non-major dependencies (#212)

## [0.10.0] - 2024-05-07

### Added

- Add kftray demo overview
- Add kftray overview demo in readme
- Add ss to readme
- Add ss to readme

### Changed

- New release
- New release
- Improve frontend code and fix some lints (#211)
- Improve lint and fix minor bugs with the new lint configuration (#210)
- Initial custom kubeconfig rust code (#208)
- Update rust docker tag to v1.78.0 (#209)
- Better error handling to toggle port forwards (#207)

## [0.9.8] - 2024-04-25

### Added

- Add the alias domain workaround

### Changed

- Improve readme
- Improve readme
- Update npm dependencies
- Bump the cargo group across 2 directories with 1 update (#205)
- Disable renovate dependency dashboard
- Update README.md
- Update dependency @testing-library/react to v15 (#203)
- Update all non-major dependencies (#202)

### Fixed

- Darwin target

### Removed

- Remove deps in renovate check

## [0.9.7] - 2024-04-12

### Fixed

- Incorrect window position in linux when click in toggle button (#199)

## [0.9.6] - 2024-04-10

### Fixed

- Fix arrow position in windows OS

## [0.9.5] - 2024-04-10

### Fixed

- Fix arrow position in each OS
- Show window in linux and windows
- Fmt
- Import github configs fails when configs is empty
- Examples

## [0.9.2] - 2024-04-09

### Changed

- Change unique universal icon to better compatibility
- Change unique universal icon to better compatibility
- Update all non-major dependencies (#196)

## [0.9.1] - 2024-04-05

### Changed

- New version
- Change the tray icon to monochromatic style (#195)
- Bump the cargo group across 2 directories with 1 update (#193)
- Upgrade github actions dependencies (#190)
- Update all non-major dependencies (#185)
- Change readme layout
- Update dependency vite to v5.1.7 [security] (#189)
- Update star chart
- Update readme (#186)

### Fixed

- H2 security hotfix
- Fix badges

## [0.9.0] - 2024-03-27

### Added

- Add frontend logic to enable domain hosts
- Add hcavarsan as a contributor for code (#177)
- Add fandujar as a contributor for code (#179)

### Changed

- Release new version
- Logic to add entries hosts based on alias and config id (#184)
- Bump the cargo group across 2 directories with 1 update (#183)
- Adjust readme
- Update readme
- Adjust readme
- Bump mio from 0.8.10 to 0.8.11 in /kftray-server (#175)
- Update readme

### Fixed

- Upgrade libs (#182)
- Build error - footermenu interface
- Fix contributors file
- Fix readme

## [0.8.0] - 2024-03-03

### Added

- Add new feature to configure sync github polling (#172)
- Add new icon to open local url (#166)

### Changed

- Prepare new version
- Prepare new version
- Improve overall layout (#174)
- Prepare new version
- Improve overall layout
- Improve overall layout
- Improve overall layout
- Improve overall layout
- WIP refactor: improve code readability (#167)
- Improve overall scripts usage and use rust to automated task utils (#165)

### Removed

- Remove duplicated sync button

## [0.7.4] - 2024-03-01

### Changed

- Force new release
- Docs force

## [0.7.3] - 2024-03-01

### Added

- Add comments

### Changed

- Improve menu footer layout
- Improve menu footer layout
- Improve menu footer layout
- Prepare new version
- Improve menu footer layout
- Improve menu footer layout
- Improve menu footer layout (#163)
- Update renovate.json
- Change renovate
- Configure Renovate (#146)

### Removed

- Remove comments
- Remove unused comments
- Remove unused comments

## [0.7.2] - 2024-02-29

### Added

- Add breakline
- Add some comments

### Changed

- Adjust new version
- Prepare new version
- Update deps
- Improve readme
- Improve readme
- Improve readme
- Improve readme
- Improve readme
- Improve readme
- Improve readme
- Improve readme
- Improve readme

### Fixed

- Resolve ip address before relay (#144)
- Increase relay buffer size (#143)
- Versions in readme (#142)

## [0.7.1] - 2024-02-27

### Added

- Add initial comments
- Add initial comments
- Add initial comments

### Changed

- Prepare new release
- Update readme with git sync info
- Improve the log lib

### Fixed

- Useeffect loop with high cpu usage (#140)
- Fix conflicts (#139)
- Fix typo in readme

## [0.7.0] - 2024-02-23

### Added

- Add more logs and improve relay server connections (#127)
- Add initial tests for rust code (#126)
- Add initial tests for kftray-server (#125)
- Add git feature in README
- Add new button to delete multiples configs (#118)
- Add PULL_REQUEST_TEMPLATE.md
- Add  SECURITY.md
- Add issues template
- Add proxy manifest config in readme
- Add option to select configs to start
- Patreon
- Add badges in readme
- Add starchart
- Add debug option file
- Add autocomplete fields (#32)
- Add brew formula
- Add brew formula
- Add brew formula
- Add brew formula
- Add port forward for specific configs (#25)
- Add auto updater
- Add auto updater
- Add auto updater
- Add auto updater
- Add new buttons to import/export configs
- Add new buttons to import/export configs
- Add new buttons to import/export configs
- Add button to edit configs
- Add confirmation button in delete configs
- Add confirmation button in delete configs

### Changed

- Create new mode to enable state sync with github repositories (#128)
- Update readme typos and consistency
- Update README
- Update readme title
- Update README
- Prepare new version
- Docker image reduce size (#124)
- Rewrite in rust  and improve kftray server (#123)
- Import configs from git (#112)
- Commit example json
- Split frontend menu component  (#109)
- Code better readable (#107)
- Update deps
- Update deps
- Update deps (#103)
- Update deps (#101)
- Deps update (#100)
- Update deps (#99)
- Update deps (#95)
- Update readme
- Update libs (#79)
- Update readme (#76)
- Create CONTRIBUTING.md
- Create CODE_OF_CONDUCT.md
- Upgrade deps and remove unused (#66)
- Dependabot config
- Create dependabot.yml
- Update readme gif demo
- Update readme gif demo
- Update readme gif demo
- Prepare new version (#53)
- Improve overall layout of window and fix bug when start all for… (#52)
- 0.5.6 version
- Custom krelay server pod configs (#50)
- Update image
- Disable switch when the function is running (#47)
- Prepare version 0.5.2
- Update components and remove unused code/deps (#46)
- Improve the ci and add step to build and push server image (#45)
- Improve readme
- Improve readme
- Improve readme
- Improve readme
- Update readme
- Forward tcp proxy connection (#34)
- New version
- Window layout final version
- Window layout final version
- Window layout final version (#30)
- Improve overall layout (#28)
- Overall improvements in port forwarding usage (#27)
- Refactor frontend code (#22)
- New icon
- General improvements and tests (#20)
- Merge pull request #18 from fandujar/main
- Separate table
- Merge pull request #17 from fandujar/main
- Separate footer
- Merge pull request #16 from fandujar/main
- Split add new config as a component
- Merge pull request #15 from fandujar/main
- Separate portforward item component
- Merge pull request #14 from fandujar/main
- Separate kftray and header component
- Improve layout action buttons
- Improve window layout
- Improve window layout
- Improve window layout
- Improve window layout
- Improve window layout
- Improve table layout
- Improve table layout
- Improve table layout
- Improve layout
- Improve README
- Improve layout and add scroll bar
- Update README
- Update README
- Generate macos universal binary
- Generate macos universal binary
- Improve tray window layout
- Improve tray window layout
- Improve tray window layout
- Improve modal button style
- Update kubectl dependency information
- Use kube-rs instead kubectl
- Use kube-rs instead kubectl
- Namespace visible in table status
- Improve readme
- Improve readme
- Improve readme
- Improve readme
- Improve readme
- Improve readme
- Improve readme
- Improve readme
- Refactor code - new version with tauri
- Refactor code
- First commit
- Initial commit

### Fixed

- Fmt
- Bug to open app in linux and add global shortcut (#88)
- Convert username to lowercase (#51)
- Readme
- Bug  in stop forward state
- Some bugs in configs state
- Some bugs in configs state
- Some bugs in configs state
- Some bugs in configs state
- Readme
- Build badge
- README
- Readme
- Readme
- Readme
- Readme
- Get ssl dinamically from kubeconfig to client - kube api
- Get ssl dinamically from kubeconfig to client - kube api (#37)
- Get target pod instead port
- Get target pod instead port
- Fmt
- Fmt
- Readme
- Table search headers layout
- Lints (#21)
- Readme
- Set universal binary in reame
- Ci
- Ci
- Ci
- Ci
- Ci
- Ci
- Ci
- Ci
- Readme
- LeastDestructiveRef
- LeastDestructiveRef
- Update the demo gif
- Update the demo gif
- Update the demo gif
- Clean modal fields when open a new add config
- Readme links to latest release
- Workflow
- Workflow
- Workflow
- Workflow
- Workflow
- Workflow
- Workflow
- Config sample

[0.15.6]: https://github.com///compare/v0.15.6..v0.15.6
[0.15.6]: https://github.com///compare/v0.15.5..v0.15.6
[0.15.5]: https://github.com///compare/v0.15.4..v0.15.5
[0.15.4]: https://github.com///compare/v0.15.3..v0.15.4
[0.15.3]: https://github.com///compare/v0.15.2..v0.15.3
[0.15.2]: https://github.com///compare/v0.15.1..v0.15.2
[0.15.1]: https://github.com///compare/v0.15.0..v0.15.1
[0.15.0]: https://github.com///compare/v0.14.9..v0.15.0
[0.14.9]: https://github.com///compare/v0.14.8..v0.14.9
[0.14.8]: https://github.com///compare/v0.14.7..v0.14.8
[0.14.7]: https://github.com///compare/v0.14.6..v0.14.7
[0.14.6]: https://github.com///compare/v0.14.5..v0.14.6
[0.14.5]: https://github.com///compare/v0.14.4..v0.14.5
[0.14.4]: https://github.com///compare/v0.14.3..v0.14.4
[0.14.3]: https://github.com///compare/v0.14.2..v0.14.3
[0.14.2]: https://github.com///compare/v0.14.1..v0.14.2
[0.14.1]: https://github.com///compare/v0.14.0..v0.14.1
[0.14.0]: https://github.com///compare/v0.13.3..v0.14.0
[0.13.3]: https://github.com///compare/v0.13.2..v0.13.3
[0.13.2]: https://github.com///compare/v0.13.1..v0.13.2
[0.13.1]: https://github.com///compare/v0.13.0..v0.13.1
[0.13.0]: https://github.com///compare/v0.12.2..v0.13.0
[0.12.2]: https://github.com///compare/v0.12.1..v0.12.2
[0.12.1]: https://github.com///compare/v0.12.0..v0.12.1
[0.12.0]: https://github.com///compare/v0.11.7..v0.12.0
[0.11.7]: https://github.com///compare/v0.11.6..v0.11.7
[0.11.6]: https://github.com///compare/v0.11.5..v0.11.6
[0.11.5]: https://github.com///compare/v0.11.3..v0.11.5
[0.11.3]: https://github.com///compare/v0.11.2..v0.11.3
[0.11.2]: https://github.com///compare/v0.11.1..v0.11.2
[0.11.1]: https://github.com///compare/v0.10.7..v0.11.1
[0.10.7]: https://github.com///compare/v0.10.6..v0.10.7
[0.10.6]: https://github.com///compare/v0.10.5..v0.10.6
[0.10.5]: https://github.com///compare/v0.10.3..v0.10.5
[0.10.3]: https://github.com///compare/v0.10.2..v0.10.3
[0.10.2]: https://github.com///compare/v0.10.1..v0.10.2
[0.10.1]: https://github.com///compare/v0.10.0..v0.10.1
[0.10.0]: https://github.com///compare/v0.9.8..v0.10.0
[0.9.8]: https://github.com///compare/v0.9.7..v0.9.8
[0.9.7]: https://github.com///compare/v0.9.6..v0.9.7
[0.9.6]: https://github.com///compare/v0.9.5..v0.9.6
[0.9.5]: https://github.com///compare/v0.9.2..v0.9.5
[0.9.2]: https://github.com///compare/v0.9.1..v0.9.2
[0.9.1]: https://github.com///compare/v0.9.0..v0.9.1
[0.9.0]: https://github.com///compare/v0.8.0..v0.9.0
[0.8.0]: https://github.com///compare/v0.7.4..v0.8.0
[0.7.4]: https://github.com///compare/v0.7.3..v0.7.4
[0.7.3]: https://github.com///compare/v0.7.2..v0.7.3
[0.7.2]: https://github.com///compare/v0.7.1..v0.7.2
[0.7.1]: https://github.com///compare/v0.7.0..v0.7.1

<!-- generated by git-cliff -->

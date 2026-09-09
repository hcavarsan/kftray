# OBS publishing

The release workflow publishes Linux packages to the OpenSUSE Build Service after
the GitHub release is public and its updater metadata is final. Each package uses
one source archive containing the release binaries, downloaded from the release
and verified against GitHub's SHA256 asset digests. RPM and Debian packaging must
not strip the AppImage, because stripping discards the embedded filesystem.
The kftray archive includes both glibc variants for each architecture; package
builds select the newer variant on glibc 2.39 or later, matching the updater.

## Local validation

Use Linux with Bash, curl, jq, python3, GNU tar/coreutils, gzip, xz, and xmllint:

```sh
VERSION=0.27.31 bash hacks/obs/dry-run.sh
VERSION=0.27.31 bash hacks/obs/dry-run.sh kftui
```

Dry runs download and verify the real artifacts, render the same templates and
project metadata as the publisher, and remove their temporary files. They do not
need OBS credentials and never write to OBS. They check archive integrity and
preparation, not application ABI compatibility or remote build results. Releases
without GitHub asset digests are rejected rather than uploaded unverified.
Set `GITHUB_TOKEN` to avoid the anonymous GitHub API rate limit.

Offline regression checks:

```sh
shellcheck hacks/obs/*.sh
bash hacks/obs/test-publish.sh
bash hacks/obs/test-package-builds.sh
uv run --project hacks --with pytest python -m pytest hacks/tests/test_fix_updater_json.py
```

The package-build test additionally requires build-essential, pkg-config,
debhelper, libdbus-1-dev, rpm, and cpio. It builds native Debian and RPM packages,
checks dependency metadata, and compares opaque ELF payloads before and after
packaging. Set `OBS_TEST_APPIMAGE` and `OBS_TEST_NEWER_APPIMAGE` to local AppImages
to test their exact bytes on the corresponding target system. CI runs these checks
from `.github/workflows/obs-publishing.yml` when the publishing files change.

## Publishing

```sh
export VERSION=0.27.31 OBS_USER=your-user OBS_PASSWORD=your-password GITHUB_TOKEN=...
bash hacks/obs/setup.sh
bash hacks/obs/publish.sh
```

All selected packages are prepared before any OBS write. The publisher then:

1. Replaces the OBS project's repository list with `distros.conf`, creating the
   project when it does not exist. Other project settings (title, maintainers,
   build flags) are preserved. Removing a target from `distros.conf` removes its
   repository and published packages on the next run.
2. Makes each OBS package mirror the rendered templates and generated archives
   exactly; files from earlier versions are deleted.
3. Commits, waits until the OBS scheduler has picked up the new revision, then
   waits for the builds (`OBS_RESULTS_TIMEOUT`, default `30m` per package).
   Failed, broken, unresolved, unfinished, or empty build results fail the
   command, so every target in `distros.conf` must build.

The command is idempotent: rerunning it for the same version detects no changes,
waits for the current builds, and reports their result. When the release job
fails because OBS was slow, rerun the job. GitHub Actions serializes OBS
publication jobs and limits each to 90 minutes.

## Release history notes

kftui ARM64 binaries up to v0.27.31 were built on Ubuntu 24.04 and require
glibc 2.39; they cannot run on Ubuntu 22.04 or Debian 12 even when repackaged.
Later releases build ARM64 on Ubuntu 22.04.

Updater manifests published before the finalizer moved ahead of publication may
carry generic `linux-*` entries that point to deleted assets. Rerunning the
finalizer against the tag repairs the published `latest.json` in place:

```sh
GH_REPO=hcavarsan/kftray uv run hacks/fix_updater_json.py v0.27.31
```

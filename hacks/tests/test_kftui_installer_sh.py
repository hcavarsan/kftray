import os
import shutil
import subprocess
from pathlib import Path

import pytest

INSTALLER = Path(__file__).resolve().parents[1] / "kftui_installer.sh"

CURL_SHIM = """#!/bin/sh
out=""
while [ $# -gt 0 ]; do
  if [ "$1" = "-o" ]; then out="$2"; shift; fi
  shift
done
if [ -n "$out" ]; then
  echo stub > "$out"
else
  echo '  "tag_name": "v9.9.9",'
fi
"""

UNAME_SHIM = """#!/bin/sh
case "$1" in
  -s) echo "$SHIM_OS" ;;
  -m) echo "$SHIM_ARCH" ;;
esac
"""

pytestmark = pytest.mark.skipif(
    shutil.which("bash") is None, reason="bash is not installed"
)


def run_installer(tmp_path: Path, os_name: str, arch: str) -> subprocess.CompletedProcess[str]:
    shims = tmp_path / "shims"
    shims.mkdir()
    for name, body in (("curl", CURL_SHIM), ("uname", UNAME_SHIM)):
        shim = shims / name
        shim.write_text(body)
        shim.chmod(0o755)
    home = tmp_path / "home"
    home.mkdir()
    return subprocess.run(
        ["bash", str(INSTALLER)],
        env={
            **os.environ,
            "HOME": str(home),
            "PATH": f"{shims}{os.pathsep}{os.environ['PATH']}",
            "SHIM_OS": os_name,
            "SHIM_ARCH": arch,
        },
        capture_output=True,
        text=True,
        check=False,
    )


@pytest.mark.parametrize(
    ("os_name", "arch", "message"),
    [
        ("MINGW64_NT-10.0-19045", "x86_64", "Unsupported OS"),
        ("FreeBSD", "amd64", "Unsupported OS"),
        ("Linux", "armv7l", "Unsupported architecture"),
    ],
)
def test_unsupported_platform_prints_reason_on_stderr(
    tmp_path: Path, os_name: str, arch: str, message: str
) -> None:
    result = run_installer(tmp_path, os_name, arch)

    assert result.returncode == 1
    assert message in result.stderr


@pytest.mark.skipif(
    Path("/proc/version").exists() and "Microsoft" in Path("/proc/version").read_text(),
    reason="the installer treats this host as WSL",
)
@pytest.mark.parametrize(
    ("os_name", "arch", "asset"),
    [
        ("Linux", "x86_64", "kftui_linux_amd64"),
        ("Linux", "aarch64", "kftui_linux_arm64"),
        ("Darwin", "arm64", "kftui_macos_universal"),
    ],
)
def test_supported_platform_reaches_download(
    tmp_path: Path, os_name: str, arch: str, asset: str
) -> None:
    if os_name == "Linux" and not Path("/proc/version").exists():
        pytest.skip("the Linux branch reads /proc/version")

    result = run_installer(tmp_path, os_name, arch)

    assert f"/releases/download/v9.9.9/{asset}" in result.stdout
    assert "Unsupported" not in result.stderr

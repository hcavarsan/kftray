import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

INSTALLER = Path(__file__).resolve().parents[1] / "kftui_installer.ps1"

STUBS = r"""
$ErrorActionPreference = 'Stop'
function Invoke-RestMethod { [pscustomobject]@{ tag_name = 'v9.9.9' } }
function Invoke-WebRequest {
    param([string]$Uri, [string]$OutFile)
    Set-Content -Path $OutFile -Value "#!/bin/sh`necho kftui-stub $Uri"
    chmod +x $OutFile
}
"""


@pytest.mark.skipif(shutil.which("pwsh") is None, reason="pwsh is not installed")
@pytest.mark.skipif(sys.platform == "win32", reason="the stub binary is a shell script")
def test_installer_puts_kftui_in_install_dir_and_on_path(tmp_path: Path) -> None:
    home = tmp_path / "home"
    home.mkdir()
    work = tmp_path / "work"
    work.mkdir()
    command = (
        STUBS
        + f"Invoke-Expression (Get-Content -Raw '{INSTALLER}')\n"
        + "Write-Output \"SESSION_PATH=$env:PATH\"\n"
    )

    result = subprocess.run(
        ["pwsh", "-NoProfile", "-NonInteractive", "-Command", command],
        cwd=work,
        env={**os.environ, "HOME": str(home)},
        capture_output=True,
        text=True,
        check=False,
    )

    assert result.returncode == 0, result.stdout + result.stderr
    install_dir = home / ".local" / "bin"
    assert (install_dir / "kftui.exe").is_file()
    assert (
        "kftui-stub https://github.com/hcavarsan/kftray/releases/download/v9.9.9/"
        "kftui_windows_x86_64.exe"
    ) in result.stdout
    session_path = next(
        line.removeprefix("SESSION_PATH=")
        for line in result.stdout.splitlines()
        if line.startswith("SESSION_PATH=")
    )
    assert session_path.split(";")[0] == f"{home}\\.local\\bin"

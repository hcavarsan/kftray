import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

INSTALLER = Path(__file__).resolve().parents[1] / "kftui_installer.ps1"

STUBS = r"""
function Invoke-RestMethod { [pscustomobject]@{ tag_name = 'v9.9.9' } }
function Invoke-WebRequest {
    param([string]$Uri, [string]$OutFile)
    Set-Content -Path $OutFile -Value "#!/bin/sh`necho kftui-stub $Uri"
    chmod +x $OutFile
}
function Stop-Request {
    param([System.Management.Automation.PSCmdlet]$Cmdlet, [string]$Message)
    $Cmdlet.ThrowTerminatingError([System.Management.Automation.ErrorRecord]::new(
        [System.Net.Http.HttpRequestException]::new($Message),
        'WebCmdletWebResponseException',
        [System.Management.Automation.ErrorCategory]::InvalidOperation,
        $null
    ))
}
"""

FAILED_RELEASE_LOOKUP = r"""
function Invoke-RestMethod {
    [CmdletBinding()]
    param([string]$Uri)
    Stop-Request $PSCmdlet 'Response status code does not indicate success: 403 (rate limit exceeded).'
}
"""

FAILED_DOWNLOAD = r"""
function Invoke-WebRequest {
    [CmdletBinding()]
    param([string]$Uri, [string]$OutFile)
    Stop-Request $PSCmdlet 'Response status code does not indicate success: 404 (Not Found).'
}
"""

OLD_KFTUI = "#!/bin/sh\necho kftui 0.0.1\n"


def run_installer(
    stubs: str, home: Path, work: Path
) -> subprocess.CompletedProcess[str]:
    command = (
        stubs
        + "try { Invoke-Expression (Get-Content -Raw -LiteralPath $env:KFTUI_INSTALLER) }\n"
        + "finally {\n"
        + '    Write-Output "SESSION_PATH=$env:PATH"\n'
        + '    Write-Output "SESSION_EAP=$ErrorActionPreference"\n'
        + "}\n"
    )
    return subprocess.run(
        ["pwsh", "-NoProfile", "-NonInteractive", "-Command", command],
        cwd=work,
        env={**os.environ, "HOME": str(home), "KFTUI_INSTALLER": str(INSTALLER)},
        capture_output=True,
        text=True,
        check=False,
    )


@pytest.mark.skipif(shutil.which("pwsh") is None, reason="pwsh is not installed")
@pytest.mark.skipif(sys.platform == "win32", reason="the stub binary is a shell script")
def test_installer_puts_kftui_in_install_dir_and_on_path(tmp_path: Path) -> None:
    home = tmp_path / "home"
    home.mkdir()
    work = tmp_path / "work"
    work.mkdir()

    result = run_installer(STUBS, home, work)

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
    assert "SESSION_EAP=Continue" in result.stdout


@pytest.mark.skipif(shutil.which("pwsh") is None, reason="pwsh is not installed")
@pytest.mark.skipif(sys.platform == "win32", reason="the stub binary is a shell script")
@pytest.mark.parametrize(
    "failure",
    [FAILED_RELEASE_LOOKUP, FAILED_DOWNLOAD],
    ids=["release_lookup", "download"],
)
def test_installer_stops_and_keeps_old_kftui_when_a_request_fails(
    tmp_path: Path, failure: str
) -> None:
    home = tmp_path / "home"
    install_dir = home / ".local" / "bin"
    install_dir.mkdir(parents=True)
    (install_dir / "kftui.exe").write_text(OLD_KFTUI)
    work = tmp_path / "work"
    work.mkdir()

    result = run_installer(STUBS + failure, home, work)

    assert result.returncode != 0, result.stdout + result.stderr
    assert "installed successfully" not in result.stdout
    assert "kftui 0.0.1" not in result.stdout
    assert (install_dir / "kftui.exe").read_text() == OLD_KFTUI
    assert "SESSION_EAP=Continue" in result.stdout

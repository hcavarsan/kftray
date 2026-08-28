#Requires -Version 5.1
<#
.SYNOPSIS
  Build kftray Windows release artifacts (exe + NSIS/MSI) into dist/.

.DESCRIPTION
  Runs the same flow as `pnpm tauri build`, then copies:
    - dist/kftray.exe
    - dist/kftray-helper.exe
    - dist/installers/*.msi / *-setup.exe

  Tauri updater signing requires TAURI_SIGNING_PRIVATE_KEY. Local builds without
  that key still produce exe/installers; the script treats that as success only
  when required artifacts exist and are fresher than build start.

.PARAMETER OutDir
  Output directory relative to repo root (default: dist).

.PARAMETER SkipBundles
  Only build/copy kftray.exe (+ helper), skip requiring NSIS/MSI.

.EXAMPLE
  .\hacks\build-windows.ps1
  pnpm run build:win
#>
param(
    [string]$OutDir = "dist",
    [switch]$SkipBundles
)

$ErrorActionPreference = "Stop"

function Write-Step([string]$Message) {
    Write-Host ""
    Write-Host "==> $Message" -ForegroundColor Cyan
}

function Write-Ok([string]$Message) {
    Write-Host "OK  $Message" -ForegroundColor Green
}

function Write-Warn([string]$Message) {
    Write-Host "WARN $Message" -ForegroundColor Yellow
}

function Write-Err([string]$Message) {
    Write-Host "ERR $Message" -ForegroundColor Red
}

function Get-RepoRoot {
    if ($PSScriptRoot) {
        return (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
    }
    return (Get-Location).Path
}

function Get-ReleaseCandidates([string]$RepoRoot) {
    $candidates = New-Object System.Collections.Generic.List[string]

    if ($env:CARGO_TARGET_DIR) {
        $candidates.Add((Join-Path $env:CARGO_TARGET_DIR "release"))
    }
    $candidates.Add((Join-Path $RepoRoot "target\release"))

    $sandboxRoot = Join-Path $env:LOCALAPPDATA "Temp\cursor-sandbox-cache"
    if (Test-Path $sandboxRoot) {
        Get-ChildItem -Path $sandboxRoot -Directory -ErrorAction SilentlyContinue |
            ForEach-Object {
                $candidates.Add((Join-Path $_.FullName "cargo-target\release"))
            }
    }

    return $candidates | Select-Object -Unique
}

function Find-ReleaseDir([string]$RepoRoot) {
    $found = @()
    foreach ($dir in (Get-ReleaseCandidates $RepoRoot)) {
        $exe = Join-Path $dir "kftray.exe"
        if (Test-Path $exe) {
            $found += [PSCustomObject]@{
                Dir   = $dir
                Exe   = Get-Item $exe
                Mtime = (Get-Item $exe).LastWriteTimeUtc
            }
        }
    }

    if (-not $found) {
        return $null
    }

    return ($found | Sort-Object Mtime -Descending | Select-Object -First 1).Dir
}

function Test-FreshFile([string]$Path, [datetime]$SinceUtc) {
    if (-not (Test-Path $Path)) {
        return $false
    }
    return ((Get-Item $Path).LastWriteTimeUtc -ge $SinceUtc)
}

function Clear-StaleReleaseOutputs([string]$RepoRoot) {
    foreach ($dir in (Get-ReleaseCandidates $RepoRoot)) {
        if (-not (Test-Path $dir)) {
            continue
        }
        $exe = Join-Path $dir "kftray.exe"
        if (Test-Path $exe) {
            Write-Warn "Removing stale $exe"
            Remove-Item $exe -Force -ErrorAction SilentlyContinue
        }
        $msiDir = Join-Path $dir "bundle\msi"
        if (Test-Path $msiDir) {
            Get-ChildItem (Join-Path $msiDir "*.msi") -ErrorAction SilentlyContinue |
                ForEach-Object {
                    Write-Warn ("Removing stale {0}" -f $_.FullName)
                    Remove-Item $_.FullName -Force -ErrorAction SilentlyContinue
                }
        }
        $nsisDir = Join-Path $dir "bundle\nsis"
        if (Test-Path $nsisDir) {
            Get-ChildItem (Join-Path $nsisDir "*-setup.exe") -ErrorAction SilentlyContinue |
                ForEach-Object {
                    Write-Warn ("Removing stale {0}" -f $_.FullName)
                    Remove-Item $_.FullName -Force -ErrorAction SilentlyContinue
                }
        }
    }
}

function Test-RequiredArtifacts(
    [string]$ReleaseDir,
    [datetime]$BuildStartedUtc,
    [bool]$RequireBundles
) {
    $exePath = Join-Path $ReleaseDir "kftray.exe"
    if (-not (Test-FreshFile $exePath $BuildStartedUtc)) {
        Write-Err "kftray.exe missing or older than build start: $exePath"
        return $false
    }

    if ($RequireBundles) {
        $msi = Get-ChildItem (Join-Path $ReleaseDir "bundle\msi\*.msi") -ErrorAction SilentlyContinue |
            Where-Object { $_.LastWriteTimeUtc -ge $BuildStartedUtc } |
            Select-Object -First 1
        $nsis = Get-ChildItem (Join-Path $ReleaseDir "bundle\nsis\*-setup.exe") -ErrorAction SilentlyContinue |
            Where-Object { $_.LastWriteTimeUtc -ge $BuildStartedUtc } |
            Select-Object -First 1
        if (-not $msi -and -not $nsis) {
            Write-Err ('No fresh MSI/NSIS under {0}\bundle (required unless -SkipBundles)' -f $ReleaseDir)
            return $false
        }
    }

    return $true
}

$RepoRoot = Get-RepoRoot
Set-Location $RepoRoot

Write-Host "kftray Windows build"
Write-Host "repo: $RepoRoot"

if (-not (Get-Command pnpm -ErrorAction SilentlyContinue)) {
    Write-Err "pnpm not found in PATH"
    exit 1
}
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Err "cargo not found in PATH"
    exit 1
}

Write-Step "Clearing stale release outputs"
Clear-StaleReleaseOutputs $RepoRoot

$buildStarted = (Get-Date).ToUniversalTime()

Write-Step "Building helper + Tauri release (pnpm tauri build)"
$buildLog = Join-Path $env:TEMP ("kftray-build-{0:yyyyMMdd-HHmmss}.log" -f (Get-Date))

# pnpm/tauri may exit 1 on missing updater signing key even after successful bundles
$prevEap = $ErrorActionPreference
$ErrorActionPreference = "Continue"
pnpm tauri build 2>&1 | Tee-Object -FilePath $buildLog
$buildExit = $LASTEXITCODE
$ErrorActionPreference = $prevEap

$logText = ""
if (Test-Path $buildLog) {
    $logText = Get-Content $buildLog -Raw -ErrorAction SilentlyContinue
}

$signingOnlyFailure =
    $buildExit -ne 0 -and
    $logText -match "TAURI_SIGNING_PRIVATE_KEY|public key has been found, but no private key"

Write-Step "Locating and verifying release artifacts"
$releaseDir = Find-ReleaseDir $RepoRoot
if (-not $releaseDir) {
    Write-Err "kftray.exe not found under target/release (or CARGO_TARGET_DIR)"
    if ($logText) {
        Write-Host ($logText.Substring([Math]::Max(0, $logText.Length - 2000)))
    }
    exit 1
}

$artifactsOk = Test-RequiredArtifacts `
    -ReleaseDir $releaseDir `
    -BuildStartedUtc $buildStarted `
    -RequireBundles (-not $SkipBundles)

if ($buildExit -eq 0) {
    if (-not $artifactsOk) {
        Write-Err "Build exit 0 but required fresh artifacts are missing. Log: $buildLog"
        exit 1
    }
    Write-Ok "pnpm tauri build finished (exit 0)"
}
elseif ($signingOnlyFailure -and $artifactsOk) {
    Write-Warn "Build finished with updater-signing error (no TAURI_SIGNING_PRIVATE_KEY)."
    Write-Warn "Fresh exe/installers verified - treating as success for local use."
}
else {
    Write-Err "Build failed (exit $buildExit). Log: $buildLog"
    if (-not $artifactsOk) {
        Write-Err "Required fresh artifacts were not produced."
    }
    if ($logText) {
        Write-Host ($logText.Substring([Math]::Max(0, $logText.Length - 2000)))
    }
    exit $buildExit
}

Write-Ok "release dir: $releaseDir"

$exePath = Join-Path $releaseDir "kftray.exe"
$exeItem = Get-Item $exePath

$outRoot = Join-Path $RepoRoot $OutDir
$outInstallers = Join-Path $outRoot "installers"
New-Item -ItemType Directory -Force -Path $outRoot | Out-Null
New-Item -ItemType Directory -Force -Path $outInstallers | Out-Null

Write-Step "Copying to $outRoot"
Copy-Item $exePath (Join-Path $outRoot "kftray.exe") -Force
$exeMb = [math]::Round($exeItem.Length / 1MB, 1)
Write-Ok ('kftray.exe ({0} MB)' -f $exeMb)

$helperSrc = Join-Path $releaseDir "kftray-helper.exe"
if (-not (Test-Path $helperSrc)) {
    $helperSrc = Join-Path $RepoRoot "target\release\kftray-helper.exe"
}
if (Test-Path $helperSrc) {
    Copy-Item $helperSrc (Join-Path $outRoot "kftray-helper.exe") -Force
    Write-Ok "kftray-helper.exe"
}
else {
    Write-Warn "kftray-helper.exe not found (sidecar may be missing)"
}

$copiedBundles = 0
if (-not $SkipBundles) {
    $msi = Get-ChildItem (Join-Path $releaseDir "bundle\msi\*.msi") -ErrorAction SilentlyContinue |
        Where-Object { $_.LastWriteTimeUtc -ge $buildStarted } |
        Sort-Object LastWriteTimeUtc -Descending |
        Select-Object -First 1
    $nsis = Get-ChildItem (Join-Path $releaseDir "bundle\nsis\*-setup.exe") -ErrorAction SilentlyContinue |
        Where-Object { $_.LastWriteTimeUtc -ge $buildStarted } |
        Sort-Object LastWriteTimeUtc -Descending |
        Select-Object -First 1

    if ($msi) {
        Copy-Item $msi.FullName $outInstallers -Force
        Write-Ok ("installer: {0}" -f $msi.Name)
        $copiedBundles++
    }
    if ($nsis) {
        Copy-Item $nsis.FullName $outInstallers -Force
        Write-Ok ("installer: {0}" -f $nsis.Name)
        $copiedBundles++
    }

    if ($copiedBundles -eq 0) {
        Write-Err ("No fresh MSI/NSIS bundles under {0}\bundle" -f $releaseDir)
        exit 1
    }
}

Write-Host ""
Write-Host "Artifacts:" -ForegroundColor Cyan
Get-ChildItem $outRoot -Recurse -Include *.exe, *.msi |
    ForEach-Object {
        $mb = [math]::Round($_.Length / 1MB, 1)
        Write-Host ('  {0}  ({1} MB)' -f $_.FullName, $mb)
    }

Write-Host ""
Write-Ok ('Done. Log: {0}' -f $buildLog)
exit 0

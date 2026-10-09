# PowerShell script to install kftui on Windows

$INSTALL_DIR = "$HOME\.local\bin"

function Print-Message {
    param (
        [string]$Color,
        [string]$Message
    )
    $timestamp = Get-Date -Format "yyyy-MM-dd HH:mm:ss"
    switch ($Color) {
        "red" { Write-Host "[$timestamp] $Message" -ForegroundColor Red }
        "green" { Write-Host "[$timestamp] $Message" -ForegroundColor Green }
        "yellow" { Write-Host "[$timestamp] $Message" -ForegroundColor Yellow }
        "blue" { Write-Host "[$timestamp] $Message" -ForegroundColor Blue }
        default { Write-Host "[$timestamp] $Message" }
    }
}

function Download-File {
    param (
        [string]$Url,
        [string]$Output
    )
    Print-Message -Color "blue" -Message "Downloading kftui from $Url"
    Invoke-WebRequest -Uri $Url -OutFile $Output
}

function Install-Kftui {
    param (
        [string]$Url,
        [string]$Filename
    )
    Download-File -Url $Url -Output $Filename
    New-Item -ItemType Directory -Force -Path $INSTALL_DIR
    Print-Message -Color "blue" -Message "Installing kftui to $INSTALL_DIR"
    Move-Item -Force -Path $Filename -Destination $INSTALL_DIR
    Print-Message -Color "green" -Message "kftui installed successfully"

    # Add $INSTALL_DIR to PATH if it's not already there
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $userPathEntries = @($userPath -split ";" | Where-Object { $_ })
    if ($userPathEntries -notcontains $INSTALL_DIR) {
        [Environment]::SetEnvironmentVariable("Path", (($userPathEntries + $INSTALL_DIR) -join ";"), "User")
        Print-Message -Color "yellow" -Message "Added $INSTALL_DIR to your user PATH. Restart your terminal to use kftui."
    }
    if (($env:PATH -split ";") -notcontains $INSTALL_DIR) {
        $env:PATH = "$INSTALL_DIR;$env:PATH"
    }

    & "$INSTALL_DIR\kftui.exe" --version
}

# Get the latest release tag from GitHub
$LATEST_RELEASE = (Invoke-RestMethod -Uri https://api.github.com/repos/hcavarsan/kftray/releases/latest).tag_name
$BASE_URL = "https://github.com/hcavarsan/kftray/releases/download/$LATEST_RELEASE"

# Determine architecture
$ARCH = if ([System.Environment]::Is64BitOperatingSystem) { "x86_64" } else { "x86" }

# Set the download URL based on architecture
$URL = "$BASE_URL/kftui_windows_$ARCH.exe"

# Install kftui
Install-Kftui -Url $URL -Filename "kftui.exe"

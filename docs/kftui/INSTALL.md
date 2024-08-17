# `kftui` Installation Guide

## Prerequisites

- Ensure `curl` or `wget` is installed.
- For Windows, PowerShell is required.

## Installation Commands

### Unix-like Systems (Linux, macOS, WSL)

To install `kftui`, run one of the following commands:

Using `curl`:

#### Bash Shell
```sh
bash <(curl -s https://raw.githubusercontent.com/hcavarsan/kftray/main/hacks/kftui_installer.sh)
```

#### Fish Shell
```fish
curl -s https://raw.githubusercontent.com/hcavarsan/kftray/main/hacks/kftui_installer.sh | bash
```

Using `wget`:

#### Bash Shell
```sh
bash <(wget -qO- https://raw.githubusercontent.com/hcavarsan/kftray/main/hacks/kftui_installer.sh)
```

#### Fish Shell
```fish
wget -qO- https://raw.githubusercontent.com/hcavarsan/kftray/main/hacks/kftui_installer.sh | bash
```

### Windows (Native)

Run the following PowerShell command:

```powershell
Invoke-Expression ((New-Object System.Net.WebClient).DownloadString('https://raw.githubusercontent.com/hcavarsan/kftray/main/hacks/kftui_installer.ps1'))
```

### Post-Installation Steps

After installation, restart your terminal and verify the installation:

```sh
kftui
```

---

## Direct Downloads for `kftui` Binaries

Download the latest `kftui` binaries directly from GitHub:

<div align="left">
    <a href="https://github.com/hcavarsan/kftray/releases/latest/download/kftui_macos_universal">
        <img src="https://img.shields.io/badge/macOS-Universal-grey.svg?style=for-the-badge&logo=apple" alt="Download for macOS Universal" />
    </a>
    <a href="https://github.com/hcavarsan/kftray/releases/latest/download/kftui_arm64">
        <img src="https://img.shields.io/badge/Linux-ARM64-grey.svg?style=for-the-badge&logo=linux" alt="Download for Linux ARM64" />
    </a>
    <a href="https://github.com/hcavarsan/kftray/releases/latest/download/kftui_amd64">
        <img src="https://img.shields.io/badge/Linux-AMD64-grey.svg?style=for-the-badge&logo=linux" alt="Download for Linux AMD64" />
    </a>
    <a href="https://github.com/hcavarsan/kftray/releases/latest/download/kftui_x86.exe">
        <img src="https://img.shields.io/badge/Windows-x86-grey.svg?style=for-the-badge&logo=windows" alt="Download for Windows x86" />
    </a>
    <a href="https://github.com/hcavarsan/kftray/releases/latest/download/kftui_x86_64.exe">
        <img src="https://img.shields.io/badge/Windows-x64-grey.svg?style=for-the-badge&logo=windows" alt="Download for Windows x64" />
    </a>
</div>

# Flatpak Distribution

## Prerequisites
```bash
pip install flatpak-builder-tools
```

## Submit to Flathub
```bash
./flathub-submit.sh
```

## Files
- `com.hcavarsan.kftray.yml` - Flatpak manifest (offline build)
- `com.hcavarsan.kftray.appdata.xml` - App metadata  
- `generate-sources.sh` - Generate offline dependencies
- `flathub-submit.sh` - Complete submission script
- `TRAY_ICON_FIX.md` - Required Rust code changes

## Critical Fixes Applied
- **Offline build support** - Uses node-sources.json & cargo-sources.json
- **Proper build commands** - npm ci --offline, cargo --offline
- **Shared modules** - libappindicator for system tray
- **Tray icon fix** - Uses $XDG_DATA_HOME for sandbox compatibility
- **FLATPAK=1 env var** - Allows detection in Rust code

## Automation
- Release workflow generates offline sources automatically
- Flathub External Data Checker updates versions
- Zero maintenance after submission approval

## Installation
```bash
flatpak install flathub com.hcavarsan.kftray
flatpak run com.hcavarsan.kftray
```
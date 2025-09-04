#!/usr/bin/env bash

set -eou pipefail

echo "Generating offline sources for Flatpak build..."

command -v flatpak-node-generator >/dev/null 2>&1 || { 
    echo "flatpak-node-generator required. Install: pip install flatpak-builder-tools"
    exit 1
}

command -v flatpak-cargo-generator.py >/dev/null 2>&1 || {
    echo "flatpak-cargo-generator.py required. Install flatpak-builder-tools"
    exit 1
}

if [ ! -f "../../frontend/package-lock.json" ]; then
    echo "Generating package-lock.json..."
    cd ../../frontend
    npm install
    cd ../hacks/flatpak
fi

echo "Generating node-sources.json..."
rm -rf ../../frontend/node_modules
flatpak-node-generator npm -o node-sources.json ../../frontend/package-lock.json

echo "Generating cargo-sources.json..." 
flatpak-cargo-generator.py -d ../../crates/kftray-tauri/Cargo.lock -o cargo-sources.json

if [ ! -d "shared-modules" ]; then
    echo "Cloning shared-modules..."
    git submodule add https://github.com/flathub/shared-modules.git shared-modules
fi

echo "Sources generated successfully"
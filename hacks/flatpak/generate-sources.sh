#!/usr/bin/env bash

set -eou pipefail

echo "Generating offline sources for Flatpak build..."

# Check if we need to use container for the generators
if command -v toolbox >/dev/null 2>&1; then
    CONTAINER_PREFIX="toolbox run -c flatpakdev"
elif command -v distrobox >/dev/null 2>&1; then
    CONTAINER_PREFIX="distrobox enter flatpak-dev --"
else
    CONTAINER_PREFIX=""
fi

if [ -n "$CONTAINER_PREFIX" ]; then
    $CONTAINER_PREFIX bash -c "export PATH=\$HOME/.local/bin:\$PATH && which flatpak-node-generator" >/dev/null 2>&1 || { 
        echo "flatpak-node-generator required in container"
        exit 1
    }
    $CONTAINER_PREFIX bash -c "export PATH=\$HOME/.local/bin:\$PATH && which flatpak-cargo-generator.py" >/dev/null 2>&1 || {
        echo "flatpak-cargo-generator.py required in container"
        exit 1
    }
else
    command -v flatpak-node-generator >/dev/null 2>&1 || { 
        echo "flatpak-node-generator required. Install: pip install flatpak-builder-tools"
        exit 1
    }
    command -v flatpak-cargo-generator.py >/dev/null 2>&1 || {
        echo "flatpak-cargo-generator.py required. Install flatpak-builder-tools"
        exit 1
    }
fi

if [ ! -f "../../frontend/package-lock.json" ]; then
    echo "Generating package-lock.json..."
    cd ../../frontend
    npm install
    cd ../hacks/flatpak
fi

echo "Generating node-sources.json..."
rm -rf ../../frontend/node_modules
if [ -n "$CONTAINER_PREFIX" ]; then
    $CONTAINER_PREFIX bash -c "export PATH=\$HOME/.local/bin:\$PATH && flatpak-node-generator npm -o node-sources.json ../../frontend/package-lock.json"
else
    flatpak-node-generator npm -o node-sources.json ../../frontend/package-lock.json
fi

echo "Generating cargo-sources.json..." 
if [ -n "$CONTAINER_PREFIX" ]; then
    $CONTAINER_PREFIX bash -c "export PATH=\$HOME/.local/bin:\$PATH && flatpak-cargo-generator.py -d ../../crates/kftray-tauri/Cargo.lock -o cargo-sources.json"
else
    flatpak-cargo-generator.py -d ../../crates/kftray-tauri/Cargo.lock -o cargo-sources.json
fi

if [ ! -d "shared-modules" ]; then
    echo "Cloning shared-modules..."
    git submodule add https://github.com/flathub/shared-modules.git shared-modules
fi

echo "Sources generated successfully"
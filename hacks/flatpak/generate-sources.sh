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
    # Check if we need to install cargo generator manually
    if ! $CONTAINER_PREFIX bash -c "export PATH=\$HOME/.local/bin:\$PATH && (which flatpak-cargo-generator.py || which flatpak-cargo-generator)" >/dev/null 2>&1; then
        echo "Installing flatpak-cargo-generator manually..."
        $CONTAINER_PREFIX bash -c "
            export PATH=\$HOME/.local/bin:\$PATH
            cd /tmp/flatpak-builder-tools/cargo
            python3 flatpak-cargo-generator.py --help >/dev/null 2>&1 && {
                echo 'Using direct python script'
                ln -sf /tmp/flatpak-builder-tools/cargo/flatpak-cargo-generator.py \$HOME/.local/bin/flatpak-cargo-generator.py
            }
        "
    fi
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

# Create a comprehensive package-lock.json with ALL dependencies needed
echo "Creating comprehensive package-lock.json with all dependencies..."

# Go to frontend directory and create a temporary comprehensive package.json
cd ../../frontend

# Backup original package.json
cp package.json package.json.backup

# Add all root-level devDependencies to frontend package.json for comprehensive lockfile
cat package.json.backup | jq '.devDependencies += {
  "@codecov/vite-plugin": "1.9.1",
  "@eslint/eslintrc": "^3.3.1", 
  "@eslint/js": "^9.34.0",
  "@tauri-apps/cli": "^2.8.4"
}' > package.json

# Clean and regenerate comprehensive package-lock.json
rm -rf node_modules package-lock.json
echo "Generating comprehensive package-lock.json..."
npm install --package-lock-only --legacy-peer-deps

# Restore original package.json
mv package.json.backup package.json

cd ../hacks/flatpak

echo "Generating node-sources.json from comprehensive package-lock.json..."
# Clean any existing node_modules before generating sources  
rm -rf ../../frontend/node_modules

if [ -n "$CONTAINER_PREFIX" ]; then
    $CONTAINER_PREFIX bash -c "export PATH=\$HOME/.local/bin:\$PATH && flatpak-node-generator npm -o node-sources.json ../../frontend/package-lock.json"
else
    flatpak-node-generator npm -o node-sources.json ../../frontend/package-lock.json
fi

echo "Generating cargo-sources.json..." 
if [ -n "$CONTAINER_PREFIX" ]; then
    $CONTAINER_PREFIX bash -c "
        export PATH=\$HOME/.local/bin:\$PATH
        if which flatpak-cargo-generator.py >/dev/null 2>&1; then
            flatpak-cargo-generator.py -d ../../Cargo.lock -o cargo-sources.json
        else
            flatpak-cargo-generator -d ../../Cargo.lock -o cargo-sources.json
        fi
    "
else
    if command -v flatpak-cargo-generator.py >/dev/null 2>&1; then
        flatpak-cargo-generator.py -d ../../Cargo.lock -o cargo-sources.json
    else
        flatpak-cargo-generator -d ../../Cargo.lock -o cargo-sources.json
    fi
fi

if [ ! -d "shared-modules" ]; then
    echo "Cloning shared-modules..."
    git submodule add https://github.com/flathub/shared-modules.git shared-modules
fi

echo "Sources generated successfully"
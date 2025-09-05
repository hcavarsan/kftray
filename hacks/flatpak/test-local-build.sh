#!/usr/bin/env bash

set -eou pipefail

echo "🔧 Installing Flatpak build dependencies..."
# Check if we're on Fedora Silverblue/Kinoite
if command -v rpm-ostree >/dev/null 2>&1; then
    echo "Detected Fedora Silverblue/Kinoite - using rpm-ostree..."
    # Install packages that aren't already present
    PACKAGES_TO_INSTALL=()
    for pkg in flatpak flatpak-builder git python3-pip; do
        if ! rpm -q "$pkg" >/dev/null 2>&1; then
            PACKAGES_TO_INSTALL+=("$pkg")
        fi
    done
    
    if [ ${#PACKAGES_TO_INSTALL[@]} -gt 0 ]; then
        echo "Installing missing packages: ${PACKAGES_TO_INSTALL[*]}"
        sudo rpm-ostree install --apply-live "${PACKAGES_TO_INSTALL[@]}"
    else
        echo "All required packages already installed"
    fi
else
    echo "Using traditional package manager..."
    sudo dnf install -y flatpak flatpak-builder git python3-pip
fi

echo "📦 Installing flatpak-builder-tools..."
# Check if toolbox is available for container isolation
if command -v toolbox >/dev/null 2>&1; then
    echo "Using toolbox container for Python packages..."
    if ! toolbox list | grep -q flatpakdev; then
        toolbox create flatpakdev fedora-toolbox:latest
    fi

    toolbox run -c flatpakdev bash -c "
        dnf install -y python3-pip git &&
        pip3 install aiohttp toml
    "
elif command -v distrobox >/dev/null 2>&1; then
    echo "Using distrobox container for Python packages..."
    if ! distrobox list | grep -q flatpak-dev; then
        distrobox create -n flatpak-dev -i fedora:latest
    fi

    distrobox enter flatpak-dev -- bash -c "
        dnf install -y python3-pip git &&
        pip3 install aiohttp toml
    "
else
    echo "No container tool available, installing directly..."
    pip3 install --user aiohttp toml
fi

echo "🏗️ Setting up Flatpak runtime..."
flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak install -y flathub org.gnome.Platform//46 org.gnome.Sdk//46 org.freedesktop.Sdk.Extension.rust-stable//23.08 org.freedesktop.Sdk.Extension.node20//23.08

echo "📁 Setting up shared-modules..."
if [ ! -d "shared-modules" ]; then
    git clone https://github.com/flathub/shared-modules.git
fi

echo "🛠️ Setting up flatpak-builder-tools..."
if command -v toolbox >/dev/null 2>&1; then
    CONTAINER_RUN="toolbox run -c flatpakdev"
elif command -v distrobox >/dev/null 2>&1; then
    CONTAINER_RUN="distrobox enter flatpak-dev --"
else
    CONTAINER_RUN=""
fi

if [ -n "$CONTAINER_RUN" ]; then
    $CONTAINER_RUN bash -c "
        if [ ! -d '/tmp/flatpak-builder-tools' ]; then
            cd /tmp &&
            git clone https://github.com/flatpak/flatpak-builder-tools.git
        fi &&
        pip3 install /tmp/flatpak-builder-tools/node
    "
else
    if [ ! -d "flatpak-builder-tools" ]; then
        git clone https://github.com/flatpak/flatpak-builder-tools.git
    fi
    pip3 install --user ./flatpak-builder-tools/node
fi

echo "📋 Generating offline sources..."
./generate-sources.sh

echo "🔨 Building Flatpak..."
flatpak-builder --disable-rofiles-fuse --force-clean --install-deps-from=flathub --repo=kftray-flatpak-repo build-dir com.hcavarsan.kftray.yml

echo "📦 Creating bundle..."
VERSION="${1:-test}"
flatpak build-bundle --arch=x86_64 kftray-flatpak-repo com.hcavarsan.kftray-${VERSION}.flatpak com.hcavarsan.kftray

echo "✅ Build complete! Bundle created: com.hcavarsan.kftray-${VERSION}.flatpak"
echo ""
echo "🧪 To test the build:"
echo "  flatpak install --user --bundle com.hcavarsan.kftray-${VERSION}.flatpak"
echo "  flatpak run com.hcavarsan.kftray"
echo ""
echo "🗑️ To uninstall after testing:"
echo "  flatpak uninstall --user com.hcavarsan.kftray"
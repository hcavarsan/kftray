#!/usr/bin/env bash

set -eou pipefail

echo "🔧 Installing Flatpak build dependencies..."
# Check if we're on Fedora Silverblue/Kinoite
if command -v rpm-ostree >/dev/null 2>&1; then
    echo "Detected Fedora Silverblue/Kinoite - using rpm-ostree..."
    sudo rpm-ostree install --apply-live flatpak flatpak-builder git python3-pip
else
    echo "Using traditional package manager..."
    sudo dnf install -y flatpak flatpak-builder git python3-pip
fi

echo "📦 Installing flatpak-builder-tools in toolbx container..."
# Use toolbx for Python package isolation
if ! toolbx list | grep -q flatpak-dev; then
    toolbx create -c flatpak-dev fedora-toolbox:latest
fi

toolbx run -c flatpak-dev bash -c "
    dnf install -y python3-pip git &&
    pip3 install aiohttp toml
"

echo "🏗️ Setting up Flatpak runtime..."
flatpak remote-add --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak install -y flathub org.gnome.Platform//46 org.gnome.Sdk//46 org.freedesktop.Sdk.Extension.rust-stable//23.08 org.freedesktop.Sdk.Extension.node20//23.08

echo "📁 Setting up shared-modules..."
if [ ! -d "shared-modules" ]; then
    git clone https://github.com/flathub/shared-modules.git
fi

echo "🛠️ Setting up flatpak-builder-tools in toolbx..."
toolbx run -c flatpak-dev bash -c "
    if [ ! -d '/tmp/flatpak-builder-tools' ]; then
        cd /tmp &&
        git clone https://github.com/flatpak/flatpak-builder-tools.git
    fi &&
    pip3 install /tmp/flatpak-builder-tools/node
"

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
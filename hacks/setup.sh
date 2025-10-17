#!/usr/bin/env bash
set -e

echo "Setting up development environment..."
echo ""

if [[ "$OSTYPE" == "linux-gnu"* ]]; then
    echo "Detected Linux"

    if [ -f /etc/os-release ]; then
        . /etc/os-release
        DISTRO=$ID
    fi

    echo "Installing system dependencies..."

    case $DISTRO in
        ubuntu|debian)
            echo "Installing dependencies for Debian/Ubuntu..."
            sudo apt update
            sudo apt install -y \
                libwebkit2gtk-4.1-dev \
                build-essential \
                curl \
                wget \
                file \
                libxdo-dev \
                libssl-dev \
                libayatana-appindicator3-dev \
                librsvg2-dev
            ;;
        fedora)
            echo "Installing dependencies for Fedora..."
            sudo dnf check-update
            sudo dnf install -y \
                webkit2gtk4.1-devel \
                openssl-devel \
                curl \
                wget \
                file \
                libappindicator-gtk3-devel \
                librsvg2-devel
            sudo dnf group install -y "C Development Tools and Libraries"
            ;;
        arch|manjaro)
            echo "Installing dependencies for Arch Linux..."
            sudo pacman -Syu --needed \
                webkit2gtk-4.1 \
                base-devel \
                curl \
                wget \
                file \
                openssl \
                appmenu-gtk-module \
                gtk3 \
                libappindicator-gtk3 \
                librsvg \
                libvips
            ;;
        opensuse*)
            echo "Installing dependencies for openSUSE..."
            sudo zypper refresh
            sudo zypper install -y \
                webkit2gtk3-devel \
                libopenssl-devel \
                curl \
                wget \
                file \
                libappindicator3-1 \
                librsvg-devel
            sudo zypper install -t pattern devel_basis
            ;;
        *)
            echo "Warning: Unknown Linux distribution. Please install dependencies manually."
            echo "See: https://v2.tauri.app/start/prerequisites/"
            ;;
    esac

elif [[ "$OSTYPE" == "darwin"* ]]; then
    echo "Detected macOS"

    if ! xcode-select -p &> /dev/null; then
        echo "Installing Xcode Command Line Tools..."
        xcode-select --install
        echo "Please complete the Xcode installation and run this script again."
        exit 1
    fi

    if ! command -v brew &> /dev/null; then
        echo "Installing Homebrew..."
        /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
    fi

    echo "macOS dependencies are managed by Xcode."

elif [[ "$OSTYPE" == "msys" || "$OSTYPE" == "win32" ]]; then
    echo "Detected Windows"
    echo ""
    echo "Please ensure you have installed:"
    echo "  1. Microsoft C++ Build Tools"
    echo "     Download: https://visualstudio.microsoft.com/visual-cpp-build-tools/"
    echo "  2. WebView2 Runtime"
    echo "     Download: https://developer.microsoft.com/en-us/microsoft-edge/webview2/"
    echo ""
    echo "After installing these, re-run this setup script."
fi


echo ""
echo "Installing project dependencies..."

mise install
pnpm install

echo ""
echo "========================================="
echo "Setup complete!"
echo "========================================="
echo ""
echo "Next steps:"
echo "  2. Run 'mise run dev' to start development"
echo ""

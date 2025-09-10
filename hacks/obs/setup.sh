#!/usr/bin/env bash
set -euo pipefail

# Detect if we're running in a container as root
if [ "$(id -u)" = "0" ]; then
    # Running as root, no sudo needed
    if command -v zypper &> /dev/null; then
        # openSUSE/SUSE - tools should already be installed in our container
        echo "Using existing OBS tools in openSUSE container"
    elif command -v apt-get &> /dev/null; then
        # Debian/Ubuntu
        apt-get update
        apt-get install -y osc obs-build
    else
        echo "Unsupported package manager"
        exit 1
    fi
else
    # Running as regular user, use sudo
    if command -v apt-get &> /dev/null; then
        sudo apt-get update
        sudo apt-get install -y osc obs-build
    else
        echo "Unsupported package manager for non-root user"
        exit 1
    fi
fi

mkdir -p ~/.config/osc
cat > ~/.config/osc/oscrc << EOF
[general]
apiurl = https://api.opensuse.org

[https://api.opensuse.org]
user = ${OBS_USER}
pass = ${OBS_PASSWORD}
EOF
chmod 600 ~/.config/osc/oscrc

echo "OBS setup complete"
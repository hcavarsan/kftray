#!/usr/bin/env bash
set -euo pipefail

sudo apt-get update
sudo apt-get install -y osc obs-build

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
#!/usr/bin/env bash
set -euo pipefail

echo "Setting up OBS tools in container..."

if [ -z "${OBS_USER:-}" ] || [ -z "${OBS_PASSWORD:-}" ]; then
    echo "Error: OBS_USER and OBS_PASSWORD environment variables must be set"
    exit 1
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

echo "OBS configuration created for user: ${OBS_USER}"
echo "Testing OBS connection..."

if osc ls > /dev/null 2>&1; then
    echo "✅ OBS connection successful!"
else
    echo "❌ OBS connection failed. Check credentials."
    exit 1
fi
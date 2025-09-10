#!/usr/bin/env bash
set -euo pipefail

if ! command -v docker &> /dev/null; then
    echo "Docker is required but not installed."
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "Building test container..."
docker build -f Dockerfile.test -t obs-test .

echo "Running OBS test container..."
echo "Available commands inside container:"
echo "  - ./setup.sh     # Setup OBS tools (will fail without credentials)"
echo "  - ./publish.sh   # Test publish (dry-run mode)"
echo "  - osc --help     # OBS command help"
echo ""

docker run -it --rm \
    -v "${SCRIPT_DIR}:/workspace/obs" \
    -e OBS_USER="${OBS_USER:-test}" \
    -e OBS_PASSWORD="${OBS_PASSWORD:-test}" \
    -e VERSION="${VERSION:-1.0.0-test}" \
    obs-test \
    bash -c "cd /workspace/obs && bash"
#!/usr/bin/env bash

set -eou pipefail

command -v gh >/dev/null 2>&1 || { echo "GitHub CLI required"; exit 1; }
command -v flatpak-node-generator >/dev/null 2>&1 || { echo "flatpak-builder-tools required"; exit 1; }

[[ -f "com.hcavarsan.kftray.yml" && -f "com.hcavarsan.kftray.appdata.xml" ]] || { echo "Missing manifest files"; exit 1; }

echo "Generating offline sources..."
./generate-sources.sh

echo "Preparing Flathub submission..."
if [ -d "flathub" ]; then
    cd flathub
    git fetch origin
    git checkout new-pr
    git pull origin new-pr
else
    gh repo fork flathub/flathub --clone
    cd flathub
    git checkout --track origin/new-pr
fi

BRANCH_NAME="add-kftray-$(date +%Y%m%d)"
git checkout -b "$BRANCH_NAME" new-pr

mkdir -p com.hcavarsan.kftray
cp ./com.hcavarsan.kftray.yml com.hcavarsan.kftray/
cp ./com.hcavarsan.kftray.appdata.xml com.hcavarsan.kftray/
cp ./node-sources.json com.hcavarsan.kftray/
cp ./cargo-sources.json com.hcavarsan.kftray/

git submodule add https://github.com/flathub/shared-modules.git shared-modules 2>/dev/null || true

git add com.hcavarsan.kftray/ shared-modules/
git commit -m "Add com.hcavarsan.kftray

Cross-platform system tray application for managing kubectl port-forward commands.
Built with Tauri, supports offline builds.

License: GPL-3.0
Website: https://kftray.app"

git push origin "$BRANCH_NAME"

gh pr create \
    --base new-pr \
    --title "Add com.hcavarsan.kftray" \
    --body "Cross-platform system tray application for managing kubectl port-forward commands.

**Features**:
- System tray integration
- Multiple Kubernetes contexts support  
- Configuration sharing
- Offline build support

**Technical**:
- App ID: com.hcavarsan.kftray
- License: GPL-3.0
- Runtime: GNOME 46
- Repository: https://github.com/hcavarsan/kftray

Original author submission with proper offline dependencies."

echo "Submission complete: gh pr view --web"
cd ..
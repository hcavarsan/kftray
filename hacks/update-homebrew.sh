#!/usr/bin/env bash

set -euo pipefail

REPO="${1:-hcavarsan/kftray}"
VERSION="${2:-v0.15.2}"
TAP_REPO="${3:-hcavarsan/homebrew-kftray}"
GH_TOKEN="${4:-}"

VERSION_NO_V="${VERSION#v}"
TEMP_DIR="homebrew-tap-test"

MAC_FILE="kftray_universal.app.tar.gz"
LINUX_FILE="kftray_linux_amd64.AppImage"

MAC_URL="https://github.com/${REPO}/releases/download/${VERSION}/kftray_universal.app.tar.gz"
LINUX_URL="https://github.com/${REPO}/releases/download/${VERSION}/kftray_${VERSION_NO_V}_amd64.AppImage"
INITIAL_DIR="$(pwd)"

cleanup() {
	echo "Cleaning up..."
	cd "$INITIAL_DIR" || return
	if [ -f "$MAC_FILE" ]; then rm -f "$MAC_FILE"; fi
	if [ -f "$LINUX_FILE" ]; then rm -f "$LINUX_FILE"; fi
	if [ -d "$TEMP_DIR" ]; then rm -rf "$TEMP_DIR"; fi
	echo "Cleanup completed"
}
trap cleanup EXIT INT TERM

download_and_hash() {
	local url="$1"
	local file="$2"

	echo "Downloading: $url" >&2
	if ! curl -L --fail -H "Authorization: token ${GH_TOKEN}" -o "$file" "$url"; then
		echo "Error: Failed to download $url" >&2
		return 1
	fi

	shasum -a 256 "$file" | awk '{ print $1 }'
}

update_formula() {
	local file="$1"
	local version="$2"
	local url="$3"
	local hash="$4"
	local temp_file="${file}.tmp"

	cp "$file" "$temp_file"
	perl -pi -e "s|version \".*\"|version \"$version\"|g" "$temp_file"
	perl -pi -e "s|sha256 \".*\"|sha256 \"$hash\"|g" "$temp_file"
	perl -pi -e "s|url \".*\"|url \"$url\"|g" "$temp_file"
	mv "$temp_file" "$file"
}

main() {
	echo "Cloning Homebrew tap..."
	git clone "https://${GH_TOKEN}@github.com/${TAP_REPO}.git" "$TEMP_DIR"
	cd "$TEMP_DIR" || exit 1

	echo "Calculating hashes..."
	local mac_hash linux_hash
	mac_hash=$(download_and_hash "$MAC_URL" "../$MAC_FILE")
	linux_hash=$(download_and_hash "$LINUX_URL" "../$LINUX_FILE")

	echo "Hashes calculated:"
	echo "macOS: $mac_hash"
	echo "Linux: $linux_hash"

	echo "Updating formulas..."
	update_formula "Casks/kftray.rb" "$VERSION_NO_V" "$MAC_URL" "$mac_hash"
	update_formula "Formula/kftray-linux.rb" "$VERSION_NO_V" "$LINUX_URL" "$linux_hash"

	git config user.name "github-actions[bot]"
	git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
	git add Casks/kftray.rb Formula/kftray-linux.rb
	git commit -m "Update kftray to version ${VERSION}"
	git push
}

main "$@"

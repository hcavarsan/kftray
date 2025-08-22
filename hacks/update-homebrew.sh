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
KFTUI_MAC_FILE="kftui_macos_universal"
KFTUI_LINUX_AMD64_FILE="kftui_linux_amd64"
KFTUI_LINUX_ARM64_FILE="kftui_linux_arm64"

MAC_URL="https://github.com/${REPO}/releases/download/${VERSION}/kftray_universal.app.tar.gz"
LINUX_URL="https://github.com/${REPO}/releases/download/${VERSION}/kftray_${VERSION_NO_V}_amd64.AppImage"
KFTUI_MAC_URL="https://github.com/${REPO}/releases/download/${VERSION}/kftui_macos_universal"
KFTUI_LINUX_AMD64_URL="https://github.com/${REPO}/releases/download/${VERSION}/kftui_linux_amd64"
KFTUI_LINUX_ARM64_URL="https://github.com/${REPO}/releases/download/${VERSION}/kftui_linux_arm64"
INITIAL_DIR="$(pwd)"

cleanup() {
	echo "Cleaning up..."
	cd "$INITIAL_DIR" || return
	if [ -f "$MAC_FILE" ]; then rm -f "$MAC_FILE"; fi
	if [ -f "$LINUX_FILE" ]; then rm -f "$LINUX_FILE"; fi
	if [ -f "$KFTUI_MAC_FILE" ]; then rm -f "$KFTUI_MAC_FILE"; fi
	if [ -f "$KFTUI_LINUX_AMD64_FILE" ]; then rm -f "$KFTUI_LINUX_AMD64_FILE"; fi
	if [ -f "$KFTUI_LINUX_ARM64_FILE" ]; then rm -f "$KFTUI_LINUX_ARM64_FILE"; fi
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

update_kftui_formula() {
	local file="$1"
	local version="$2"
	local mac_url="$3"
	local mac_hash="$4"
	local linux_amd64_url="$5"
	local linux_amd64_hash="$6"
	local linux_arm64_url="$7"
	local linux_arm64_hash="$8"
	local temp_file="${file}.tmp"

	cp "$file" "$temp_file"

	perl -pi -e "s|VERSION_PLACEHOLDER|$version|g" "$temp_file"
	perl -pi -e "s|SHA256_PLACEHOLDER|$mac_hash|" "$temp_file"

	sed -i.bak "s|https://github.com/hcavarsan/kftray/releases/download/VERSION_PLACEHOLDER/kftui_macos_universal|$mac_url|g" "$temp_file"
	sed -i.bak "s|https://github.com/hcavarsan/kftray/releases/download/VERSION_PLACEHOLDER/kftui_linux_amd64|$linux_amd64_url|g" "$temp_file"
	sed -i.bak "s|https://github.com/hcavarsan/kftray/releases/download/VERSION_PLACEHOLDER/kftui_linux_arm64|$linux_arm64_url|g" "$temp_file"

	local first_sha_done=false
	local second_sha_done=false

	while IFS= read -r line; do
		if [[ "$line" == *"SHA256_PLACEHOLDER"* ]] && [[ "$first_sha_done" == false ]]; then
			echo "${line/SHA256_PLACEHOLDER/$mac_hash}"
			first_sha_done=true
		elif [[ "$line" == *"SHA256_PLACEHOLDER"* ]] && [[ "$second_sha_done" == false ]]; then
			echo "${line/SHA256_PLACEHOLDER/$linux_amd64_hash}"
			second_sha_done=true
		elif [[ "$line" == *"SHA256_PLACEHOLDER"* ]]; then
			echo "${line/SHA256_PLACEHOLDER/$linux_arm64_hash}"
		else
			echo "$line"
		fi
	done < "$temp_file" > "${temp_file}.new"

	mv "${temp_file}.new" "$temp_file"
	rm -f "${temp_file}.bak"
	mv "$temp_file" "$file"
}

main() {
	echo "Cloning Homebrew tap..."
	git clone "https://${GH_TOKEN}@github.com/${TAP_REPO}.git" "$TEMP_DIR"
	cd "$TEMP_DIR" || exit 1

	echo "Calculating hashes for kftray..."
	local mac_hash linux_hash
	mac_hash=$(download_and_hash "$MAC_URL" "../$MAC_FILE")
	linux_hash=$(download_and_hash "$LINUX_URL" "../$LINUX_FILE")

	echo "Calculating hashes for kftui..."
	local kftui_mac_hash kftui_linux_amd64_hash kftui_linux_arm64_hash
	kftui_mac_hash=$(download_and_hash "$KFTUI_MAC_URL" "../$KFTUI_MAC_FILE")
	kftui_linux_amd64_hash=$(download_and_hash "$KFTUI_LINUX_AMD64_URL" "../$KFTUI_LINUX_AMD64_FILE")
	kftui_linux_arm64_hash=$(download_and_hash "$KFTUI_LINUX_ARM64_URL" "../$KFTUI_LINUX_ARM64_FILE")

	echo "Hashes calculated:"
	echo "kftray macOS: $mac_hash"
	echo "kftray Linux: $linux_hash"
	echo "kftui macOS: $kftui_mac_hash"
	echo "kftui Linux AMD64: $kftui_linux_amd64_hash"
	echo "kftui Linux ARM64: $kftui_linux_arm64_hash"

	echo "Updating formulas..."
	update_formula "Casks/kftray.rb" "$VERSION_NO_V" "$MAC_URL" "$mac_hash"
	update_formula "Formula/kftray-linux.rb" "$VERSION_NO_V" "$LINUX_URL" "$linux_hash"
	update_kftui_formula "Formula/kftui.rb" "$VERSION" "$KFTUI_MAC_URL" "$kftui_mac_hash" "$KFTUI_LINUX_AMD64_URL" "$kftui_linux_amd64_hash" "$KFTUI_LINUX_ARM64_URL" "$kftui_linux_arm64_hash"

	git config user.name "github-actions[bot]"
	git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
	git add Casks/kftray.rb Formula/kftray-linux.rb Formula/kftui.rb
	git commit -m "Update kftray to version ${VERSION} and kftui to version ${VERSION}"
	git push
}

main "$@"

#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VERSION="${VERSION:-${GITHUB_REF#refs/tags/v}}"
OBS_PROJECT="home:${OBS_USER}:kftray"
PACKAGES=("kftui" "kftray")

# Version validation function
validate_version() {
    local version="$1"
    
    # Remove leading 'v' if present
    version="${version#v}"
    
    # Check if version matches semantic versioning pattern
    # Accepts patterns like: 1.2.3, 1.2.3-beta.1, 1.2.3-alpha, etc.
    if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.-]+)?$ ]]; then
        echo "Error: Invalid version format: '$version'"
        echo "Expected format: X.Y.Z or X.Y.Z-suffix (e.g., 1.2.3, 1.2.3-beta.1)"
        return 1
    fi
    
    echo "Version validated: $version"
    return 0
}

# Retry function for OBS operations
retry_command() {
    local max_attempts=3
    local delay=5
    local attempt=1
    local command="$*"
    
    while [ $attempt -le $max_attempts ]; do
        echo "Attempt $attempt/$max_attempts: $command"
        if eval "$command"; then
            return 0
        else
            echo "Command failed on attempt $attempt"
            if [ $attempt -lt $max_attempts ]; then
                echo "Waiting ${delay}s before retry..."
                sleep $delay
                delay=$((delay * 2))  # exponential backoff
            fi
            attempt=$((attempt + 1))
        fi
    done
    
    echo "Command failed after $max_attempts attempts: $command"
    return 1
}

# Generate checksums for binary downloads
generate_checksums() {
    local package_name="$1"
    local temp_dir="checksums-${package_name}"
    
    mkdir -p "$temp_dir"
    cd "$temp_dir"
    
    local base_url="https://github.com/hcavarsan/kftray/releases/download/v${VERSION}"
    local amd64_file=""
    local arm64_file=""
    local sha256_amd64=""
    local sha256_arm64=""
    
    case "$package_name" in
        "kftui")
            amd64_file="kftui_linux_amd64"
            arm64_file="kftui_linux_arm64"
            ;;
        "kftray")
            amd64_file="kftray_${VERSION}_amd64.AppImage"
            arm64_file="kftray_${VERSION}_arm64.AppImage"
            ;;
    esac
    
    echo "Calculating checksums for ${package_name}..."
    
    # Download and calculate SHA256 for amd64
    if curl -sL "${base_url}/${amd64_file}" -o "${amd64_file}"; then
        sha256_amd64=$(sha256sum "${amd64_file}" | cut -d' ' -f1)
        echo "AMD64 SHA256: $sha256_amd64"
    else
        echo "Warning: Could not download ${amd64_file}, using fallback"
        sha256_amd64="SKIP"
    fi
    
    # Download and calculate SHA256 for arm64
    if curl -sL "${base_url}/${arm64_file}" -o "${arm64_file}"; then
        sha256_arm64=$(sha256sum "${arm64_file}" | cut -d' ' -f1)
        echo "ARM64 SHA256: $sha256_arm64"
    else
        echo "Warning: Could not download ${arm64_file}, using fallback"
        sha256_arm64="SKIP"
    fi
    
    cd ..
    rm -rf "$temp_dir"
    
    # Export checksums for template substitution
    export SHA256_AMD64="$sha256_amd64"
    export SHA256_ARM64="$sha256_arm64"
}

generate_repos_xml() {
    while IFS=: read -r name project repo archs; do
        [[ "$name" =~ ^#.*$ ]] && continue
        [[ -z "$name" ]] && continue
        
        echo "  <repository name=\"$name\">"
        echo "    <path project=\"$project\" repository=\"$repo\"/>"
        
        IFS=',' read -ra ARCH_ARRAY <<< "$archs"
        for arch in "${ARCH_ARRAY[@]}"; do
            echo "    <arch>$arch</arch>"
        done
        
        echo "  </repository>"
    done < "${SCRIPT_DIR}/distros.conf"
}

create_project() {
    if ! retry_command "osc meta prj \"${OBS_PROJECT}\" &>/dev/null"; then
        cat > project.xml << EOF
<project name="${OBS_PROJECT}">
  <title>KFtray</title>
  <description>Kubernetes port-forwarding manager</description>
$(generate_repos_xml)
</project>
EOF
        retry_command "osc meta prj -F project.xml \"${OBS_PROJECT}\""
        rm project.xml
    fi
}

prepare_package() {
    local package_name="$1"
    mkdir -p "package-${package_name}"
    
    # Generate checksums for PKGBUILD files
    generate_checksums "${package_name}"
    
    for template in "${SCRIPT_DIR}/${package_name}/templates"/*; do
        [ -f "$template" ] || continue
        
        filename=$(basename "$template")
        # Substitute VERSION, SHA256_AMD64, and SHA256_ARM64 placeholders
        sed -e "s/{{VERSION}}/${VERSION}/g" \
            -e "s/{{SHA256_AMD64}}/${SHA256_AMD64}/g" \
            -e "s/{{SHA256_ARM64}}/${SHA256_ARM64}/g" \
            "$template" > "package-${package_name}/$filename"
    done
    
    [ -f "package-${package_name}/debian-rules" ] && chmod +x "package-${package_name}/debian-rules"
}

upload_package() {
    local package_name="$1"
    
    if ! retry_command "osc co \"${OBS_PROJECT}\" \"${package_name}\" &>/dev/null"; then
        retry_command "osc mkpac \"${OBS_PROJECT}\" \"${package_name}\""
    fi
    retry_command "osc co \"${OBS_PROJECT}\" \"${package_name}\""
    
    cd "${OBS_PROJECT}/${package_name}"
    cp -r "../../package-${package_name}"/* .
    
    if [ -f debian-control ] || [ -f debian-rules ]; then
        mkdir -p debian
        [ -f debian-control ] && mv debian-control debian/control
        [ -f debian-rules ] && mv debian-rules debian/rules
    fi
    
    osc add ./* 2>/dev/null || true
    retry_command "osc commit -m \"Update to version ${VERSION}\""
    
    echo "Build status for ${package_name}:"
    retry_command "osc results"
    cd ../..
}

main() {
    # Validate version format before proceeding
    validate_version "${VERSION}"
    
    create_project
    
    for package in "${PACKAGES[@]}"; do
        echo "Processing package: ${package}"
        prepare_package "${package}"
        upload_package "${package}"
        echo "Published ${package} to OBS: https://build.opensuse.org/package/show/${OBS_PROJECT}/${package}"
        echo
    done
}

main
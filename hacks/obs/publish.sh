#!/usr/bin/env bash
set -euo pipefail

# OBS Package Publisher
# Publishes kftui and kftray packages to OpenSUSE Build Service
# 
# Usage:
#   export VERSION=1.2.3 OBS_USER=username OBS_PASSWORD=password
#   ./publish.sh [package_name...]
#
# Examples:
#   ./publish.sh                    # Publish all packages
#   ./publish.sh kftui              # Publish only kftui
#   ./publish.sh kftui kftray       # Publish specific packages
#
# Environment Variables:
#   VERSION      - Version to publish (required)
#   OBS_USER     - OBS username (required)
#   OBS_PASSWORD - OBS password (required)
#   OBS_PROJECT  - OBS project name (optional, defaults to home:${OBS_USER}:kftray)

show_usage() {
    echo "OBS Package Publisher"
    echo "Usage: $0 [package_name...]"
    echo ""
    echo "Environment variables required:"
    echo "  VERSION      - Version to publish (e.g., 1.2.3)"
    echo "  OBS_USER     - OpenSUSE Build Service username"
    echo "  OBS_PASSWORD - OpenSUSE Build Service password"
    echo ""
    echo "Optional:"
    echo "  OBS_PROJECT  - Project name (default: home:\${OBS_USER}:kftray)"
    echo ""
    echo "Examples:"
    echo "  export VERSION=1.2.3 OBS_USER=myuser OBS_PASSWORD=mypass"
    echo "  $0                    # Publish all packages"
    echo "  $0 kftui              # Publish only kftui"
    echo "  $0 kftui kftray       # Publish both packages"
    echo ""
    echo "Available packages: kftui, kftray"
}

# Validate required environment variables
if [ "${1:-}" = "--help" ] || [ "${1:-}" = "-h" ]; then
    show_usage
    exit 0
fi

if [ -z "${VERSION:-}" ]; then
    echo "Error: VERSION environment variable is required"
    echo "Example: export VERSION=1.2.3"
    show_usage
    exit 1
fi

if [ -z "${OBS_USER:-}" ]; then
    echo "Error: OBS_USER environment variable is required"
    echo "Example: export OBS_USER=myusername"
    show_usage
    exit 1
fi

if [ -z "${OBS_PASSWORD:-}" ]; then
    echo "Error: OBS_PASSWORD environment variable is required"
    echo "Example: export OBS_PASSWORD=mypassword"
    show_usage
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VERSION="${VERSION:-${GITHUB_REF#refs/tags/v}}"
OBS_PROJECT="${OBS_PROJECT:-home:${OBS_USER}:kftray}"
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
# Generate dynamic changelog with release notes from GitHub
generate_changelog() {
    local package_name="$1"
    local release_notes=""
    
    # Try to fetch release notes from GitHub API
    echo "Fetching release notes for v${VERSION}..." >&2
    local api_url="https://api.github.com/repos/hcavarsan/kftray/releases/tags/v${VERSION}"
    
    if command -v curl >/dev/null 2>&1; then
        local response=$(curl -s "$api_url" 2>/dev/null)
        if [ $? -eq 0 ] && echo "$response" | grep -q '"tag_name"'; then
            # Extract body from JSON (simple extraction, works for basic cases)
            release_notes=$(echo "$response" | grep -o '"body":"[^"]*"' | sed 's/"body":"//' | sed 's/"$//' | sed 's/\\n/\n/g' | sed 's/\\r//g')
        fi
    fi
    
    # Fallback to generic message if no release notes found
    if [ -z "$release_notes" ]; then
        release_notes="Update to version ${VERSION}"
    fi
    
    # Generate RFC 2822 date format for changelog
    local date_str=$(date -R 2>/dev/null || date '+%a, %d %b %Y %H:%M:%S %z')
    
    # Generate changelog entry
    echo "${package_name} (${VERSION}-1) stable; urgency=low"
    echo ""
    
    # Format release notes with proper indentation
    if [ -n "$release_notes" ]; then
        echo "$release_notes" | while IFS= read -r line; do
            if [ -n "$line" ]; then
                echo "  * $line"
            else
                echo "  ."
            fi
        done
    else
        echo "  * Update to version ${VERSION}"
    fi
    
    echo ""
    echo " -- hcavarsan <hcavarsan@yahoo.com.br>  $date_str"
}

# Generate debian source package files and checksums
generate_debian_source() {
    local package_name="$1"
    local temp_dir="checksums-${package_name}"
    
    mkdir -p "$temp_dir"
    cd "$temp_dir"
    
    local base_url="https://github.com/hcavarsan/kftray/releases/download/v${VERSION}"
    local amd64_file=""
    local arm64_file=""
    
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
    
    echo "Creating debian source package for ${package_name}..."
    
    # Create orig.tar.gz with the actual binaries
    mkdir -p "${package_name}-${VERSION}"
    
    # Download binaries and add to source tree
    echo "Downloading binaries..."
    if curl -sL "${base_url}/${amd64_file}" -o "${package_name}-${VERSION}/${amd64_file}"; then
        echo "Downloaded: ${amd64_file}"
    else
        echo "Warning: Could not download ${amd64_file}"
    fi
    
    if curl -sL "${base_url}/${arm64_file}" -o "${package_name}-${VERSION}/${arm64_file}"; then
        echo "Downloaded: ${arm64_file}"
    else
        echo "Warning: Could not download ${arm64_file}"
    fi
    
    # Create orig.tar.gz
    tar -czf "${package_name}_${VERSION}.orig.tar.gz" "${package_name}-${VERSION}/"
    
    # Create debian.tar.xz with proper debian/ subdirectory structure
    mkdir -p debian-temp/debian
    
    # Copy debian files from templates to debian/ subdirectory
    for template in "${SCRIPT_DIR}/${package_name}/templates/debian-"*; do
        [ -f "$template" ] || continue
        filename=$(basename "$template")
        # Remove debian- prefix and copy to debian/ subdirectory
        target_file="${filename#debian-}"
        cp "$template" "debian-temp/debian/$target_file"
    done
    
    # Create debian/source/format file
    mkdir -p debian-temp/debian/source
    echo "3.0 (quilt)" > debian-temp/debian/source/format
    
    # Generate dynamic changelog with release notes
    generate_changelog "$package_name" > debian-temp/debian/changelog
    
    # Process debian control files with version substitution (except changelog)
    for file in debian-temp/debian/*; do
        [ -f "$file" ] || continue
        [ "$(basename "$file")" = "changelog" ] && continue  # Skip changelog, already generated
        sed -i "s/{{VERSION}}/${VERSION}/g" "$file"
    done
    
    # Create the debian.tar.xz from the temp directory
    tar -cJf "${package_name}_${VERSION}-1.debian.tar.xz" -C debian-temp .
    
    # Calculate checksums and sizes
    local orig_file="${package_name}_${VERSION}.orig.tar.gz"
    local debian_file="${package_name}_${VERSION}-1.debian.tar.xz"
    
    # Copy generated files to package directory for upload
    cp "$orig_file" "${SCRIPT_DIR}/package-${package_name}/"
    cp "$debian_file" "${SCRIPT_DIR}/package-${package_name}/"
    
    # Calculate all checksums and sizes for orig.tar.gz
    local md5_orig=$(md5sum "$orig_file" | cut -d' ' -f1)
    local sha1_orig=$(sha1sum "$orig_file" | cut -d' ' -f1)
    local sha256_orig=$(sha256sum "$orig_file" | cut -d' ' -f1)
    local size_orig=$(stat -c%s "$orig_file")
    
    # Calculate all checksums and sizes for debian.tar.xz
    local md5_debian=$(md5sum "$debian_file" | cut -d' ' -f1)
    local sha1_debian=$(sha1sum "$debian_file" | cut -d' ' -f1)
    local sha256_debian=$(sha256sum "$debian_file" | cut -d' ' -f1)
    local size_debian=$(stat -c%s "$debian_file")
    
    echo "Orig file checksums:"
    echo "  MD5: $md5_orig"
    echo "  SHA1: $sha1_orig"
    echo "  SHA256: $sha256_orig"
    echo "  Size: $size_orig"
    
    echo "Debian file checksums:"
    echo "  MD5: $md5_debian"
    echo "  SHA1: $sha1_debian"
    echo "  SHA256: $sha256_debian"
    echo "  Size: $size_debian"
    
    cd ..
    rm -rf "$temp_dir"
    
    # Export all values for template substitution
    export MD5_ORIG="$md5_orig"
    export SHA1_ORIG="$sha1_orig"
    export SHA256_ORIG="$sha256_orig"
    export SIZE_ORIG="$size_orig"
    export MD5_DEBIAN="$md5_debian"
    export SHA1_DEBIAN="$sha1_debian"
    export SHA256_DEBIAN="$sha256_debian"
    export SIZE_DEBIAN="$size_debian"
    
    # Keep original exports for RPM compatibility
    export SHA256_AMD64="$sha256_orig"
    export SHA256_ARM64="$sha256_orig"
}

# Compatibility alias for old function name
generate_checksums() {
    generate_debian_source "$@"
}

generate_repos_xml() {
    while IFS=: read -r name project version repo archs; do
        [[ "$name" =~ ^#.*$ ]] && continue
        [[ -z "$name" ]] && continue
        
        echo "  <repository name=\"$name\">"
        echo "    <path project=\"${project}:${version}\" repository=\"$repo\"/>"
        
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
  <person userid="${OBS_USER}" role="maintainer"/>
$(generate_repos_xml)
</project>
EOF
        retry_command "osc meta prj -F project.xml \"${OBS_PROJECT}\""
        rm project.xml
    fi
}

prepare_package() {
    local package_name="$1"
    echo "Creating package directory: package-${package_name}"
    mkdir -p "package-${package_name}" || {
        echo "Error: Failed to create package directory"
        return 1
    }
    
    # Generate debian source package and checksums
    echo "Generating debian source package for ${package_name}..."
    generate_debian_source "${package_name}" || {
        echo "Error: Failed to generate debian source package"
        return 1
    }
    
    echo "Processing template files from: ${SCRIPT_DIR}/${package_name}/templates/"
    for template in "${SCRIPT_DIR}/${package_name}/templates"/*; do
        [ -f "$template" ] || continue
        
        filename=$(basename "$template")
        echo "Processing template: $filename"
        # Substitute all template placeholders
        sed -e "s/{{VERSION}}/${VERSION}/g" \
            -e "s/{{SHA256_AMD64}}/${SHA256_AMD64}/g" \
            -e "s/{{SHA256_ARM64}}/${SHA256_ARM64}/g" \
            -e "s/{{MD5_ORIG}}/${MD5_ORIG}/g" \
            -e "s/{{SHA1_ORIG}}/${SHA1_ORIG}/g" \
            -e "s/{{SHA256_ORIG}}/${SHA256_ORIG}/g" \
            -e "s/{{SIZE_ORIG}}/${SIZE_ORIG}/g" \
            -e "s/{{MD5_DEBIAN}}/${MD5_DEBIAN}/g" \
            -e "s/{{SHA1_DEBIAN}}/${SHA1_DEBIAN}/g" \
            -e "s/{{SHA256_DEBIAN}}/${SHA256_DEBIAN}/g" \
            -e "s/{{SIZE_DEBIAN}}/${SIZE_DEBIAN}/g" \
            "$template" > "package-${package_name}/$filename" || {
            echo "Error: Failed to process template $filename"
            return 1
        }
    done
    
    [ -f "package-${package_name}/debian-rules" ] && chmod +x "package-${package_name}/debian-rules"
    echo "Successfully prepared package files for ${package_name}"
}

upload_package() {
    local package_name="$1"
    local working_dir="${OBS_PROJECT}/${package_name}"
    local base_dir="$(pwd)"
    
    echo "Processing package: ${package_name}"
    
    # Step 1: Ensure we're in the correct base directory
    cd "$base_dir"
    
    # Step 2: Ensure package exists in OBS (idempotent)
    echo "Ensuring package ${package_name} exists in OBS..."
    if ! osc meta pkg "${OBS_PROJECT}" "${package_name}" &>/dev/null; then
        echo "Package doesn't exist, creating..."
        retry_command "osc meta pkg \"${OBS_PROJECT}\" \"${package_name}\" -F /dev/stdin" << EOF
<package name="${package_name}" project="${OBS_PROJECT}">
  <title>${package_name}</title>
  <description>Kubernetes port-forwarding tool</description>
</package>
EOF
    else
        echo "Package ${package_name} already exists"
    fi
    
    # Step 3: Handle working copy (resilient to existing state)
    if [ -d "$working_dir" ]; then
        if [ -d "$working_dir/.osc" ]; then
            echo "Valid working copy exists, updating from server..."
            cd "$working_dir"
            if retry_command "osc up"; then
                echo "Working copy updated successfully"
                cd "$base_dir"
            else
                echo "Update failed, recreating working copy..."
                cd "$base_dir"
                rm -rf "$working_dir"
            fi
        else
            echo "Invalid working copy directory, removing..."
            rm -rf "$working_dir"
        fi
    fi
    
    # Step 4: Create working copy if it doesn't exist
    if [ ! -d "$working_dir" ]; then
        echo "Creating working copy for ${package_name}..."
        retry_command "osc co \"${OBS_PROJECT}\" \"${package_name}\""
    fi
    
    # Step 5: Verify we have a valid working copy
    if [ ! -d "$working_dir/.osc" ]; then
        echo "Error: Failed to create valid working copy for ${package_name}"
        return 1
    fi
    
    # Step 6: Update files (with change detection)
    cd "$working_dir"
    
    # Verify source files exist
    if [ ! -d "${base_dir}/package-${package_name}" ]; then
        echo "Error: Source directory package-${package_name} not found"
        cd "$base_dir"
        return 1
    fi
    
    # Create backup for change detection
    local temp_backup=$(mktemp -d)
    cp * "$temp_backup/" 2>/dev/null || true
    
    # Copy new files
    echo "Updating package files..."
    cp "${base_dir}/package-${package_name}"/* . 2>/dev/null || {
        echo "Warning: No files found in package-${package_name}/"
        rm -rf "$temp_backup"
        cd "$base_dir"
        return 1
    }
    
    # Handle debian packaging - keep files flat for OBS
    # OBS expects debian files at root level, not in debian/ subdirectory
    
    # Step 7: Detect changes and commit only if needed
    local has_changes=false
    
    # Check for new or modified files
    for file in *; do
        [ -f "$file" ] || continue
        if [ ! -f "$temp_backup/$file" ] || ! cmp -s "$file" "$temp_backup/$file" 2>/dev/null; then
            has_changes=true
            echo "Detected change in: $file"
            break
        fi
    done
    
    # Check for deleted files
    if [ "$has_changes" = false ]; then
        for file in "$temp_backup"/*; do
            [ -f "$file" ] || continue
            local basename_file=$(basename "$file")
            if [ ! -f "$basename_file" ]; then
                has_changes=true
                echo "Detected deletion: $basename_file"
                break
            fi
        done
    fi
    
    # Clean up backup
    rm -rf "$temp_backup"
    
    # Step 8: Commit changes if detected
    if [ "$has_changes" = true ]; then
        echo "Changes detected for ${package_name}, committing..."
        
        # Add only new files to avoid interactive prompts on existing files
        for file in *; do
            [ -f "$file" ] || continue
            if ! osc status "$file" &>/dev/null; then
                echo "Adding new file: $file"
                osc add "$file" 2>/dev/null || true
            fi
        done
        
        # Specifically handle debian files that might be untracked
        for debian_file in debian-*; do
            if [ -f "$debian_file" ]; then
                local status_output=$(osc status "$debian_file" 2>/dev/null || echo "?")
                if [[ "$status_output" == *"?"* ]] || ! osc status "$debian_file" &>/dev/null; then
                    echo "Adding untracked debian file: $debian_file"
                    osc add "$debian_file" 2>/dev/null || true
                fi
            fi
        done
        
        # Specifically handle source tarball files
        for source_file in *.orig.tar.gz *.debian.tar.xz; do
            if [ -f "$source_file" ]; then
                local status_output=$(osc status "$source_file" 2>/dev/null || echo "?")
                if [[ "$status_output" == *"?"* ]] || ! osc status "$source_file" &>/dev/null; then
                    echo "Adding source file: $source_file"
                    osc add "$source_file" 2>/dev/null || true
                fi
            fi
        done
        
        # Check OBS status before committing
        echo "Current OBS status:"
        osc status || true
        
        # Commit with proper error handling
        if retry_command "osc commit -m \"Update to version ${VERSION}\""; then
            echo "Successfully committed ${package_name}"
            
            # Show build status (non-blocking)
            echo "Build status for ${package_name}:"
            retry_command "osc results" || echo "Could not retrieve build status (this is normal for new packages)"
        else
            echo "Failed to commit ${package_name}"
            cd "$base_dir"
            return 1
        fi
    else
        echo "No changes detected for ${package_name}, skipping commit"
    fi
    
    # Step 9: Return to base directory
    cd "$base_dir"
    echo "Finished processing ${package_name}"
    return 0
}

# Cleanup function for graceful exit
cleanup() {
    local exit_code=$?
    if [ $exit_code -ne 0 ]; then
        echo "Script failed with exit code $exit_code, cleaning up..."
    fi
    
    # Clean up any temporary directories
    find . -maxdepth 1 -name "package-*" -type d -exec rm -rf {} + 2>/dev/null || true
    find . -maxdepth 1 -name "checksums-*" -type d -exec rm -rf {} + 2>/dev/null || true
    
    # Remove any stale project.xml files
    [ -f project.xml ] && rm -f project.xml
    
    if [ $exit_code -ne 0 ]; then
        echo "Cleanup completed. Check logs above for errors."
        echo "To retry: export VERSION=${VERSION:-} OBS_USER=${OBS_USER:-} OBS_PASSWORD=*** && $0 $*"
    fi
    
    exit $exit_code
}

# Set up signal handlers for cleanup
trap cleanup EXIT INT TERM

main() {
    echo "=== OBS Package Publisher ==="
    echo "Version: ${VERSION}"
    echo "Project: ${OBS_PROJECT}"
    echo "User: ${OBS_USER}"
    echo "Timestamp: $(date)"
    echo "================================"
    
    # Validate version format before proceeding
    validate_version "${VERSION}"
    
    # Create project (idempotent)
    echo "Step 1: Creating/verifying OBS project..."
    create_project
    echo "✓ Project ready"
    
    # Determine which packages to process
    local packages_to_process
    if [ $# -gt 0 ]; then
        # Process only specified package(s)
        packages_to_process=("$@")
        echo "Step 2: Processing specific package(s): ${packages_to_process[*]}"
    else
        # Process all packages
        packages_to_process=("${PACKAGES[@]}")
        echo "Step 2: Processing all packages: ${packages_to_process[*]}"
    fi
    
    # Track success/failure
    local failed_packages=()
    local successful_packages=()
    
    # Process each package
    for package in "${packages_to_process[@]}"; do
        echo ""
        echo "=== Processing package: ${package} ==="
        
        # Prepare package files
        echo "Step 2a: Preparing package files for ${package}..."
        if prepare_package "${package}"; then
            echo "✓ Package files prepared for ${package}"
        else
            echo "✗ Failed to prepare package files for ${package}"
            failed_packages+=("${package}")
            continue
        fi
        
        # Upload package
        echo "Step 2b: Uploading package ${package} to OBS..."
        if upload_package "${package}"; then
            echo "✓ Successfully processed ${package}"
            successful_packages+=("${package}")
            echo "Package URL: https://build.opensuse.org/package/show/${OBS_PROJECT}/${package}"
        else
            echo "✗ Failed to upload package ${package}"
            failed_packages+=("${package}")
        fi
    done
    
    # Final summary
    echo ""
    echo "=== SUMMARY ==="
    if [ ${#successful_packages[@]} -gt 0 ]; then
        echo "✓ Successfully processed packages: ${successful_packages[*]}"
    fi
    
    if [ ${#failed_packages[@]} -gt 0 ]; then
        echo "✗ Failed packages: ${failed_packages[*]}"
        echo "Project URL: https://build.opensuse.org/project/show/${OBS_PROJECT}"
        return 1
    else
        echo "✓ All packages processed successfully!"
        echo "Project URL: https://build.opensuse.org/project/show/${OBS_PROJECT}"
        echo "Repository URL: https://download.opensuse.org/repositories/${OBS_PROJECT//:\/}//"
        return 0
    fi
}

main "$@"
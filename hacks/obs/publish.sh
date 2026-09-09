#!/usr/bin/env bash
set -euo pipefail
umask 022

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
    echo "Usage: $0 [--dry-run] [package_name...]"
    echo ""
    echo "Environment variables required:"
    echo "  VERSION      - Version to publish (e.g., 1.2.3)"
    echo "  OBS_USER     - OpenSUSE Build Service username"
    echo "  OBS_PASSWORD - OpenSUSE Build Service password"
    echo ""
    echo "Optional:"
    echo "  OBS_PROJECT         - Project name (default: home:\${OBS_USER}:kftray)"
    echo "  OBS_RESULTS_TIMEOUT - Wait budget per package for OBS builds (default: 30m)"
    echo "  GITHUB_TOKEN        - Authenticates GitHub API calls (avoids anonymous rate limits)"
    echo "  --dry-run           - Validate and prepare packages without contacting OBS"
    echo ""
    echo "Examples:"
    echo "  export VERSION=1.2.3 OBS_USER=myuser OBS_PASSWORD=mypass"
    echo "  $0                    # Publish all packages"
    echo "  $0 kftui              # Publish only kftui"
    echo "  $0 kftui kftray       # Publish both packages"
    echo ""
    echo "Available packages: kftui, kftray"
}

ORIGINAL_ARGS="$*"
DRY_RUN=false
if [ "${1:-}" = "--dry-run" ]; then
    DRY_RUN=true
    shift
fi

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

if [ "$DRY_RUN" = false ] && [ -z "${OBS_USER:-}" ]; then
    echo "Error: OBS_USER environment variable is required"
    echo "Example: export OBS_USER=myusername"
    show_usage
    exit 1
fi

if [ "$DRY_RUN" = false ] && [ -z "${OBS_PASSWORD:-}" ]; then
    echo "Error: OBS_PASSWORD environment variable is required"
    echo "Example: export OBS_PASSWORD=mypassword"
    show_usage
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VERSION="${VERSION#v}"
OBS_PROJECT="${OBS_PROJECT:-home:${OBS_USER:-dryrun}:kftray}"
OBS_RESULTS_TIMEOUT="${OBS_RESULTS_TIMEOUT:-30m}"
if [[ "$OBS_RESULTS_TIMEOUT" =~ ^([0-9]+)([smhd]?)$ ]]; then
    case "${BASH_REMATCH[2]}" in
        m) OBS_RESULTS_TIMEOUT_SECONDS=$((BASH_REMATCH[1] * 60)) ;;
        h) OBS_RESULTS_TIMEOUT_SECONDS=$((BASH_REMATCH[1] * 3600)) ;;
        d) OBS_RESULTS_TIMEOUT_SECONDS=$((BASH_REMATCH[1] * 86400)) ;;
        *) OBS_RESULTS_TIMEOUT_SECONDS=$((BASH_REMATCH[1])) ;;
    esac
else
    echo "Error: OBS_RESULTS_TIMEOUT must be a number with an optional s, m, h or d suffix, got '${OBS_RESULTS_TIMEOUT}'" >&2
    exit 1
fi
PACKAGES=("kftui" "kftray")
WORK_DIR=""
RELEASE_METADATA=""
SOURCE_DATE_EPOCH=""

# Version validation function
validate_version() {
    local version="$1"
    
    if [[ ! "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$ ]]; then
        echo "Error: Invalid version format: '$version'"
        echo "Expected a stable version: X.Y.Z (e.g., 1.2.3)"
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
    
    while [ $attempt -le $max_attempts ]; do
        printf 'Attempt %s/%s:' "$attempt" "$max_attempts"
        printf ' %q' "$@"
        printf '\n'
        if "$@"; then
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
    
    echo "Command failed after $max_attempts attempts: $*"
    return 1
}

load_release() {
    local -a auth=()
    RELEASE_METADATA="${WORK_DIR}/release.json"
    if [ -n "${GITHUB_TOKEN:-}" ]; then
        auth=(-H "Authorization: Bearer ${GITHUB_TOKEN}")
    fi
    curl -fsSL --retry 3 --connect-timeout 15 --max-time 60 \
        -H "Accept: application/vnd.github+json" "${auth[@]}" \
        "https://api.github.com/repos/hcavarsan/kftray/releases/tags/v${VERSION}" \
        -o "$RELEASE_METADATA" || return 1
    if ! jq -e --arg tag "v${VERSION}" \
        '.tag_name == $tag and .draft == false and .prerelease == false and (.assets | type == "array")' \
        "$RELEASE_METADATA" >/dev/null; then
        echo "Error: Expected a published stable release v${VERSION}" >&2
        return 1
    fi
    SOURCE_DATE_EPOCH=$(jq -er '.published_at | fromdateiso8601' "$RELEASE_METADATA") || return 1
}

download_asset() {
    local filename="$1" destination="$2" digest
    digest=$(jq -er --arg name "$filename" \
        '[.assets[] | select(.name == $name)] | select(length == 1) | .[0].digest | select(type == "string") | select(test("^sha256:[0-9a-f]{64}$"))' \
        "$RELEASE_METADATA") || {
        echo "Error: Release asset $filename is missing a SHA256 digest" >&2
        return 1
    }
    curl -fsSL --retry 3 --connect-timeout 15 --max-time 600 \
        "https://github.com/hcavarsan/kftray/releases/download/v${VERSION}/${filename}" \
        -o "$destination" || return 1
    printf '%s  %s\n' "${digest#sha256:}" "$destination" | sha256sum --check --status || {
        echo "Error: SHA256 mismatch for $filename" >&2
        return 1
    }
}

generate_changelog() {
    local package_name="$1"
    local release_notes date_str
    release_notes=$(jq -r '.body // "" | gsub("\r"; "")' "$RELEASE_METADATA") || return 1
    
    # Fallback to generic message if no release notes found
    if [ -z "$release_notes" ]; then
        release_notes="Update to version ${VERSION}"
    fi
    
    date_str=$(jq -r '.published_at | fromdateiso8601 | strftime("%a, %d %b %Y %H:%M:%S +0000")' "$RELEASE_METADATA") || return 1
    
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
    
    mkdir -p "$temp_dir" || return 1
    cd "$temp_dir" || return 1
    
    local amd64_file=""
    local arm64_file=""
    
    case "$package_name" in
        "kftui")
            amd64_file="kftui_linux_amd64"
            arm64_file="kftui_linux_arm64"
            ;;
        "kftray")
            amd64_file="kftray_${VERSION}_amd64.AppImage"
            arm64_file="kftray_${VERSION}_aarch64.AppImage"
            ;;
    esac
    
    echo "Creating debian source package for ${package_name}..."
    
    # Create orig.tar.gz with the actual binaries
    mkdir -p "${package_name}-${VERSION}" || return 1
    
    # Download binaries and add to source tree
    echo "Downloading binaries..."
    if download_asset "$amd64_file" "${package_name}-${VERSION}/${amd64_file}"; then
        echo "Downloaded: ${amd64_file}"
    else
        echo "Error: Could not download ${amd64_file}" >&2
        return 1
    fi
    
    if download_asset "$arm64_file" "${package_name}-${VERSION}/${arm64_file}"; then
        echo "Downloaded: ${arm64_file}"
    else
        echo "Error: Could not download ${arm64_file}" >&2
        return 1
    fi

    if [ "$package_name" = kftray ]; then
        local architecture newer_file
        for architecture in amd64 aarch64; do
            newer_file="kftray_${VERSION}_newer-glibc_${architecture}.AppImage"
            download_asset "$newer_file" "${package_name}-${VERSION}/${newer_file}" || return 1
        done
    fi
    
    # Create orig.tar.gz
    tar --sort=name --mtime="@${SOURCE_DATE_EPOCH}" --owner=0 --group=0 --numeric-owner \
        -czf "${package_name}_${VERSION}.orig.tar.gz" "${package_name}-${VERSION}/" || return 1
    
    # Create debian.tar.xz with proper debian/ subdirectory structure
    mkdir -p debian-temp/debian || return 1
    
    # Copy debian files from templates to debian/ subdirectory
    for template in "${SCRIPT_DIR}/${package_name}/templates/debian-"*; do
        [ -f "$template" ] || continue
        local filename target_file
        filename=$(basename "$template") || return 1
        # Remove debian- prefix and copy to debian/ subdirectory
        target_file="${filename#debian-}"
        cp "$template" "debian-temp/debian/$target_file" || return 1
    done
    
    # Create debian/source/format file
    mkdir -p debian-temp/debian/source || return 1
    echo "3.0 (quilt)" > debian-temp/debian/source/format || return 1
    
    # Generate dynamic changelog with release notes
    generate_changelog "$package_name" > debian-temp/debian/changelog || return 1
    
    # Process debian control files with version substitution (except changelog)
    for file in debian-temp/debian/*; do
        [ -f "$file" ] || continue
        [ "$(basename "$file")" = "changelog" ] && continue  # Skip changelog, already generated
        sed -i "s/{{VERSION}}/${VERSION}/g" "$file" || return 1
    done
    chmod +x debian-temp/debian/rules || return 1
    
    # Create the debian.tar.xz from the temp directory
    tar --sort=name --mtime="@${SOURCE_DATE_EPOCH}" --owner=0 --group=0 --numeric-owner \
        -cJf "${package_name}_${VERSION}-1.debian.tar.xz" -C debian-temp . || return 1
    
    # Calculate checksums and sizes
    local orig_file="${package_name}_${VERSION}.orig.tar.gz"
    local debian_file="${package_name}_${VERSION}-1.debian.tar.xz"
    
    # Copy generated files to package directory for upload
    cp "$orig_file" "$debian_file" "${WORK_DIR}/package-${package_name}/" || return 1
    
    # Calculate all checksums and sizes for orig.tar.gz
    local md5_orig sha1_orig sha256_orig size_orig
    md5_orig=$(md5sum "$orig_file" | cut -d' ' -f1) || return 1
    sha1_orig=$(sha1sum "$orig_file" | cut -d' ' -f1) || return 1
    sha256_orig=$(sha256sum "$orig_file" | cut -d' ' -f1) || return 1
    size_orig=$(stat -c%s "$orig_file") || return 1
    
    # Calculate all checksums and sizes for debian.tar.xz
    local md5_debian sha1_debian sha256_debian size_debian
    md5_debian=$(md5sum "$debian_file" | cut -d' ' -f1) || return 1
    sha1_debian=$(sha1sum "$debian_file" | cut -d' ' -f1) || return 1
    sha256_debian=$(sha256sum "$debian_file" | cut -d' ' -f1) || return 1
    size_debian=$(stat -c%s "$debian_file") || return 1
    
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
    
    cd "$WORK_DIR" || return 1
    
    # Export all values for template substitution
    export MD5_ORIG="$md5_orig"
    export SHA1_ORIG="$sha1_orig"
    export SHA256_ORIG="$sha256_orig"
    export SIZE_ORIG="$size_orig"
    export MD5_DEBIAN="$md5_debian"
    export SHA1_DEBIAN="$sha1_debian"
    export SHA256_DEBIAN="$sha256_debian"
    export SIZE_DEBIAN="$size_debian"
    
}

generate_repos_xml() {
    local name project repo archs arch
    while read -r name project repo archs; do
        [[ "$name" =~ ^#.*$ ]] && continue
        [[ -z "$name" ]] && continue
        if [ -z "$project" ] || [ -z "$repo" ] || [ -z "$archs" ]; then
            echo "Error: Invalid distros.conf entry: $name" >&2
            return 1
        fi
        
        echo "  <repository name=\"$name\">"
        echo "    <path project=\"$project\" repository=\"$repo\"/>"
        
        IFS=',' read -ra ARCH_ARRAY <<< "$archs"
        for arch in "${ARCH_ARRAY[@]}"; do
            echo "    <arch>$arch</arch>"
        done
        
        echo "  </repository>"
    done < "${SCRIPT_DIR}/distros.conf"
}

render_project_meta() {
    local current="$1" output="$2" template="${WORK_DIR}/project-template.xml" repositories
    repositories=$(generate_repos_xml) || return 1
    {
        echo "<project name=\"${OBS_PROJECT}\">"
        echo "  <title>KFtray</title>"
        echo "  <description>Kubernetes port-forwarding manager</description>"
        echo "  <person userid=\"${OBS_USER:-dryrun}\" role=\"maintainer\"/>"
        printf '%s\n' "$repositories"
        echo "</project>"
    } > "$template" || return 1
    python3 - "$current" "$template" "$output" <<'PY'
import copy
import sys
import xml.etree.ElementTree as ET

current_path, template_path, output_path = sys.argv[1:4]


def canonical(element):
    clone = copy.deepcopy(element)
    ET.indent(clone)
    return ET.tostring(clone)


template = ET.parse(template_path).getroot()
if current_path:
    project = ET.parse(current_path).getroot()
    before = canonical(project)
    for repository in project.findall("repository"):
        project.remove(repository)
    project.extend(template.findall("repository"))
    changed = canonical(project) != before
else:
    project = template
    changed = True
ET.indent(project)
ET.ElementTree(project).write(output_path, encoding="unicode")
print("changed" if changed else "unchanged")
PY
}

ensure_project() {
    local current="${WORK_DIR}/project-current.xml" rendered="${WORK_DIR}/project.xml"
    local errors="${WORK_DIR}/project-errors.txt" attempt state
    for attempt in 1 2 3; do
        if osc meta prj "${OBS_PROJECT}" > "$current" 2> "$errors"; then
            break
        fi
        if grep -q 'HTTP Error 404' "$errors"; then
            current=""
            break
        fi
        cat "$errors" >&2
        [ "$attempt" -lt 3 ] || return 1
        sleep 5
    done
    state=$(render_project_meta "$current" "$rendered") || return 1
    if [ -z "$current" ]; then
        echo "Creating OBS project ${OBS_PROJECT} from distros.conf"
    elif [ "$state" = unchanged ]; then
        echo "OBS project repositories already match distros.conf"
        return 0
    else
        echo "Updating OBS project repositories from distros.conf"
    fi
    retry_command osc meta prj -F "$rendered" "${OBS_PROJECT}" || return 1
}

prepare_package() {
    local package_name="$1"
    local filename
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
        
        filename=$(basename "$template") || return 1
        echo "Processing template: $filename"
        # Substitute all template placeholders
        sed -e "s/{{VERSION}}/${VERSION}/g" \
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
    
    chmod +x "package-${package_name}/debian-rules" || return 1
    echo "Successfully prepared package files for ${package_name}"
}

fetch_results() {
    local results_file="$1" attempt
    for attempt in 1 2 3; do
        [ "$attempt" -eq 1 ] || sleep 5
        osc results --xml > "$results_file" && return 0
        echo "Warning: could not fetch OBS build results (attempt ${attempt}/3)" >&2
    done
    return 1
}

wait_for_scheduler() {
    local package_name="$1" results_file="$2" previous_state="$3" attempt state
    for attempt in $(seq 1 60); do
        fetch_results "$results_file" || return 1
        state=$(xmllint --xpath 'string(/resultlist/@state)' "$results_file") || state=""
        if [ "$state" != "$previous_state" ] || [ "$(xmllint --xpath 'boolean(/resultlist/result[@dirty="true"] | /resultlist/result/status[@code="scheduled" or @code="blocked" or @code="dispatching" or @code="building" or @code="signing" or @code="finished"])' "$results_file")" = true ]; then
            return 0
        fi
        sleep 5
    done
    echo "Error: OBS scheduler did not pick up the new revision of ${package_name} within 5 minutes" >&2
    return 1
}

wait_for_results() {
    local package_name="$1" deadline remaining status
    deadline=$(( $(date +%s) + OBS_RESULTS_TIMEOUT_SECONDS ))
    while :; do
        remaining=$(( deadline - $(date +%s) ))
        if [ "$remaining" -le 0 ]; then
            echo "Error: OBS builds of ${package_name} did not finish within ${OBS_RESULTS_TIMEOUT}" >&2
            return 1
        fi
        status=0
        timeout "$remaining" osc results --watch || status=$?
        [ "$status" -ne 0 ] || return 0
        [ "$status" -ne 124 ] || continue
        echo "Warning: lost the OBS connection while watching ${package_name} builds (osc exited ${status}), retrying in 10s..." >&2
        sleep 10
    done
}

upload_package() {
    local package_name="$1"
    local working_dir="${OBS_PROJECT}/${package_name}"
    local base_dir="$WORK_DIR"
    local package_meta="${WORK_DIR}/${package_name}.xml"
    local file status_output
    
    echo "Processing package: ${package_name}"
    
    cd "$base_dir" || return 1
    
    echo "Ensuring package ${package_name} exists in OBS..."
    if ! osc meta pkg "${OBS_PROJECT}" "${package_name}" &>/dev/null; then
        echo "Package doesn't exist, creating..."
        cat > "$package_meta" << EOF || return 1
<package name="${package_name}" project="${OBS_PROJECT}">
  <title>${package_name}</title>
  <description>Kubernetes port-forwarding tool</description>
</package>
EOF
        retry_command osc meta pkg "${OBS_PROJECT}" "${package_name}" -F "$package_meta" || return 1
    else
        echo "Package ${package_name} already exists"
    fi
    
    echo "Creating working copy for ${package_name}..."
    retry_command osc co "${OBS_PROJECT}" "${package_name}" || return 1
    
    if [ ! -d "$working_dir/.osc" ]; then
        echo "Error: Failed to create valid working copy for ${package_name}"
        return 1
    fi
    
    cd "$working_dir" || return 1
    
    if [ ! -d "${base_dir}/package-${package_name}" ]; then
        echo "Error: Source directory package-${package_name} not found"
        return 1
    fi
    
    echo "Updating package files..."
    cp "${base_dir}/package-${package_name}"/* . || return 1
    for file in *; do
        if [ -f "$file" ] && [ ! -f "${base_dir}/package-${package_name}/$file" ]; then
            rm -- "$file" || return 1
        fi
    done
    osc addremove || return 1
    status_output=$(osc status) || return 1

    local results_file="${WORK_DIR}/results-${package_name}.xml" previous_state=""
    if [ -n "$status_output" ]; then
        echo "Changes detected for ${package_name}, committing..."
        printf '%s\n' "$status_output"
        fetch_results "$results_file" || return 1
        previous_state=$(xmllint --xpath 'string(/resultlist/@state)' "$results_file") || previous_state=""
        if retry_command osc commit -m "Update to version ${VERSION}"; then
            echo "Successfully committed ${package_name}"
        else
            echo "Failed to commit ${package_name}"
            return 1
        fi
        wait_for_scheduler "$package_name" "$results_file" "$previous_state" || return 1
    else
        echo "No changes detected for ${package_name}, skipping commit"
    fi
    
    echo "Waiting for OBS builds of ${package_name} (up to ${OBS_RESULTS_TIMEOUT})..."
    wait_for_results "$package_name" || return 1
    fetch_results "$results_file" || return 1
    if [ "$(xmllint --xpath 'boolean(/resultlist/result/status[@code="succeeded"]) and not(/resultlist/result[@dirty="true" or not(status)]) and not(/resultlist/result/status[not(@code) or (@code!="succeeded" and @code!="excluded" and @code!="disabled")])' "$results_file")" != true ]; then
        cat "$results_file" >&2
        echo "Error: OBS builds did not all succeed for ${package_name}" >&2
        return 1
    fi
    cd "$base_dir" || return 1
    echo "Finished processing ${package_name}"
    return 0
}

# Cleanup function for graceful exit
cleanup() {
    local exit_code=$?
    if [ $exit_code -ne 0 ]; then
        echo "Script failed with exit code $exit_code, cleaning up..."
    fi
    
    if [ -n "$WORK_DIR" ]; then
        rm -rf -- "$WORK_DIR"
    fi
    
    if [ $exit_code -ne 0 ]; then
        echo "Cleanup completed. Check logs above for errors."
        echo "To retry: export VERSION=${VERSION:-} OBS_USER=${OBS_USER:-} OBS_PASSWORD=*** && $0 ${ORIGINAL_ARGS}"
    fi
    
    exit $exit_code
}

# Set up signal handlers for cleanup
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

main() {
    echo "=== OBS Package Publisher ==="
    echo "Version: ${VERSION}"
    echo "Project: ${OBS_PROJECT}"
    echo "User: ${OBS_USER:-dryrun}"
    echo "Timestamp: $(date)"
    echo "================================"
    
    # Validate version format before proceeding
    validate_version "${VERSION}"
    
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

    for package in "${packages_to_process[@]}"; do
        case "$package" in
            kftui|kftray) ;;
            *) echo "Error: Unknown package: $package" >&2; return 1 ;;
        esac
    done

    WORK_DIR=$(mktemp -d)
    cd "$WORK_DIR"
    load_release
    if [ "$DRY_RUN" = true ]; then
        echo "OBS project metadata rendered from distros.conf:"
        render_project_meta "" "${WORK_DIR}/project.xml" >/dev/null || return 1
        cat "${WORK_DIR}/project.xml"
    fi
    
    # Track success/failure
    local failed_packages=()
    local successful_packages=()
    
    # Process each package
    for package in "${packages_to_process[@]}"; do
        cd "$WORK_DIR"
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

    done

    if [ ${#failed_packages[@]} -gt 0 ]; then
        echo "Preparation failed for: ${failed_packages[*]}. No packages were uploaded." >&2
        return 1
    fi
    if [ "$DRY_RUN" = true ]; then
        echo "Dry run: validated sources and packaging metadata for ${packages_to_process[*]}"
        return 0
    fi

    cd "$WORK_DIR"
    echo "Reconciling OBS project ${OBS_PROJECT} with distros.conf..."
    ensure_project || return 1
    for package in "${packages_to_process[@]}"; do
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
        echo "Repository URL: https://download.opensuse.org/repositories/${OBS_PROJECT//:/:\/}/"
        return 0
    fi
}

main "$@"

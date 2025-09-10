#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VERSION="${VERSION:-1.0.0-test}"
OBS_PROJECT="home:${OBS_USER:-testuser}:kftray"
PACKAGES=("kftui" "kftray")

echo "=== OBS Dry Run Test ==="
echo "Version: $VERSION"
echo "Project: $OBS_PROJECT"
echo

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

test_project_xml() {
    echo "=== Testing project.xml generation ==="
    cat > project.xml << EOF
<project name="${OBS_PROJECT}">
  <title>KFtray</title>
  <description>Kubernetes port-forwarding manager</description>
$(generate_repos_xml)
</project>
EOF
    echo "Generated project.xml:"
    cat project.xml
    rm project.xml
    echo
}

test_package_preparation() {
    local package_name="$1"
    echo "=== Testing package: ${package_name} ==="
    
    mkdir -p "test-package-${package_name}"
    
    for template in "${SCRIPT_DIR}/${package_name}/templates"/*; do
        [ -f "$template" ] || continue
        
        filename=$(basename "$template")
        echo "Processing template: $filename"
        sed "s/{{VERSION}}/${VERSION}/g" "$template" > "test-package-${package_name}/$filename"
    done
    
    echo "Generated files for ${package_name}:"
    ls -la "test-package-${package_name}/"
    
    echo "Sample _service content:"
    [ -f "test-package-${package_name}/_service" ] && cat "test-package-${package_name}/_service"
    echo
    
    rm -rf "test-package-${package_name}"
}

main() {
    test_project_xml
    
    for package in "${PACKAGES[@]}"; do
        test_package_preparation "$package"
    done
    
    echo "=== Dry run completed successfully! ==="
    echo "Next steps:"
    echo "1. Set real OBS credentials: export OBS_USER=xxx OBS_PASSWORD=xxx"
    echo "2. Run: ./publish.sh"
}

main
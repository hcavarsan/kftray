#!/usr/bin/env bash
set -euo pipefail

# Run the real publisher with local stand-ins for GitHub downloads and OBS writes.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OBS_TEST_ROOT=$(mktemp -d)
trap 'rm -rf -- "$OBS_TEST_ROOT"' EXIT

curl() {
    local url="" output="" fail_http=false argument
    while [ "$#" -gt 0 ]; do
        argument="$1"
        shift
        case "$argument" in
            -o|--output) output="$1"; shift ;;
            -H|--header)
                [[ "$1" != Authorization:* ]] || printf '%s\n' "$1" >> "$OBS_TEST_CASE/auth"
                shift
                ;;
            --fail|--fail-with-body) fail_http=true ;;
            --*) ;;
            -*) [[ "$argument" != *f* ]] || fail_http=true ;;
            https://*) url="$argument" ;;
        esac
    done
    if [[ "$url" == *api.github.com* ]]; then
        if [ -n "$output" ]; then
            printf '%s\n' "$OBS_TEST_METADATA" > "$output"
        else
            printf '%s\n' "$OBS_TEST_METADATA"
        fi
        return 0
    fi
    printf '%s\n' "$url" >> "$OBS_TEST_CASE/downloads"
    if [ "${OBS_TEST_HTTP_FAILURE:-false}" = true ] || [[ "$url" == *kftray_*_arm64.AppImage ]]; then
        if [ "$fail_http" = true ]; then
            return 22
        fi
        printf 'Not Found' > "$output"
    else
        if [ "${OBS_TEST_CORRUPT_DOWNLOAD:-false}" = true ]; then
            printf 'truncated binary\n' > "$output"
        else
            printf 'binary from %s\n' "$url" > "$output"
        fi
    fi
    touch -t "${OBS_TEST_SOURCE_TIME:-202001010000}" "$output"
}

osc() {
    local operation="$1" file package polls
    shift
    printf '%s\n' "$operation $*" >> "$OBS_TEST_CASE/operations"
    case "$operation" in
        up) return 0 ;;
        meta)
            case "$1" in
                pkg) return 0 ;;
                prj)
                    if [ "$2" = -F ]; then
                        cp "$3" "$OBS_TEST_CASE/project-meta.xml"
                        return 0
                    fi
                    if [ "${OBS_TEST_PROJECT_MISSING:-false}" = true ]; then
                        printf 'Server returned an error: HTTP Error 404: Not Found\nProject not found: %s\n' "$2" >&2
                        return 1
                    fi
                    printf '%s\n' "$OBS_TEST_PROJECT_META"
                    ;;
                *) printf 'Unexpected osc meta target: %s\n' "$1" >&2; return 1 ;;
            esac
            ;;
        results)
            package=$(basename "$PWD")
            case " $* " in
                *' --watch '*)
                    if [[ " $* " == *' --xml '* ]]; then
                        printf '<resultlist state="after"><result><status code="building"/></result></resultlist>\n'
                    else
                        printf 'fixture: succeeded\n'
                        return 0
                    fi
                    ;;
            esac
            # Results only change after the scheduler has processed the commit.
            if [ ! -f "$OBS_TEST_CASE/committed-$package" ]; then
                printf '<resultlist state="before"><result><status package="%s" code="failed"/></result></resultlist>\n' "$package"
                return 0
            fi
            polls=$(( $(cat "$OBS_TEST_CASE/polls-$package" 2>/dev/null || echo 0) + 1 ))
            printf '%s' "$polls" > "$OBS_TEST_CASE/polls-$package"
            if [ "$polls" -le "${OBS_TEST_STALE_POLLS:-0}" ]; then
                printf '<resultlist state="before"><result><status package="%s" code="failed"/></result></resultlist>\n' "$package"
                return 0
            fi
            case "${OBS_TEST_BUILD_STATE:-succeeded}" in
                empty) printf '<resultlist state="after"/>\n' ;;
                partial-empty) printf '<resultlist state="after"><result><status code="succeeded"/></result><result/></resultlist>\n' ;;
                missing-code) printf '<resultlist state="after"><result><status code="succeeded"/></result><result><status/></result></resultlist>\n' ;;
                dirty) printf '<resultlist state="after"><result dirty="true"><status code="succeeded"/></result></resultlist>\n' ;;
                *) printf '<resultlist state="after"><result><status package="fixture" code="%s"/></result></resultlist>\n' "${OBS_TEST_BUILD_STATE:-succeeded}" ;;
            esac
            ;;
        co)
            mkdir -p "$1/$2/.osc"
            : > "$1/$2/.osc/tracked"
            if [ "${OBS_TEST_OLD_SOURCES:-false}" = true ]; then
                printf 'old archive\n' > "$1/$2/${2}_0.9.0.orig.tar.gz"
                printf 'stale arch recipe\n' > "$1/$2/PKGBUILD"
                printf 'stale flat changelog\n' > "$1/$2/debian-changelog"
                printf '%s\n' "${2}_0.9.0.orig.tar.gz" PKGBUILD debian-changelog >> "$1/$2/.osc/tracked"
            fi
            ;;
        status)
            if [ "$#" -gt 0 ]; then
                if ! grep -Fxq "${1#./}" .osc/tracked; then
                    printf '?    %s\n' "$1"
                fi
            else
                for file in ./*; do
                    [ -f "$file" ] || continue
                    printf '?    %s\n' "${file#./}"
                done
            fi
            ;;
        add)
            [ "${OBS_TEST_ADD_FAILURE:-false}" != true ] || return 1
            for file in "$@"; do
                printf '%s\n' "${file#./}" >> .osc/tracked
            done
            ;;
        addremove)
            [ "${OBS_TEST_ADD_FAILURE:-false}" != true ] || return 1
            : > .osc/tracked
            for file in ./*; do
                [ -f "$file" ] || continue
                printf '%s\n' "${file#./}" >> .osc/tracked
            done
            ;;
        rm|remove|delete)
            for file in "$@"; do
                rm -- "$file"
            done
            ;;
        commit)
            package=$(basename "$PWD")
            mkdir -p "$OBS_TEST_CASE/upload/$package"
            while IFS= read -r file; do
                [ -f "$file" ] || continue
                cp "$file" "$OBS_TEST_CASE/upload/$package/"
            done < .osc/tracked
            : > "$OBS_TEST_CASE/committed-$package"
            ;;
        *) printf 'Unexpected osc command: %s\n' "$operation" >&2; return 1 ;;
    esac
}

sleep() { :; }
timeout() { shift; "$@"; }

tar() {
    [ "${OBS_TEST_TAR_FAILURE:-false}" != true ] || return 2
    command tar "$@"
}

export -f curl osc sleep tar timeout

fail() { printf 'FAIL: %s\n' "$*" >&2; return 1; }

# Project metadata as OBS would return it today: an obsolete target, custom
# title, extra maintainer and build flags that must survive reconciliation.
drifted_project_meta() {
    cat << 'EOF'
<project name="home:fixture:kftray">
  <title>Custom title</title>
  <description>Custom description</description>
  <person userid="fixture" role="maintainer"/>
  <person userid="other" role="bugowner"/>
  <build>
    <disable arch="i586"/>
  </build>
  <repository name="Ubuntu_20.04">
    <path project="Ubuntu:20.04" repository="standard"/>
    <arch>x86_64</arch>
    <arch>aarch64</arch>
  </repository>
  <repository name="openSUSE_Tumbleweed">
    <path project="openSUSE:Tumbleweed" repository="standard"/>
    <arch>x86_64</arch>
    <arch>aarch64</arch>
  </repository>
</project>
EOF
}

run_publisher() {
    local scenario="$1" log="$2" result=0
    if [ "$scenario" = unknown-package ]; then
        bash hacks/obs/publish.sh unknown > "$log" 2>&1 || result=$?
    elif [ "$scenario" = dry-run ]; then
        unset OBS_USER OBS_PASSWORD OBS_PROJECT
        bash hacks/obs/dry-run.sh > "$log" 2>&1 || result=$?
    elif [ "$scenario" = dry-run-help ]; then
        unset VERSION OBS_USER OBS_PASSWORD OBS_PROJECT
        bash hacks/obs/dry-run.sh --help > "$log" 2>&1 || result=$?
    else
        bash hacks/obs/publish.sh > "$log" 2>&1 || result=$?
    fi
    return "$result"
}

check_uploaded_package() {
    local scenario="$1" package="$2" uploaded="$OBS_TEST_CASE/upload/$2" file glibc architecture expected_file
    if [ "$scenario" = metadata ]; then
        for file in _service "$package.spec" "$package.dsc"; do
            [ -f "$uploaded/$file" ] || fail "missing uploaded $package/$file" || return 1
        done
        return 0
    fi
    [ -f "$uploaded/${package}_1.2.3.orig.tar.gz" ] || fail "missing uploaded $package source archive" || return 1
    [ -f "$uploaded/${package}_1.2.3-1.debian.tar.xz" ] || fail "missing uploaded $package Debian archive" || return 1
    mkdir -p "$OBS_TEST_CASE/unpacked/$package"
    command tar -xzf "$uploaded/${package}_1.2.3.orig.tar.gz" -C "$OBS_TEST_CASE/unpacked/$package"
    if grep -Rq 'Not Found' "$OBS_TEST_CASE/unpacked/$package"; then
        fail "$package archive contains an HTTP error response"; return 1
    fi
    if [ "$package" = kftray ]; then
        [ -f "$OBS_TEST_CASE/unpacked/kftray/kftray-1.2.3/kftray_1.2.3_aarch64.AppImage" ] || fail "missing aarch64 AppImage" || return 1
        [ -f "$OBS_TEST_CASE/unpacked/kftray/kftray-1.2.3/kftray_1.2.3_newer-glibc_aarch64.AppImage" ] || fail "missing newer-glibc AppImage" || return 1
    fi
    command tar -xJf "$uploaded/${package}_1.2.3-1.debian.tar.xz" -C "$OBS_TEST_CASE/unpacked/$package"
    [ -x "$OBS_TEST_CASE/unpacked/$package/debian/rules" ] || fail "Debian rules are not executable" || return 1
    if [ "$scenario" = changelog ]; then
        local changelog="$OBS_TEST_CASE/unpacked/$package/debian/changelog"
        grep -Fq 'Release notes with "quotes"' "$changelog" || fail "release notes lost during JSON parsing" || return 1
        grep -Fq '  * and a second line' "$changelog" || fail "CRLF release notes were not split into lines" || return 1
        ! grep -q $'\r' "$changelog" || fail "carriage returns leaked into the changelog" || return 1
        grep -Fq 'Fri, 02 Jan 2026 03:04:05 +0000' "$changelog" || fail "changelog date is not the release date" || return 1
    fi
    if command -v dpkg-source >/dev/null 2>&1; then
        dpkg-source -x "$uploaded/$package.dsc" "$OBS_TEST_CASE/source-$package" > "$OBS_TEST_CASE/dpkg-$package.log" 2>&1 || {
            cat "$OBS_TEST_CASE/dpkg-$package.log"
            fail "invalid $package Debian source package"; return 1
        }
        mkdir -p "$OBS_TEST_CASE/bin"
        for glibc in 2.36 2.39 2.40 3.0; do
            printf '%s\n' '#!/bin/sh' "printf 'glibc $glibc\\n'" > "$OBS_TEST_CASE/bin/getconf"
            chmod +x "$OBS_TEST_CASE/bin/getconf"
            for architecture in amd64 arm64; do
                PATH="$OBS_TEST_CASE/bin:$PATH" DEB_HOST_ARCH="$architecture" make -C "$OBS_TEST_CASE/source-$package" -f debian/rules override_dh_auto_install > "$OBS_TEST_CASE/install-$package.log" 2>&1 || {
                    cat "$OBS_TEST_CASE/install-$package.log"
                    fail "$package install failed for $architecture"; return 1
                }
                if [ "$package" = kftui ]; then
                    expected_file="kftui_linux_$architecture"
                elif [ "$glibc" = 2.36 ]; then
                    expected_file="kftray_1.2.3_${architecture/arm64/aarch64}.AppImage"
                else
                    expected_file="kftray_1.2.3_newer-glibc_${architecture/arm64/aarch64}.AppImage"
                fi
                cmp "$OBS_TEST_CASE/source-$package/$expected_file" "$OBS_TEST_CASE/source-$package/debian/$package/usr/bin/$package" || {
                    fail "$package installed the wrong binary for $architecture"; return 1
                }
            done
        done
    fi
    for file in "${package}_0.9.0.orig.tar.gz" PKGBUILD debian-changelog; do
        [ ! -f "$uploaded/$file" ] || fail "obsolete file $file retained in $package" || return 1
    done
}

run_case() (
    local scenario="$1" result=0 package expected_file digest assets='[]'
    export OBS_TEST_CASE="$OBS_TEST_ROOT/$scenario"
    export VERSION=1.2.3 OBS_USER=fixture OBS_PASSWORD=fixture
    export OBS_PROJECT=home:fixture:kftray
    unset GITHUB_TOKEN
    mkdir -p "$OBS_TEST_CASE/repo/hacks/obs" "$OBS_TEST_CASE/repo/package-unrelated"
    cp "$SCRIPT_DIR/publish.sh" "$SCRIPT_DIR/dry-run.sh" "$SCRIPT_DIR/distros.conf" "$OBS_TEST_CASE/repo/hacks/obs/"
    cp -R "$SCRIPT_DIR/kftui" "$SCRIPT_DIR/kftray" "$OBS_TEST_CASE/repo/hacks/obs/"
    printf 'keep\n' > "$OBS_TEST_CASE/repo/package-unrelated/sentinel"
    cd "$OBS_TEST_CASE/repo"
    for expected_file in kftui_linux_amd64 kftui_linux_arm64 kftray_1.2.3_amd64.AppImage kftray_1.2.3_aarch64.AppImage kftray_1.2.3_newer-glibc_amd64.AppImage kftray_1.2.3_newer-glibc_aarch64.AppImage; do
        digest=$(printf 'binary from https://github.com/hcavarsan/kftray/releases/download/v1.2.3/%s\n' "$expected_file" | sha256sum | cut -d' ' -f1)
        assets=$(jq --arg name "$expected_file" --arg digest "sha256:$digest" '. + [{name: $name, digest: $digest}]' <<< "$assets")
    done
    OBS_TEST_METADATA=$(jq -n --argjson assets "$assets" '{tag_name: "v1.2.3", draft: false, prerelease: false, published_at: "2026-01-02T03:04:05Z", body: "Release notes with \"quotes\"\r\nand a second line\r\n", assets: $assets}')
    export OBS_TEST_METADATA
    OBS_TEST_PROJECT_META=$(drifted_project_meta)
    export OBS_TEST_PROJECT_META
    case "$scenario" in
        missing-download) export OBS_TEST_HTTP_FAILURE=true ;;
        failed-archive) export OBS_TEST_TAR_FAILURE=true ;;
        failed-add) export OBS_TEST_ADD_FAILURE=true ;;
        obsolete-source) export OBS_TEST_OLD_SOURCES=true ;;
        v-prefixed-version) export VERSION=v1.2.3 ;;
        repeated-prefix) export VERSION=vv1.2.3 ;;
        prerelease-version) export VERSION=1.2.3-rc.1 ;;
        corrupt-download) export OBS_TEST_CORRUPT_DOWNLOAD=true ;;
        draft-release) OBS_TEST_METADATA=$(jq '.draft = true' <<< "$OBS_TEST_METADATA") ;;
        prerelease-release) OBS_TEST_METADATA=$(jq '.prerelease = true' <<< "$OBS_TEST_METADATA") ;;
        missing-digest) OBS_TEST_METADATA=$(jq '.assets[0].digest = null' <<< "$OBS_TEST_METADATA") ;;
        failed-build) export OBS_TEST_BUILD_STATE=failed ;;
        empty-build) export OBS_TEST_BUILD_STATE=empty ;;
        partial-empty-build) export OBS_TEST_BUILD_STATE=partial-empty ;;
        missing-code-build) export OBS_TEST_BUILD_STATE=missing-code ;;
        dirty-build) export OBS_TEST_BUILD_STATE=dirty ;;
        unresolved-build) export OBS_TEST_BUILD_STATE=unresolvable ;;
        stale-results) export OBS_TEST_STALE_POLLS=2 ;;
        scheduler-timeout) export OBS_TEST_STALE_POLLS=999 ;;
        project-missing|project-current) export OBS_TEST_PROJECT_MISSING=true ;;
        github-token) export GITHUB_TOKEN=fixture-token ;;
    esac
    run_publisher "$scenario" "$OBS_TEST_CASE/log" || result=$?
    if [ "$scenario" = reproducible ]; then
        cp -R "$OBS_TEST_CASE/upload" "$OBS_TEST_CASE/first-upload"
        export OBS_TEST_SOURCE_TIME=202101010000
        rm -f "$OBS_TEST_CASE"/committed-* "$OBS_TEST_CASE"/polls-*
        bash hacks/obs/publish.sh > "$OBS_TEST_CASE/repeat-log" 2>&1 || result=$?
        diff -r "$OBS_TEST_CASE/first-upload" "$OBS_TEST_CASE/upload" || fail "same release produced different package files" || return 1
    fi
    if [ "$scenario" = project-current ]; then
        [ "$result" -eq 0 ] || { cat "$OBS_TEST_CASE/log"; fail "initial project creation failed"; return 1; }
        OBS_TEST_PROJECT_META=$(cat "$OBS_TEST_CASE/project-meta.xml")
        export OBS_TEST_PROJECT_META
        unset OBS_TEST_PROJECT_MISSING
        rm -rf "$OBS_TEST_CASE/operations" "$OBS_TEST_CASE/project-meta.xml" "$OBS_TEST_CASE/upload" "$OBS_TEST_CASE"/committed-* "$OBS_TEST_CASE"/polls-*
        bash hacks/obs/publish.sh > "$OBS_TEST_CASE/repeat-log" 2>&1 || result=$?
    fi
    case "$scenario" in
        missing-download|failed-archive|failed-add|unknown-package|corrupt-download|missing-digest|draft-release|prerelease-release|repeated-prefix|prerelease-version)
            [ "$result" -ne 0 ] || fail "$scenario reported success" || return 1
            [ ! -d "$OBS_TEST_CASE/upload" ] || fail "$scenario uploaded incomplete packages" || return 1
            [ "$scenario" = failed-add ] || [ ! -f "$OBS_TEST_CASE/project-meta.xml" ] || fail "$scenario changed the project before packages were ready" || return 1
            ;;
        failed-build|empty-build|partial-empty-build|missing-code-build|dirty-build|unresolved-build)
            [ "$result" -ne 0 ] || fail "$scenario reported success" || return 1
            ;;
        scheduler-timeout)
            [ "$result" -ne 0 ] || fail "$scenario reported success" || return 1
            grep -q 'scheduler did not pick up' "$OBS_TEST_CASE/log" || fail "$scenario failed for another reason" || return 1
            ;;
        dry-run)
            [ "$result" -eq 0 ] || { cat "$OBS_TEST_CASE/log"; fail "dry-run failed"; return 1; }
            [ ! -e "$OBS_TEST_CASE/operations" ] || fail "dry-run contacted OBS" || return 1
            [ -s "$OBS_TEST_CASE/downloads" ] || fail "dry-run skipped real preparation" || return 1
            grep -Fq 'project="openSUSE:Tumbleweed" repository="standard"' "$OBS_TEST_CASE/log" || fail "invalid dry-run repositories" || return 1
            grep -Fq 'project="openSUSE:Leap:16.0" repository="standard"' "$OBS_TEST_CASE/log" || fail "multi-level OBS project path lost" || return 1
            ;;
        dry-run-help)
            [ "$result" -eq 0 ] || fail "dry-run help required credentials/version" || return 1
            [ ! -e "$OBS_TEST_CASE/operations" ] && [ ! -e "$OBS_TEST_CASE/downloads" ] || fail "help contacted external services" || return 1
            ;;
        preserve-unrelated)
            [ -f package-unrelated/sentinel ] || fail "cleanup deleted an unrelated directory" || return 1
            ;;
        *)
            [ "$result" -eq 0 ] || { cat "$OBS_TEST_CASE/log"; fail "$scenario exited $result"; return 1; }
            for package in kftui kftray; do
                check_uploaded_package "$scenario" "$package" || return 1
            done
            ;;
    esac
    case "$scenario" in
        github-token)
            grep -Fxq 'Authorization: Bearer fixture-token' "$OBS_TEST_CASE/auth" || fail "GitHub API calls were not authenticated" || return 1
            ;;
        archives)
            [ ! -e "$OBS_TEST_CASE/auth" ] || fail "Authorization header sent without a token" || return 1
            grep -q '^meta prj -F ' "$OBS_TEST_CASE/operations" || fail "drifted project metadata was not updated" || return 1
            for expected_file in Debian_13 Fedora_44 openSUSE_Leap_16.0; do
                grep -Fq "<repository name=\"$expected_file\">" "$OBS_TEST_CASE/project-meta.xml" || fail "missing $expected_file in project metadata" || return 1
            done
            ! grep -Fq 'Ubuntu_20.04' "$OBS_TEST_CASE/project-meta.xml" || fail "obsolete Ubuntu 20.04 target retained" || return 1
            grep -Fq '<title>Custom title</title>' "$OBS_TEST_CASE/project-meta.xml" || fail "project title was overwritten" || return 1
            grep -Fq '<person userid="other" role="bugowner" />' "$OBS_TEST_CASE/project-meta.xml" || fail "extra maintainer was dropped" || return 1
            grep -Fq '<disable arch="i586" />' "$OBS_TEST_CASE/project-meta.xml" || fail "build flags were dropped" || return 1
            grep -Fq 'project="openSUSE:Leap:16.0"' "$OBS_TEST_CASE/project-meta.xml" || fail "multi-level OBS project path lost" || return 1
            ;;
        project-missing)
            grep -Fq '<project name="home:fixture:kftray">' "$OBS_TEST_CASE/project-meta.xml" || fail "project was not created" || return 1
            grep -Fq '<person userid="fixture" role="maintainer" />' "$OBS_TEST_CASE/project-meta.xml" || fail "maintainer missing from new project" || return 1
            grep -Fq '<repository name="Ubuntu_22.04">' "$OBS_TEST_CASE/project-meta.xml" || fail "repositories missing from new project" || return 1
            ;;
        project-current)
            [ "$result" -eq 0 ] || { cat "$OBS_TEST_CASE/repeat-log"; fail "publishing against a matching project failed"; return 1; }
            ! grep -q '^meta prj -F ' "$OBS_TEST_CASE/operations" || fail "matching project metadata was rewritten" || return 1
            ;;
        stale-results)
            [ "$(cat "$OBS_TEST_CASE/polls-kftui")" -gt 2 ] || fail "publisher trusted stale build results" || return 1
            ;;
    esac
    printf 'PASS: %s\n' "$scenario"
)

failures=0
for scenario in archives metadata missing-download failed-archive failed-add obsolete-source v-prefixed-version unknown-package preserve-unrelated corrupt-download missing-digest draft-release prerelease-release repeated-prefix prerelease-version dry-run dry-run-help changelog failed-build empty-build partial-empty-build missing-code-build dirty-build unresolved-build stale-results scheduler-timeout project-missing project-current github-token reproducible; do
    if run_case "$scenario"; then
        :
    else
        failures=$((failures + 1))
    fi
done
[ "$failures" -eq 0 ] || { printf '%s regression checks failed\n' "$failures" >&2; exit 1; }

#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUILD_TEST_DIR=$(mktemp -d)
trap 'rm -rf -- "$BUILD_TEST_DIR"' EXIT
formats="${*:-deb rpm}"

dbus_flags_text=$(pkg-config --cflags --libs dbus-1)
read -r -a dbus_flags <<< "$dbus_flags_text"
cc -x c -o "$BUILD_TEST_DIR/kftui" - "${dbus_flags[@]}" <<'EOF'
#include <dbus/dbus.h>
int main(void) { int major, minor, micro; dbus_get_version(&major, &minor, &micro); return 0; }
EOF

if [ -n "${OBS_TEST_APPIMAGE:-}" ]; then
    cp "$OBS_TEST_APPIMAGE" "$BUILD_TEST_DIR/kftray"
else
    cc -static -x c -o "$BUILD_TEST_DIR/kftray" - <<'EOF'
int main(void) { return 0; }
EOF
    printf '\nopaque embedded filesystem payload\n' >> "$BUILD_TEST_DIR/kftray"
fi
cp "${OBS_TEST_NEWER_APPIMAGE:-$BUILD_TEST_DIR/kftray}" "$BUILD_TEST_DIR/kftray-newer"
if [ -z "${OBS_TEST_APPIMAGE:-}" ] && [ -z "${OBS_TEST_NEWER_APPIMAGE:-}" ]; then
    printf '\nnewer-glibc fixture\n' >> "$BUILD_TEST_DIR/kftray-newer"
fi
expected_appimage="$BUILD_TEST_DIR/kftray"
if getconf GNU_LIBC_VERSION | awk -F '[ .]' '{ exit !($2 > 2 || ($2 == 2 && $3 >= 39)) }'; then
    expected_appimage="$BUILD_TEST_DIR/kftray-newer"
fi

for package in kftui kftray; do
    source_dir="$BUILD_TEST_DIR/$package-1.2.3"
    mkdir -p "$source_dir/debian" "$BUILD_TEST_DIR/rpm/SOURCES" "$BUILD_TEST_DIR/rpm/SPECS"
    if [ "$package" = kftui ]; then
        cp "$BUILD_TEST_DIR/kftui" "$source_dir/kftui_linux_amd64"
        cp "$BUILD_TEST_DIR/kftui" "$source_dir/kftui_linux_arm64"
    else
        cp "$BUILD_TEST_DIR/kftray" "$source_dir/kftray_1.2.3_amd64.AppImage"
        cp "$BUILD_TEST_DIR/kftray" "$source_dir/kftray_1.2.3_aarch64.AppImage"
        cp "$BUILD_TEST_DIR/kftray-newer" "$source_dir/kftray_1.2.3_newer-glibc_amd64.AppImage"
        cp "$BUILD_TEST_DIR/kftray-newer" "$source_dir/kftray_1.2.3_newer-glibc_aarch64.AppImage"
    fi
    for template in "$SCRIPT_DIR/$package/templates/debian-"*; do
        name=${template##*/debian-}
        sed 's/{{VERSION}}/1.2.3/g' "$template" > "$source_dir/debian/$name"
    done
    chmod +x "$source_dir/debian/rules"
    printf '%s (1.2.3-1) stable; urgency=low\n\n  * Packaging test\n\n -- Build Test <test@example.invalid>  Fri, 02 Jan 2026 03:04:05 +0000\n' "$package" > "$source_dir/debian/changelog"
    tar -czf "$BUILD_TEST_DIR/rpm/SOURCES/${package}_1.2.3.orig.tar.gz" -C "$BUILD_TEST_DIR" "$package-1.2.3"
    sed 's/{{VERSION}}/1.2.3/g' "$SCRIPT_DIR/$package/templates/$package.spec" > "$BUILD_TEST_DIR/rpm/SPECS/$package.spec"

    if [[ " $formats " == *" deb "* ]]; then
        if ! (cd "$source_dir" && dpkg-buildpackage -us -uc -b) > "$BUILD_TEST_DIR/deb-$package.log" 2>&1; then
            cat "$BUILD_TEST_DIR/deb-$package.log" >&2
            exit 1
        fi
        deb="$BUILD_TEST_DIR/${package}_1.2.3-1_$(dpkg --print-architecture).deb"
        dpkg-deb -x "$deb" "$BUILD_TEST_DIR/deb-$package"
        if [ "$package" = kftray ]; then
            cmp "$expected_appimage" "$BUILD_TEST_DIR/deb-kftray/usr/bin/kftray"
            dpkg-deb -f "$deb" Depends | grep -q fuse3
        else
            dpkg-deb -f "$deb" Depends | grep -q libdbus-1-3
            "$BUILD_TEST_DIR/deb-kftui/usr/bin/kftui"
        fi
        printf 'PASS: %s Debian build, payload and dependencies\n' "$package"
    fi

    if [[ " $formats " == *" rpm "* ]]; then
        if ! rpmbuild -bb --define "_topdir $BUILD_TEST_DIR/rpm" "$BUILD_TEST_DIR/rpm/SPECS/$package.spec" > "$BUILD_TEST_DIR/rpm-$package.log" 2>&1; then
            cat "$BUILD_TEST_DIR/rpm-$package.log" >&2
            exit 1
        fi
        rpm_file=$(find "$BUILD_TEST_DIR/rpm/RPMS" -name "$package-1.2.3-*.rpm" -print -quit)
        mkdir -p "$BUILD_TEST_DIR/rpm-$package"
        rpm2cpio "$rpm_file" | (cd "$BUILD_TEST_DIR/rpm-$package" && cpio -id --quiet)
        if [ "$package" = kftray ]; then
            cmp "$expected_appimage" "$BUILD_TEST_DIR/rpm-kftray/usr/bin/kftray"
            rpm -qp --requires "$rpm_file" | grep -q fuse3
        else
            "$BUILD_TEST_DIR/rpm-kftui/usr/bin/kftui"
        fi
        printf 'PASS: %s RPM build and payload\n' "$package"
    fi
done

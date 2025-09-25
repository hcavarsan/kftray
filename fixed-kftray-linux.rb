class KftrayLinux < Formula
    desc "A cross-platform system tray app for Kubernetes port-forward management."
    homepage "https://github.com/hcavarsan/kftray"
    version "0.26.1"

    NEWER_GLIBC_AMD64_URL = "https://github.com/hcavarsan/kftray/releases/download/v0.26.1/kftray_0.26.1_newer-glibc_amd64.AppImage"
    NEWER_GLIBC_AMD64_SHA = "5d081b153ade66583905cfa8080f1f34bcd053db45a4fc501b75a6a37fa39daf"
    NEWER_GLIBC_ARM64_URL = "https://github.com/hcavarsan/kftray/releases/download/v0.26.1/kftray_0.26.1_newer-glibc_aarch64.AppImage"
    NEWER_GLIBC_ARM64_SHA = "ee61b918fb529d98bc650846ab96cf4177c5e6009e2842a44e2e5ed1931d0805"
    LEGACY_AMD64_URL = "https://github.com/hcavarsan/kftray/releases/download/v0.26.1/kftray_0.26.1_amd64.AppImage"
    LEGACY_AMD64_SHA = "98fcfd2236da6117be716fa9f174b62502e7fb8b6e0dc29b5c1e76e5d13c4cb2"
    LEGACY_ARM64_URL = "https://github.com/hcavarsan/kftray/releases/download/v0.26.1/kftray_0.26.1_aarch64.AppImage"
    LEGACY_ARM64_SHA = "9ad0eca72a4deda7970b7d0b585c679edc09e08c01c500be198ad006f0aa83d3"

    url LEGACY_AMD64_URL
    sha256 LEGACY_AMD64_SHA

    def self.select_variant
        os_variant = :legacy

        if OS.linux? && File.exist?("/etc/os-release")
            os_release = File.read("/etc/os-release")

            if os_release.match(/^NAME.*Ubuntu/mi)
                version_match = os_release.match(/^VERSION_ID="(\d+)\.?\d*"/mi)
                os_variant = :newer_glibc if version_match && version_match[1].to_i >= 24
            end

            if os_release.match(/^NAME.*Debian/mi)
                version_match = os_release.match(/^VERSION_ID="(\d+)"/mi)
                os_variant = :newer_glibc if version_match && version_match[1].to_i >= 13
            end
        end

        arch = if Hardware::CPU.arm?
            :arm64
        else
            :amd64
        end

        { os: os_variant, arch: arch }
    end

    def install
        variant = self.class.select_variant

        selected_url = nil
        selected_sha = nil
        selected_filename = nil

        if variant[:os] == :newer_glibc && variant[:arch] == :amd64
            selected_url = NEWER_GLIBC_AMD64_URL
            selected_sha = NEWER_GLIBC_AMD64_SHA
            selected_filename = "kftray_#{version}_newer-glibc_amd64.AppImage"
        elsif variant[:os] == :newer_glibc && variant[:arch] == :arm64
            selected_url = NEWER_GLIBC_ARM64_URL
            selected_sha = NEWER_GLIBC_ARM64_SHA
            selected_filename = "kftray_#{version}_newer-glibc_aarch64.AppImage"
        elsif variant[:os] == :legacy && variant[:arch] == :arm64
            selected_url = LEGACY_ARM64_URL
            selected_sha = LEGACY_ARM64_SHA
            selected_filename = "kftray_#{version}_aarch64.AppImage"
        else
            selected_url = LEGACY_AMD64_URL
            selected_sha = LEGACY_AMD64_SHA
            selected_filename = "kftray_#{version}_amd64.AppImage"
        end

        final_appimage_name = nil

        if selected_url != url.to_s
            system "curl", "-L", "-o", selected_filename, selected_url
            downloaded_sha = `shasum -a 256 #{selected_filename}`.split.first

            unless downloaded_sha == selected_sha
                odie "SHA256 mismatch for #{selected_filename}: expected #{selected_sha}, got #{downloaded_sha}"
            end

            chmod 0755, selected_filename
            prefix.install selected_filename
            bin.install_symlink("#{prefix}/#{selected_filename}" => "kftray")
            final_appimage_name = selected_filename
        else
            appimage_name = url.to_s.split("/").last
            prefix.install Dir["*"]
            chmod(0755, "#{prefix}/#{appimage_name}")
            bin.install_symlink("#{prefix}/#{appimage_name}" => "kftray")
            final_appimage_name = appimage_name
        end

        desktop_content = <<~DESKTOP
        [Desktop Entry]
        Version=1.0
        Type=Application
        Name=kftray
        Comment=A cross-platform system tray app for Kubernetes port-forward management
        Exec=#{bin}/kftray
        Icon=kftray
        Categories=Development;Network;
        Terminal=false
        StartupWMClass=kftray
        StartupNotify=true
        MimeType=
        Keywords=kubernetes;k8s;port-forward;tray;
        DESKTOP

        desktop_dir = share/"applications"
        desktop_dir.mkpath
        (desktop_dir/"kftray.desktop").write desktop_content

        icon_configs = [
            { size: "32", file: "32x32.png" },
            { size: "128", file: "128x128.png" }
        ]

        icon_configs.each do |config|
            icon_dir = share/"icons/hicolor/#{config[:size]}x#{config[:size]}/apps"
            icon_dir.mkpath

            system "curl", "-L", "-o", "kftray-#{config[:size]}.png",
                   "https://raw.githubusercontent.com/hcavarsan/kftray/main/crates/kftray-tauri/icons/#{config[:file]}"

            if File.exist?("kftray-#{config[:size]}.png")
                (icon_dir/"kftray.png").write File.read("kftray-#{config[:size]}.png")
                rm "kftray-#{config[:size]}.png"
            end
        end

        scalable_icon_dir = share/"icons/hicolor/scalable/apps"
        scalable_icon_dir.mkpath
        system "curl", "-L", "-o", "kftray.svg",
               "https://raw.githubusercontent.com/hcavarsan/kftray/main/img/logo.svg"

        if File.exist?("kftray.svg")
            (scalable_icon_dir/"kftray.svg").write File.read("kftray.svg")
            rm "kftray.svg"
        end

        large_icon_dir = share/"icons/hicolor/256x256/apps"
        large_icon_dir.mkpath
        system "curl", "-L", "-o", "kftray-256.png",
               "https://raw.githubusercontent.com/hcavarsan/kftray/main/icon.png"

        if File.exist?("kftray-256.png")
            (large_icon_dir/"kftray.png").write File.read("kftray-256.png")
            rm "kftray-256.png"
        end
    end

    def caveats
        variant = self.class.select_variant
        arch_str = variant[:arch] == :arm64 ? "ARM64" : "AMD64"
        os_str = variant[:os] == :newer_glibc ? "newer glibc (Ubuntu 24+/Debian 13+)" : "legacy glibc"

        <<~EOS
        ================================

        Executable is linked as "kftray".
        Installed: #{os_str} for #{arch_str}

        Version selection is automatic based on your system:
        - OS: Ubuntu 24.04+/Debian 13+ uses newer glibc, others use legacy
        - Architecture: #{arch_str} detected

        ================================

        DESKTOP INTEGRATION:

        Desktop entry and icons have been installed:
        - Desktop file: ~/.linuxbrew/share/applications/kftray.desktop
        - Icons: ~/.linuxbrew/share/icons/hicolor/*/apps/kftray.*

        To update desktop database (optional):
        update-desktop-database ~/.linuxbrew/share/applications 2>/dev/null || true

        ================================

        REQUIRED for Linux systems:

        1. Install GNOME Shell extension for AppIndicator support:
           https://extensions.gnome.org/extension/615/appindicator-support/

        2. If kftray doesn't start, install missing system dependencies:
           sudo apt install libayatana-appindicator3-dev librsvg2-dev

        ================================

        EOS
    end
end
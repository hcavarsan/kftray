class KftrayLinux < Formula
    desc "A cross-platform system tray app for Kubernetes port-forward management."
    homepage "https://github.com/hcavarsan/kftray"
    version "0.26.1"

    on_linux do
        on_intel do
            url "https://github.com/hcavarsan/kftray/releases/download/v0.26.1/kftray_0.26.1_amd64.AppImage"
            sha256 "98fcfd2236da6117be716fa9f174b62502e7fb8b6e0dc29b5c1e76e5d13c4cb2"
        end

        on_arm do
            url "https://github.com/hcavarsan/kftray/releases/download/v0.26.1/kftray_0.26.1_aarch64.AppImage"
            sha256 "9ad0eca72a4deda7970b7d0b585c679edc09e08c01c500be198ad006f0aa83d3"
        end
    end

    def install
        selected_url = nil
        selected_filename = nil

        if OS.linux? && File.exist?("/etc/os-release")
            os_release = File.read("/etc/os-release")
            use_newer_glibc = false

            if os_release.match(/^NAME.*Ubuntu/mi)
                version_match = os_release.match(/^VERSION_ID="(\d+)\.?\d*"/mi)
                use_newer_glibc = version_match && version_match[1].to_i >= 24
            elsif os_release.match(/^NAME.*Debian/mi)
                version_match = os_release.match(/^VERSION_ID="(\d+)"/mi)
                use_newer_glibc = version_match && version_match[1].to_i >= 13
            end

            if use_newer_glibc
                if Hardware::CPU.arm?
                    selected_url = "https://github.com/hcavarsan/kftray/releases/download/v#{version}/kftray_#{version}_newer-glibc_aarch64.AppImage"
                    selected_filename = "kftray_#{version}_newer-glibc_aarch64.AppImage"
                else
                    selected_url = "https://github.com/hcavarsan/kftray/releases/download/v#{version}/kftray_#{version}_newer-glibc_amd64.AppImage"
                    selected_filename = "kftray_#{version}_newer-glibc_amd64.AppImage"
                end

                system "curl", "-L", "-o", selected_filename, selected_url
                system "chmod", "755", selected_filename
                prefix.install selected_filename
                bin.install_symlink("#{prefix}/#{selected_filename}" => "kftray")
                return
            end
        end

        appimage_name = url.split("/").last
        prefix.install Dir["*"]
        chmod(0755, "#{prefix}/#{appimage_name}")
        bin.install_symlink("#{prefix}/#{appimage_name}" => "kftray")
    end

    def caveats
        variant_info = ""

        if OS.linux? && File.exist?("/etc/os-release")
            os_release = File.read("/etc/os-release")
            arch_str = Hardware::CPU.arm? ? "ARM64" : "AMD64"

            if os_release.match(/^NAME.*Ubuntu/mi)
                version_match = os_release.match(/^VERSION_ID="(\d+)\.?\d*"/mi)
                if version_match && version_match[1].to_i >= 24
                    variant_info = "Installed: newer glibc (Ubuntu #{version_match[1]}+) for #{arch_str}"
                else
                    variant_info = "Installed: legacy glibc (Ubuntu #{version_match[1] if version_match}) for #{arch_str}"
                end
            elsif os_release.match(/^NAME.*Debian/mi)
                version_match = os_release.match(/^VERSION_ID="(\d+)"/mi)
                if version_match && version_match[1].to_i >= 13
                    variant_info = "Installed: newer glibc (Debian #{version_match[1]}+) for #{arch_str}"
                else
                    variant_info = "Installed: legacy glibc (Debian #{version_match[1] if version_match}) for #{arch_str}"
                end
            else
                variant_info = "Installed: legacy glibc (unknown distro) for #{arch_str}"
            end
        end

        <<~EOS
        ================================

        Executable is linked as "kftray".
        #{variant_info}

        Version selection is automatic based on your system:
        - OS: Ubuntu 24.04+/Debian 13+ uses newer glibc, others use legacy
        - Architecture: Auto-detected

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
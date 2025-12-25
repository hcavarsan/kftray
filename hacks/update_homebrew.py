#!/usr/bin/env python3

import hashlib
import re
import requests
import subprocess
import tempfile
import sys
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor, as_completed


class HomebrewUpdater:
    def __init__(self, repo, version, tap_repo, gh_token, dry_run=False, no_gpg_sign=False):
        self.repo = repo
        self.version = version.lstrip('v')
        self.full_version = version if version.startswith('v') else f'v{version}'
        self.tap_repo = tap_repo
        self.gh_token = gh_token
        self.dry_run = dry_run
        self.no_gpg_sign = no_gpg_sign
        self.base_url = f"https://github.com/{repo}/releases/download/{self.full_version}/"

    def download_and_hash(self, filename):
        """Download a file and return its SHA256 hash"""
        url = f"{self.base_url}{filename}"
        headers = {'Authorization': f'token {self.gh_token}'}

        print(f"Downloading: {url}")
        response = requests.get(url, headers=headers, stream=True)
        response.raise_for_status()

        hash_sha256 = hashlib.sha256()
        for chunk in response.iter_content(chunk_size=8192):
            hash_sha256.update(chunk)

        return filename, hash_sha256.hexdigest()

    def download_files_parallel(self, filenames):
        """Download multiple files in parallel and return their SHA256 hashes"""
        results = {}

        with ThreadPoolExecutor(max_workers=4) as executor:
            future_to_filename = {
                executor.submit(self.download_and_hash, filename): filename
                for filename in filenames
            }

            for future in as_completed(future_to_filename):
                filename, hash_value = future.result()
                results[filename] = hash_value

        return results

    def update_file_simple(self, file_path, version=None, url=None, sha256=None):
        """Simple find and replace for version, url, and sha256"""
        with open(file_path, 'r') as f:
            content = f.read()

        if version:
            content = re.sub(r'version\s+"[^"]+"', f'version "{version}"', content)
        if url:
            content = re.sub(r'url\s+"[^"]+"', f'url "{url}"', content)
        if sha256:
            content = re.sub(r'sha256\s+"[^"]+"', f'sha256 "{sha256}"', content)

        with open(file_path, 'w') as f:
            f.write(content)

    def update_kftray_linux_formula(self, file_path, amd64_hash, arm64_hash, newer_amd64_hash, newer_arm64_hash):
        """Update the Linux formula file using embedded template - KISS approach"""
        # Embedded template to ensure it's always available
        template_content = '''require "digest"

class KftrayLinux < Formula
  desc "A cross-platform system tray app for Kubernetes port-forward management."
  homepage "https://github.com/hcavarsan/kftray"
  version "{{VERSION}}"

  NEWER_GLIBC_AMD64_SHA = "{{NEWER_GLIBC_AMD64_SHA}}"
  NEWER_GLIBC_ARM64_SHA = "{{NEWER_GLIBC_ARM64_SHA}}"


  on_linux do
      on_intel do
          url "{{AMD64_URL}}"
          sha256 "{{AMD64_SHA}}"
      end

      on_arm do
          url "{{ARM64_URL}}"
          sha256 "{{ARM64_SHA}}"
      end
  end

  def install
      if needs_newer_glibc?
          download_newer_glibc_variant
      else
          install_default_appimage
      end

      install_desktop_integration
  end

  def post_install
      setup_user_directories
      copy_desktop_files
      update_desktop_database
  end

  def caveats
      arch_str = Hardware::CPU.arm? ? "ARM64" : "AMD64"
      variant_info = if OS.linux? && File.exist?("/etc/os-release")
          os_release = File.read("/etc/os-release")
          if os_release.match(/^NAME.*Ubuntu/mi)
              version_match = os_release.match(/^VERSION_ID="(\\d+)\\.?\\d*"/mi)
              if version_match && version_match[1].to_i >= 24
                  "Installed: newer glibc (Ubuntu #{version_match[1]}+) for #{arch_str}"
              else
                  "Installed: legacy glibc (Ubuntu #{version_match[1] if version_match}) for #{arch_str}"
              end
          elsif os_release.match(/^NAME.*Debian/mi)
              version_match = os_release.match(/^VERSION_ID="(\\d+)"/mi)
              if version_match && version_match[1].to_i >= 13
                  "Installed: newer glibc (Debian #{version_match[1]}+) for #{arch_str}"
              else
                  "Installed: legacy glibc (Debian #{version_match[1] if version_match}) for #{arch_str}"
              end
          else
              "Installed: legacy glibc (unknown distro) for #{arch_str}"
          end
      else
          "Installed: legacy glibc (non-Linux)"
      end

      <<~EOS
        ================================

        Executable is linked as "kftray".
        #{variant_info}

        Version selection is automatic based on your system:
        - OS: Ubuntu 24.04+/Debian 13+ uses newer glibc, others use legacy
        - Architecture: Auto-detected (#{arch_str})

        ================================

        DESKTOP INTEGRATION:

        Desktop entry and icons have been installed to both system and user locations:
        - System: #{HOMEBREW_PREFIX}/share/applications/kftray.desktop
        - User: ~/.local/share/applications/kftray.desktop
        - Icons: ~/.local/share/icons/hicolor/*/apps/kftray.*

        To refresh the desktop database:
        update-desktop-database ~/.local/share/applications 2>/dev/null || true

        ================================

        REQUIRED for Linux systems:

        1. Install GNOME Shell extension for AppIndicator support:
           https://extensions.gnome.org/extension/615/appindicator-support/

        2. If kftray doesn't start, install missing system dependencies:
           sudo apt install libayatana-appindicator3-dev librsvg2-dev

        ================================
      EOS
  end

  private

  def needs_newer_glibc?
      return false unless OS.linux? && File.exist?("/etc/os-release")

      os_release = File.read("/etc/os-release")

      if os_release.match(/^NAME.*Ubuntu/mi)
          version_match = os_release.match(/^VERSION_ID="(\\d+)\\.?\\d*"/mi)
          version_match && version_match[1].to_i >= 24
      elsif os_release.match(/^NAME.*Debian/mi)
          version_match = os_release.match(/^VERSION_ID="(\\d+)"/mi)
          version_match && version_match[1].to_i >= 13
      else
          false
      end
  end

  def download_newer_glibc_variant
      arch_suffix = Hardware::CPU.arm? ? "aarch64" : "amd64"
      filename = "kftray_#{version}_newer-glibc_#{arch_suffix}.AppImage"
      download_url = "https://github.com/hcavarsan/kftray/releases/download/v#{version}/#{filename}"
      expected_sha = Hardware::CPU.arm? ? self.class::NEWER_GLIBC_ARM64_SHA : self.class::NEWER_GLIBC_AMD64_SHA

      system "curl", "-L", "-o", filename, download_url

      # Verify SHA256
      actual_sha = Digest::SHA256.file(filename).hexdigest
      unless actual_sha == expected_sha
          odie "SHA256 mismatch for #{filename}. Expected: #{expected_sha}, Got: #{actual_sha}"
      end

      install_appimage(filename)
  end

  def install_default_appimage
      downloaded_files = Dir["*.AppImage"]
      odie "No AppImage file found after download" if downloaded_files.empty?

      install_appimage(downloaded_files.first)
  end

  def install_appimage(filename)
      system "chmod", "755", filename
      prefix.install filename
      bin.install_symlink("#{prefix}/#{filename}" => "kftray")
  end

  def install_desktop_integration
      create_desktop_file
      install_icons
  end

  def create_desktop_file
      desktop_content = <<~DESKTOP
        [Desktop Entry]
        Version=1.0
        Type=Application
        Name=kftray
        Comment=A cross-platform system tray app for Kubernetes port-forward management
        Exec=#{HOMEBREW_PREFIX}/bin/kftray
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
  end

  def install_icons
      icon_sizes = ["32x32", "48x48", "64x64", "128x128", "256x256"]
      icon_url = "https://raw.githubusercontent.com/hcavarsan/kftray-blog/main/img/logo.png"

      icon_sizes.each do |size|
          icon_dir = share/"icons/hicolor/#{size}/apps"
          icon_dir.mkpath

          tmp_icon = "kftray-#{size}.png"
          system "curl", "-L", "-o", tmp_icon, icon_url

          if File.exist?(tmp_icon)
              (icon_dir/"kftray.png").write File.read(tmp_icon)
              rm tmp_icon
          end
      end
  end

  private

  def setup_user_directories
      user_apps_dir = "#{ENV["HOME"]}/.local/share/applications"
      system "mkdir", "-p", user_apps_dir

      ["32x32", "48x48", "64x64", "128x128", "256x256"].each do |size|
          icon_dir = "#{ENV["HOME"]}/.local/share/icons/hicolor/#{size}/apps"
          system "mkdir", "-p", icon_dir
      end
  end

  def copy_desktop_files
      desktop_file = "#{HOMEBREW_PREFIX}/share/applications/kftray.desktop"
      user_desktop_file = "#{ENV["HOME"]}/.local/share/applications/kftray.desktop"
      system "cp", desktop_file, user_desktop_file

      ["32x32", "48x48", "64x64", "128x128", "256x256"].each do |size|
          src_icon = "#{HOMEBREW_PREFIX}/share/icons/hicolor/#{size}/apps/kftray.png"
          dst_icon = "#{ENV["HOME"]}/.local/share/icons/hicolor/#{size}/apps/kftray.png"
          system "cp", src_icon, dst_icon rescue nil
      end
  end

  def update_desktop_database
      user_apps_dir = "#{ENV["HOME"]}/.local/share/applications"
      user_icons_dir = "#{ENV["HOME"]}/.local/share/icons/hicolor/"

      system "update-desktop-database", user_apps_dir rescue nil
      system "gtk-update-icon-cache", user_icons_dir, "--force", "--quiet" rescue nil
  end

end'''

        # Replace template variables
        amd64_url = f"{self.base_url}kftray_{self.version}_amd64.AppImage"
        arm64_url = f"{self.base_url}kftray_{self.version}_aarch64.AppImage"

        content = template_content.replace("{{VERSION}}", self.version)
        content = content.replace("{{NEWER_GLIBC_AMD64_SHA}}", newer_amd64_hash)
        content = content.replace("{{NEWER_GLIBC_ARM64_SHA}}", newer_arm64_hash)
        content = content.replace("{{AMD64_URL}}", amd64_url)
        content = content.replace("{{AMD64_SHA}}", amd64_hash)
        content = content.replace("{{ARM64_URL}}", arm64_url)
        content = content.replace("{{ARM64_SHA}}", arm64_hash)

        with open(file_path, 'w') as f:
            f.write(content)

    def update_kftui_formula(self, file_path, mac_hash, amd64_hash, arm64_hash):
        """Update the kftui formula with ordered hash replacement"""
        with open(file_path, 'r') as f:
            content = f.read()

        # Update URLs first
        mac_url = f"{self.base_url}kftui_macos_universal"
        amd64_url = f"{self.base_url}kftui_linux_amd64"
        arm64_url = f"{self.base_url}kftui_linux_arm64"

        content = re.sub(
            r'https://github\.com/[^/]+/kftray/releases/download/v[^/]+/kftui_macos_universal',
            mac_url,
            content
        )
        content = re.sub(
            r'https://github\.com/[^/]+/kftray/releases/download/v[^/]+/kftui_linux_amd64',
            amd64_url,
            content
        )
        content = re.sub(
            r'https://github\.com/[^/]+/kftray/releases/download/v[^/]+/kftui_linux_arm64',
            arm64_url,
            content
        )

        # Update SHA256 hashes in order (1st=macOS, 2nd=Linux AMD64, 3rd=Linux ARM64)
        sha_values = [mac_hash, amd64_hash, arm64_hash]
        sha_count = 0

        def replace_sha(match):
            nonlocal sha_count
            if sha_count < len(sha_values):
                replacement = f'sha256 "{sha_values[sha_count]}"'
                sha_count += 1
                return replacement
            return match.group(0)

        content = re.sub(r'sha256\s+"[^"]+"', replace_sha, content)

        with open(file_path, 'w') as f:
            f.write(content)

    def show_file_content(self, file_path, title):
        """Show file content for dry run"""
        print(f"\n{'='*60}")
        print(f"UPDATED FILE: {title}")
        print('='*60)
        with open(file_path, 'r') as f:
            content = f.read()
            print(content)
        print('='*60)

    def run(self):
        """Main execution flow"""
        mode = "DRY RUN" if self.dry_run else "LIVE RUN"
        print(f"[{mode}] Updating homebrew formulas for {self.repo} version {self.full_version}")

        # Download all files in parallel
        print("Downloading and calculating hashes for all assets...")
        all_files = [
            f"kftray_{self.version}_universal.app.tar.gz",
            f"kftray_{self.version}_amd64.AppImage",
            f"kftray_{self.version}_aarch64.AppImage",
            f"kftray_{self.version}_newer-glibc_amd64.AppImage",
            f"kftray_{self.version}_newer-glibc_aarch64.AppImage",
            "kftui_macos_universal",
            "kftui_linux_amd64",
            "kftui_linux_arm64"
        ]

        hashes = self.download_files_parallel(all_files)

        # Extract individual hashes
        kftray_mac_hash = hashes[f"kftray_{self.version}_universal.app.tar.gz"]
        kftray_amd64_hash = hashes[f"kftray_{self.version}_amd64.AppImage"]
        kftray_arm64_hash = hashes[f"kftray_{self.version}_aarch64.AppImage"]
        kftray_newer_amd64_hash = hashes[f"kftray_{self.version}_newer-glibc_amd64.AppImage"]
        kftray_newer_arm64_hash = hashes[f"kftray_{self.version}_newer-glibc_aarch64.AppImage"]
        kftui_mac_hash = hashes["kftui_macos_universal"]
        kftui_amd64_hash = hashes["kftui_linux_amd64"]
        kftui_arm64_hash = hashes["kftui_linux_arm64"]

        print("Hashes calculated:")
        print(f"kftray macOS: {kftray_mac_hash}")
        print(f"kftray Linux AMD64 (older glibc): {kftray_amd64_hash}")
        print(f"kftray Linux ARM64 (older glibc): {kftray_arm64_hash}")
        print(f"kftray Linux AMD64 (newer glibc): {kftray_newer_amd64_hash}")
        print(f"kftray Linux ARM64 (newer glibc): {kftray_newer_arm64_hash}")
        print(f"kftui macOS: {kftui_mac_hash}")
        print(f"kftui Linux AMD64: {kftui_amd64_hash}")
        print(f"kftui Linux ARM64: {kftui_arm64_hash}")

        # Clone homebrew tap
        with tempfile.TemporaryDirectory() as temp_dir:
            tap_dir = Path(temp_dir) / "homebrew-tap"

            print("Cloning Homebrew tap...")
            subprocess.run([
                "git", "clone",
                f"https://{self.gh_token}@github.com/{self.tap_repo}.git",
                str(tap_dir)
            ], check=True)

            # Update formulas
            print("Updating formulas...")

            # Update macOS cask (simple)
            kftray_mac_url = f"{self.base_url}kftray_{self.version}_universal.app.tar.gz"
            cask_file = tap_dir / "Casks" / "kftray.rb"
            self.update_file_simple(
                cask_file,
                version=self.version,
                url=kftray_mac_url,
                sha256=kftray_mac_hash
            )

            # Update Linux formula (complex)
            linux_file = tap_dir / "Formula" / "kftray-linux.rb"
            self.update_kftray_linux_formula(
                linux_file,
                kftray_amd64_hash,
                kftray_arm64_hash,
                kftray_newer_amd64_hash,
                kftray_newer_arm64_hash
            )

            # Update kftui formula (ordered replacement)
            kftui_file = tap_dir / "Formula" / "kftui.rb"
            self.update_kftui_formula(
                kftui_file,
                kftui_mac_hash,
                kftui_amd64_hash,
                kftui_arm64_hash
            )

            if self.dry_run:
                print("\n[DRY RUN] Showing updated file contents:")
                self.show_file_content(cask_file, "Casks/kftray.rb")
                self.show_file_content(linux_file, "Formula/kftray-linux.rb")
                self.show_file_content(kftui_file, "Formula/kftui.rb")
                print("\n[DRY RUN] Would commit and push changes, but dry run enabled.")
                print(f"[DRY RUN] Commit message would be: Update kftray to version {self.full_version} and kftui to version {self.full_version}")
            else:
                # Commit and push changes
                print("Committing changes...")
                subprocess.run([
                    "git", "-C", str(tap_dir),
                    "config", "user.name", "github-actions[bot]"
                ], check=True)

                subprocess.run([
                    "git", "-C", str(tap_dir),
                    "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com"
                ], check=True)

                subprocess.run([
                    "git", "-C", str(tap_dir),
                    "add", "Casks/kftray.rb", "Formula/kftray-linux.rb", "Formula/kftui.rb"
                ], check=True)

                commit_cmd = [
                    "git", "-C", str(tap_dir),
                    "commit", "-m", f"Update kftray to version {self.full_version} and kftui to version {self.full_version}"
                ]
                if self.no_gpg_sign:
                    commit_cmd.insert(4, "--no-gpg-sign")
                subprocess.run(commit_cmd, check=True)

                subprocess.run([
                    "git", "-C", str(tap_dir),
                    "push"
                ], check=True)

        success_message = "Homebrew formulas dry run completed!" if self.dry_run else "Homebrew formulas updated successfully!"
        print(success_message)


def main():
    if len(sys.argv) < 5 or len(sys.argv) > 7:
        print("Usage: python3 update_homebrew.py <repo> <version> <tap_repo> <gh_token> [--dry-run] [--no-gpg-sign]")
        print("Example: python3 update_homebrew.py hcavarsan/kftray v0.26.3 hcavarsan/homebrew-kftray ghp_xxx")
        print("Dry run: python3 update_homebrew.py hcavarsan/kftray v0.26.3 hcavarsan/homebrew-kftray ghp_xxx --dry-run")
        print("No GPG: python3 update_homebrew.py hcavarsan/kftray v0.26.3 hcavarsan/homebrew-kftray ghp_xxx --no-gpg-sign")
        sys.exit(1)

    repo = sys.argv[1]
    version = sys.argv[2]
    tap_repo = sys.argv[3]
    gh_token = sys.argv[4]
    extra_args = sys.argv[5:]
    dry_run = "--dry-run" in extra_args
    no_gpg_sign = "--no-gpg-sign" in extra_args

    updater = HomebrewUpdater(repo, version, tap_repo, gh_token, dry_run, no_gpg_sign)
    try:
        updater.run()
    except Exception as e:
        print(f"Error: {e}")
        sys.exit(1)


if __name__ == "__main__":
    main()

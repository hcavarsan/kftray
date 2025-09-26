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
    def __init__(self, repo, version, tap_repo, gh_token, dry_run=False):
        self.repo = repo
        self.version = version.lstrip('v')
        self.full_version = version if version.startswith('v') else f'v{version}'
        self.tap_repo = tap_repo
        self.gh_token = gh_token
        self.dry_run = dry_run
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
        """Update the Linux formula file with all SHA256 hashes for glibc variants"""
        with open(file_path, 'r') as f:
            content = f.read()

        # Update version
        content = re.sub(r'version\s+"[^"]+"', f'version "{self.version}"', content)

        # Remove existing NEWER_GLIBC constants if they exist
        content = re.sub(r'\s*NEWER_GLIBC_AMD64_SHA\s*=\s*"[^"]*"\s*\n', '', content)
        content = re.sub(r'\s*NEWER_GLIBC_ARM64_SHA\s*=\s*"[^"]*"\s*\n', '', content)

        # Add newer glibc SHA256 constants after version line
        version_line_pattern = r'(\s+version\s+"[^"]+"\s*\n)'
        sha_constants = f'''
  NEWER_GLIBC_AMD64_SHA = "{newer_amd64_hash}"
  NEWER_GLIBC_ARM64_SHA = "{newer_arm64_hash}"
'''

        if re.search(version_line_pattern, content):
            content = re.sub(version_line_pattern, lambda m: m.group(1) + sha_constants, content)

        # Update Intel (AMD64) section URLs
        amd64_url = f"{self.base_url}kftray_{self.version}_amd64.AppImage"
        content = re.sub(
            r'(on_intel\s+do\s*\n\s*url\s+")[^"]+(")',
            f'\\g<1>{amd64_url}\\g<2>',
            content,
            flags=re.MULTILINE
        )

        # Update Intel (AMD64) section SHA256
        content = re.sub(
            r'(on_intel\s+do.*?\n\s*url\s+"[^"]+"\s*\n\s*sha256\s+")[^"]+(")',
            f'\\g<1>{amd64_hash}\\g<2>',
            content,
            flags=re.DOTALL
        )

        # Update ARM section URLs
        arm64_url = f"{self.base_url}kftray_{self.version}_aarch64.AppImage"
        content = re.sub(
            r'(on_arm\s+do\s*\n\s*url\s+")[^"]+(")',
            f'\\g<1>{arm64_url}\\g<2>',
            content,
            flags=re.MULTILINE
        )

        # Update ARM section SHA256
        content = re.sub(
            r'(on_arm\s+do.*?\n\s*url\s+"[^"]+"\s*\n\s*sha256\s+")[^"]+(")',
            f'\\g<1>{arm64_hash}\\g<2>',
            content,
            flags=re.DOTALL
        )

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
            "kftray_universal.app.tar.gz",
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
        kftray_mac_hash = hashes["kftray_universal.app.tar.gz"]
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
            kftray_mac_url = f"{self.base_url}kftray_universal.app.tar.gz"
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

                subprocess.run([
                    "git", "-C", str(tap_dir),
                    "commit", "-m", f"Update kftray to version {self.full_version} and kftui to version {self.full_version}"
                ], check=True)

                subprocess.run([
                    "git", "-C", str(tap_dir),
                    "push"
                ], check=True)

        success_message = "Homebrew formulas dry run completed!" if self.dry_run else "Homebrew formulas updated successfully!"
        print(success_message)


def main():
    if len(sys.argv) < 5 or len(sys.argv) > 6:
        print("Usage: python3 update_homebrew.py <repo> <version> <tap_repo> <gh_token> [--dry-run]")
        print("Example: python3 update_homebrew.py hcavarsan/kftray v0.26.3 hcavarsan/homebrew-kftray ghp_xxx")
        print("Dry run: python3 update_homebrew.py hcavarsan/kftray v0.26.3 hcavarsan/homebrew-kftray ghp_xxx --dry-run")
        sys.exit(1)

    repo = sys.argv[1]
    version = sys.argv[2]
    tap_repo = sys.argv[3]
    gh_token = sys.argv[4]
    dry_run = len(sys.argv) == 6 and sys.argv[5] == "--dry-run"

    updater = HomebrewUpdater(repo, version, tap_repo, gh_token, dry_run)
    try:
        updater.run()
    except Exception as e:
        print(f"Error: {e}")
        sys.exit(1)


if __name__ == "__main__":
    main()
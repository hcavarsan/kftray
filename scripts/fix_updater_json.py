#!/usr/bin/env python3

import json
import os
import subprocess
import sys
import glob
from pathlib import Path


def run_command(cmd):
    result = subprocess.run(cmd, shell=True, capture_output=True, text=True)
    if result.returncode != 0:
        print(f"Command failed: {cmd}")
        print(f"Error: {result.stderr}")
        sys.exit(1)
    return result.stdout.strip()


def download_release_files(tag):
    print(f"Downloading files from release {tag}")

    os.makedirs("temp_release", exist_ok=True)
    os.chdir("temp_release")

    try:
        run_command(f"gh release download {tag} -p 'latest.json'")
    except:
        print("No existing latest.json found, will create new one")
        with open("latest.json", "w") as f:
            json.dump({"platforms": {}}, f)

    run_command(f"gh release download {tag} -p '*.AppImage.sig'")

    print("Downloaded files:")
    for file in glob.glob("*"):
        print(f"  {file}")


def read_signature(sig_file):
    if not os.path.exists(sig_file):
        print(f"Warning: {sig_file} not found")
        return None

    with open(sig_file, 'r') as f:
        return f.read().strip()


def map_appimage_to_platform(filename):
    if "newer-glibc" in filename:
        if "amd64" in filename or "x86_64" in filename:
            return "linux-x86_64-glibc239"
        elif "aarch64" in filename or "arm64" in filename:
            return "linux-aarch64-glibc239"
    else:
        if "amd64" in filename or "x86_64" in filename:
            return "linux-x86_64-glibc231"
        elif "aarch64" in filename or "arm64" in filename:
            return "linux-aarch64-glibc231"

    return None


def build_fixed_json(tag):
    with open("latest.json", "r") as f:
        data = json.load(f)

    if "platforms" not in data:
        data["platforms"] = {}

    version = tag.lstrip('v')
    data["version"] = version
    data["notes"] = "See the assets to download this version and install."
    data["pub_date"] = data.get("pub_date", "2025-01-01T00:00:00.000Z")

    sig_files = glob.glob("*.AppImage.sig")

    for sig_file in sig_files:
        appimage_name = sig_file.replace(".sig", "")
        platform_key = map_appimage_to_platform(appimage_name)

        if platform_key:
            signature = read_signature(sig_file)
            if signature:
                data["platforms"][platform_key] = {
                    "signature": signature,
                    "url": f"https://github.com/hcavarsan/kftray/releases/download/{tag}/{appimage_name}"
                }
                print(f"Added platform {platform_key} -> {appimage_name}")

    return data


def main():
    if len(sys.argv) != 2:
        print("Usage: python3 fix_updater_json.py <tag>")
        print("Example: python3 fix_updater_json.py v0.26.2")
        sys.exit(1)

    tag = sys.argv[1]
    original_dir = os.getcwd()

    try:
        download_release_files(tag)

        fixed_data = build_fixed_json(tag)

        with open("latest_fixed.json", "w") as f:
            json.dump(fixed_data, f, indent=2)

        print("\nFixed JSON structure:")
        print(json.dumps(fixed_data, indent=2))

        print(f"\nUploading fixed JSON to release {tag}")
        run_command(f"gh release upload {tag} latest_fixed.json --clobber")
        run_command(f"gh release download {tag} -p 'latest_fixed.json' -O latest.json")
        run_command(f"gh release upload {tag} latest.json --clobber")
        run_command(f"gh release delete-asset {tag} latest_fixed.json")

        print("Successfully updated latest.json in release!")

    finally:
        os.chdir(original_dir)
        if os.path.exists("temp_release"):
            import shutil
            shutil.rmtree("temp_release")


if __name__ == "__main__":
    main()
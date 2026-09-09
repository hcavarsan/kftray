#!/usr/bin/env python3
# /// script
# requires-python = ">=3.12"
# dependencies = []
# ///

import json
import os
import re
import subprocess
import sys
from pathlib import Path
from tempfile import TemporaryDirectory
from typing import Final


type JsonValue = (
    None | bool | int | float | str | list[JsonValue] | dict[str, JsonValue]
)
type JsonObject = dict[str, JsonValue]

DEFAULT_REPOSITORY: Final = "hcavarsan/kftray"
DEFAULT_DIRECTORY: Final = Path(".")


class ReleaseError(ValueError):
    pass


def run_command(args: list[str]) -> str:
    return subprocess.run(
        args,
        check=True,
        stdout=subprocess.PIPE,
        text=True,
        timeout=120,
    ).stdout.strip()


def appimage_platforms(tag: str) -> tuple[tuple[str, str], ...]:
    version = tag.removeprefix("v")
    return tuple(
        (f"linux-{arch}-glibc{glibc}", f"kftray_{version}_{prefix}{suffix}.AppImage")
        for arch, suffix in (("x86_64", "amd64"), ("aarch64", "aarch64"))
        for glibc, prefix in (("231", ""), ("239", "newer-glibc_"))
    )


def download_release_files(tag: str, directory: Path, repository: str) -> None:
    assets = set(
        run_command(
            [
                "gh",
                "release",
                "view",
                tag,
                "--repo",
                repository,
                "--json",
                "assets",
                "--jq",
                '.assets[] | select(.state == "uploaded") | .name',
            ]
        ).splitlines()
    )
    required = {"latest.json"}
    for _, asset in appimage_platforms(tag):
        required.update((asset, f"{asset}.sig"))
    missing = required - assets
    if missing:
        raise ReleaseError(
            f"Release {tag} is missing assets: {', '.join(sorted(missing))}"
        )

    _ = run_command(
        [
            "gh",
            "release",
            "download",
            tag,
            "--repo",
            repository,
            "--pattern",
            "latest.json",
            "--pattern",
            "*.AppImage.sig",
            "--dir",
            str(directory),
        ]
    )


def build_fixed_json(
    tag: str,
    directory: Path = DEFAULT_DIRECTORY,
    repository: str = DEFAULT_REPOSITORY,
) -> JsonObject:
    data: JsonValue = json.loads(
        (directory / "latest.json").read_text(encoding="utf-8")
    )
    if not isinstance(data, dict) or data.get("version") != tag.removeprefix("v"):
        raise ReleaseError(f"latest.json must contain version {tag.removeprefix('v')}")
    platforms = data.get("platforms")
    if not isinstance(platforms, dict) or not platforms:
        raise ReleaseError("latest.json must contain existing platform entries")

    for platform, asset in appimage_platforms(tag):
        signature = (directory / f"{asset}.sig").read_text(encoding="utf-8").strip()
        if not signature:
            raise ReleaseError(f"Signature is empty: {asset}.sig")
        entry: JsonObject = {
            "signature": signature,
            "url": f"https://github.com/{repository}/releases/download/{tag}/{asset}",
        }
        platforms[platform] = entry
        if platform.endswith("-glibc231"):
            generic = platform.removesuffix("-glibc231")
            platforms[generic] = entry
            platforms[f"{generic}-appimage"] = entry

    return data


def main() -> int:
    if len(sys.argv) != 2:
        print("Usage: uv run hacks/fix_updater_json.py <tag>", file=sys.stderr)
        return 1

    tag = sys.argv[1]
    if re.fullmatch(r"v[0-9]+\.[0-9]+\.[0-9]+(?:-[a-zA-Z0-9.]+)?", tag) is None:
        print(f"Invalid release tag: {tag}", file=sys.stderr)
        return 1
    repository = os.environ.get("GH_REPO", DEFAULT_REPOSITORY)
    try:
        with TemporaryDirectory(prefix="kftray-updater-") as temporary:
            directory = Path(temporary)
            download_release_files(tag, directory, repository)
            fixed_data = build_fixed_json(tag, directory, repository)
            manifest = directory / "latest.json"
            _ = manifest.write_text(
                json.dumps(fixed_data, indent=2) + "\n", encoding="utf-8"
            )
            _ = run_command(
                [
                    "gh",
                    "release",
                    "upload",
                    tag,
                    str(manifest),
                    "--repo",
                    repository,
                    "--clobber",
                ]
            )
    except subprocess.CalledProcessError as error:
        print(f"Release command failed: {error}", file=sys.stderr)
        return 1
    except (OSError, ValueError, subprocess.TimeoutExpired) as error:
        print(f"Could not normalize updater metadata: {error}", file=sys.stderr)
        return 1

    print(f"Updated latest.json for {tag} from finalized AppImage signatures")
    return 0


if __name__ == "__main__":
    sys.exit(main())

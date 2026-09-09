import json
import subprocess
from pathlib import Path

import pytest

from hacks import fix_updater_json as updater

TAG = "v0.27.31"
ASSETS = {
    "linux-x86_64-glibc231": "kftray_0.27.31_amd64.AppImage",
    "linux-aarch64-glibc231": "kftray_0.27.31_aarch64.AppImage",
    "linux-x86_64-glibc239": "kftray_0.27.31_newer-glibc_amd64.AppImage",
    "linux-aarch64-glibc239": "kftray_0.27.31_newer-glibc_aarch64.AppImage",
}


@pytest.fixture
def release_files(tmp_path: Path) -> Path:
    manifest = {
        "version": "0.27.31",
        "notes": "Release notes with useful details",
        "pub_date": "2026-09-09T01:31:18.786Z",
        "extra_metadata": {"preserve": True},
        "platforms": {
            "linux-x86_64": {
                "url": "https://api.github.com/assets/1",
                "signature": "stale",
            },
            "linux-x86_64-appimage": {
                "url": "https://api.github.com/assets/1",
                "signature": "stale",
            },
            "linux-aarch64": {
                "url": "https://api.github.com/assets/2",
                "signature": "stale",
            },
            "linux-aarch64-appimage": {
                "url": "https://api.github.com/assets/2",
                "signature": "stale",
            },
            "darwin-aarch64": {
                "url": "https://example.com/mac.tar.gz",
                "signature": "mac-signature",
            },
        },
    }
    (tmp_path / "latest.json").write_text(json.dumps(manifest), encoding="utf-8")
    for asset in ASSETS.values():
        (tmp_path / f"{asset}.sig").write_text(
            f"final-signature-{asset}\n", encoding="utf-8"
        )
    return tmp_path


def test_repacked_signatures_replace_generic_linux_aliases(
    release_files: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.chdir(release_files)
    fixed = updater.build_fixed_json(TAG)
    platforms = fixed["platforms"]
    assert isinstance(platforms, dict)

    for platform, asset in ASSETS.items():
        aliases = [platform]
        if platform.endswith("glibc231"):
            generic = platform.removesuffix("-glibc231")
            aliases.extend([generic, f"{generic}-appimage"])
        for alias in aliases:
            assert platforms[alias] == {
                "signature": f"final-signature-{asset}",
                "url": f"https://github.com/hcavarsan/kftray/releases/download/{TAG}/{asset}",
            }


def test_preserves_release_metadata_and_non_linux_platforms(
    release_files: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    original = json.loads((release_files / "latest.json").read_text(encoding="utf-8"))
    monkeypatch.chdir(release_files)

    fixed = updater.build_fixed_json(TAG)
    platforms = fixed["platforms"]
    assert isinstance(platforms, dict)

    for field in ("version", "notes", "pub_date", "extra_metadata"):
        assert fixed[field] == original[field]
    assert platforms["darwin-aarch64"] == original["platforms"]["darwin-aarch64"]


@pytest.mark.parametrize("broken_signature", ["missing", "empty"])
def test_incomplete_signatures_stop_normalization(
    release_files: Path,
    monkeypatch: pytest.MonkeyPatch,
    broken_signature: str,
) -> None:
    signature = release_files / f"{ASSETS['linux-aarch64-glibc239']}.sig"
    if broken_signature == "missing":
        signature.unlink()
    else:
        signature.write_text("\n", encoding="utf-8")
    monkeypatch.chdir(release_files)

    with pytest.raises((ValueError, FileNotFoundError)):
        updater.build_fixed_json(TAG)


def test_manifest_for_another_version_is_rejected(
    release_files: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.chdir(release_files)

    with pytest.raises(ValueError):
        updater.build_fixed_json("v0.27.32")


def test_normalizer_is_idempotent(
    release_files: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.chdir(release_files)
    first = updater.build_fixed_json(TAG)
    (release_files / "latest.json").write_text(json.dumps(first), encoding="utf-8")

    assert updater.build_fixed_json(TAG) == first


@pytest.mark.parametrize(
    "missing_asset", [None, "latest.json", "kftray_0.27.31_amd64.AppImage"]
)
def test_release_inventory_gates_upload_and_temporary_files_are_isolated(
    release_files: Path,
    monkeypatch: pytest.MonkeyPatch,
    missing_asset: str | None,
) -> None:
    uploads: list[Path] = []
    download_directories: list[Path] = []
    sentinel = release_files / "temp_release" / "user-file.txt"
    sentinel.parent.mkdir()
    sentinel.write_text("keep me", encoding="utf-8")
    monkeypatch.chdir(release_files)
    monkeypatch.setenv("GH_REPO", "hcavarsan/kftray")
    monkeypatch.setattr("sys.argv", ["fix_updater_json.py", TAG])

    def fake_command(args: list[str], directory: Path | None = None) -> str:
        assert isinstance(args, list)
        assert args[:2] == ["gh", "release"]
        operation = args[2]
        if operation == "view":
            names = {
                "latest.json",
                *ASSETS.values(),
                *(f"{name}.sig" for name in ASSETS.values()),
            }
            if missing_asset:
                names.remove(missing_asset)
            return "\n".join(sorted(names))
        if operation == "download":
            destination = Path(args[args.index("--dir") + 1])
            download_directories.append(destination)
            for source in release_files.iterdir():
                if source.is_file():
                    (destination / source.name).write_bytes(source.read_bytes())
            return ""
        assert operation == "upload"
        path = Path(args[4])
        uploads.append(path)
        assert path.name == "latest.json"
        manifest = json.loads(path.read_text(encoding="utf-8"))
        assert (
            manifest["platforms"]["linux-x86_64"]["signature"]
            == "final-signature-kftray_0.27.31_amd64.AppImage"
        )
        return ""

    monkeypatch.setattr(updater, "run_command", fake_command)

    status = updater.main()

    assert status == (1 if missing_asset else 0)
    assert len(uploads) == (0 if missing_asset else 1)
    assert all(not directory.exists() for directory in download_directories)
    assert sentinel.read_text(encoding="utf-8") == "keep me"


def test_missing_release_is_an_error_without_uploads(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    commands: list[list[str]] = []
    monkeypatch.chdir(tmp_path)
    monkeypatch.setattr("sys.argv", ["fix_updater_json.py", TAG])

    def unavailable_release(args: list[str], directory: Path | None = None) -> str:
        commands.append(args)
        raise subprocess.CalledProcessError(1, args, stderr="release not found")

    monkeypatch.setattr(updater, "run_command", unavailable_release)

    assert updater.main() == 1
    assert all(command[2] != "upload" for command in commands)

#!/usr/bin/env python3
"""Package GitManager's native Linux DEB payload as a Flatpak bundle."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys

APP_ID = "com.gitmanager.desktop"
ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "installer" / "flatpak" / f"{APP_ID}.yml"
CACHE = ROOT / "build" / ".rcache" / "flatpak"


def run(command: list[str], *, cwd: Path | None = None) -> None:
    print("+", " ".join(str(part) for part in command), flush=True)
    subprocess.run(command, cwd=cwd, check=True)


def ensure_user_flatpak_runtime() -> None:
    """Prepare the GNOME runtime and SDK in the invoking user's Flatpak installation."""
    run([
        "flatpak", "remote-add", "--user", "--if-not-exists",
        "flathub", "https://dl.flathub.org/repo/flathub.flatpakrepo",
    ])
    run([
        "flatpak", "install", "--user", "--noninteractive", "--assumeyes",
        "flathub", "org.gnome.Platform//51", "org.gnome.Sdk//51",
    ])


def find_main_binary(extracted_usr: Path) -> Path:
    candidates = (
        extracted_usr / "bin" / "git-manager",
        extracted_usr / "bin" / "gitmanager",
        extracted_usr / "bin" / "GitManager",
    )
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    raise FileNotFoundError(
        "The GitManager DEB did not contain the expected main executable under usr/bin."
    )


def copy_tree_contents(source: Path, destination: Path) -> None:
    if source.is_dir():
        shutil.copytree(source, destination, dirs_exist_ok=True, symlinks=True)


def stage_deb(deb: Path, staging: Path) -> None:
    work = CACHE / "deb-extract"
    if work.exists():
        shutil.rmtree(work)
    work.mkdir(parents=True, exist_ok=True)
    extracted = work / "root"
    extracted.mkdir()
    run(["dpkg-deb", "-x", str(deb), str(extracted)])

    usr = extracted / "usr"
    main_binary = find_main_binary(usr)
    if staging.exists():
        shutil.rmtree(staging)
    (staging / "bin").mkdir(parents=True)
    shutil.copy2(main_binary, staging / "bin" / "git-manager")

    # Preserve Tauri's exact resource layout and helper binaries from the DEB.
    copy_tree_contents(usr / "bin", staging / "bin")
    shutil.copy2(main_binary, staging / "bin" / "git-manager")
    copy_tree_contents(usr / "lib", staging / "lib")
    copy_tree_contents(usr / "share", staging / "share")

    required = ("gitm", "crash_helper", "git-manager-service", "gitmanager-setup")
    search_roots = (staging / "bin", staging / "lib")
    for name in required:
        if (staging / "bin" / name).is_file():
            continue
        candidates = [
            path for root in search_roots for path in root.rglob("*")
            if path.is_file() and path.name.startswith(name)
        ]
        if candidates:
            shutil.copy2(candidates[0], staging / "bin" / name)
    missing = [
        name for name in required
        if not any(path.is_file() and path.name.startswith(name) for root in search_roots for path in root.rglob("*"))
    ]
    if missing:
        raise FileNotFoundError(
            "The DEB is missing GitManager sidecars required by the desktop app: "
            + ", ".join(missing)
        )

    applications = staging / "share" / "applications"
    applications.mkdir(parents=True, exist_ok=True)
    for old_desktop in applications.glob("*.desktop"):
        if old_desktop.name != f"{APP_ID}.desktop":
            old_desktop.unlink()
    desktop = applications / f"{APP_ID}.desktop"
    desktop.write_text(
        "[Desktop Entry]\n"
        "Type=Application\n"
        "Name=GitManager\n"
        "Comment=Native Git workspace manager by Kastrick\n"
        "Exec=git-manager\n"
        f"Icon={APP_ID}\n"
        "Terminal=false\n"
        "Categories=Development;RevisionControl;Utility;\n"
        "StartupNotify=true\n",
        encoding="utf-8",
    )

    icons = ROOT / "src-tauri" / "icons"
    for size in ("32x32", "128x128", "128x128@2x"):
        source = icons / f"{size}.png"
        if source.is_file():
            target_size = "256x256" if size == "128x128@2x" else size
            icon_dir = staging / "share" / "icons" / "hicolor" / target_size / "apps"
            icon_dir.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, icon_dir / f"{APP_ID}.png")

    metainfo = staging / "share" / "metainfo" / f"{APP_ID}.metainfo.xml"
    metainfo.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(ROOT / "installer" / "flatpak" / f"{APP_ID}.metainfo.xml", metainfo)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--deb", required=True, type=Path, help="Tauri-generated Linux DEB package.")
    parser.add_argument(
        "--output",
        type=Path,
        default=ROOT / "build" / "dist" / "store" / "flatpak",
        help="Directory for the final .flatpak bundle.",
    )
    args = parser.parse_args()

    deb = args.deb.resolve()
    if not deb.is_file() or deb.suffix.lower() != ".deb":
        parser.error(f"--deb must point to an existing .deb file: {deb}")
    missing_tools = [tool for tool in ("dpkg-deb", "flatpak-builder", "flatpak") if shutil.which(tool) is None]
    if missing_tools:
        print(
            "Missing Flatpak build tools: " + ", ".join(missing_tools) + ". "
            "Install them with your Linux distribution's package manager, then retry.",
            file=sys.stderr,
        )
        return 2
    if not MANIFEST.is_file():
        print(f"Flatpak manifest is missing: {MANIFEST}", file=sys.stderr)
        return 2

    staging = CACHE / "staging"
    try:
        print("Preparing the GNOME 51 runtime and SDK in the current user's Flatpak installation...")
        ensure_user_flatpak_runtime()
        stage_deb(deb, staging)
        repo = CACHE / "repo"
        build_dir = CACHE / "build"
        generated_manifest = CACHE / f"{APP_ID}.yml"
        manifest_text = MANIFEST.read_text(encoding="utf-8")
        manifest_text = manifest_text.replace(
            "path: ../../build/.rcache/flatpak/staging",
            "path: staging",
        )
        generated_manifest.write_text(manifest_text, encoding="utf-8")
        if repo.exists():
            shutil.rmtree(repo)
        run([
            "flatpak-builder", "--user", "--force-clean", f"--repo={repo}",
            str(build_dir), str(generated_manifest),
        ], cwd=CACHE)

        # The Linux Developer Cleaner requires Python 3.10+ inside the runtime;
        # verify it now rather than shipping a bundle with a silently broken tool.
        run([
            "flatpak-builder", "--user", "--run", str(build_dir), str(generated_manifest),
            "python3", "--version",
        ], cwd=CACHE)
        run([
            "flatpak-builder", "--user", "--run", str(build_dir), str(generated_manifest),
            "gitm", "--version",
        ], cwd=CACHE)

        arch = subprocess.check_output(
            ["flatpak", "--default-arch"], text=True
        ).strip()
        args.output.mkdir(parents=True, exist_ok=True)
        marker = args.output / ".gitmanager-generated"
        marker.write_text("Generated by tools/package_flatpak.py.\n", encoding="utf-8")
        bundle = args.output / f"GitManager-{arch}.flatpak"
        run([
            "flatpak", "build-bundle", f"--arch={arch}",
            str(repo), str(bundle), APP_ID, "stable",
            "--runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo",
        ], cwd=ROOT)
        if not bundle.is_file() or bundle.stat().st_size == 0:
            raise RuntimeError("Flatpak builder completed without producing a non-empty bundle.")
        print(f"Created Flatpak bundle: {bundle} ({bundle.stat().st_size:,} bytes)")
        return 0
    except (OSError, subprocess.CalledProcessError, RuntimeError) as error:
        print(f"Flatpak packaging failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())

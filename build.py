#!/usr/bin/env python3
"""
GitManager Tauri v2 Dependency Checker & Build Script
Supports Windows 10 & 11, Linux, and macOS. The frontend is compiled with Node.js before Tauri packages the static export.

Features:
1. Checks if Rust (`rustc` and `cargo`) is installed across PATH and `~/.cargo/bin`.
2. Compares the installed Rust version against the workspace baseline (`1.88.0` required by the current filesystem watcher):
   - If a different version is already installed (whether newer, custom/nightly, or older),
     it automatically adapts and uses the ALREADY INSTALLED Rust toolchain on the machine
     instead of failing or forcing a toolchain re-download.
   - If `rustup` has multiple toolchains installed and the active default is older than
     recommended while another installed toolchain is newer, it can automatically select
     the best already-installed toolchain.
3. Validates and auto-sanitizes `src-tauri/tauri.conf.json` for Tauri v2 schema compliance
   (e.g. ensuring `withGlobalTauri` is placed strictly under `app`, never under `build`,
   and verifying `frontendDist` points to `../ui`).
4. Checks for the Rust `tauri-cli` (`cargo tauri`) crate binary:
   - Uses the already-installed `cargo-tauri` version if present.
   - If `cargo-tauri` is not installed, offers `--install-tauri-cli` or falls back cleanly
     to building/running the pure-Rust `src-tauri` crate directly via `cargo build` / `cargo run`.
5. Verifies OS-specific native libraries (WebView2/MSVC on Windows 10/11, WebKit2GTK on Linux,
   Xcode Command Line Tools on macOS).
"""

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import tarfile
import urllib.error
import urllib.parse
import urllib.request
import zipfile
import stat
from pathlib import Path
from typing import Dict, Optional, Tuple, List

RECOMMENDED_RUST_VERSION = (1, 88, 0)
RECOMMENDED_TAURI_CRATE_VERSION = (2, 0, 0)


def log_info(msg: str) -> None:
    print(f"[INFO] {msg}")


def log_ok(msg: str) -> None:
    print(f"[ OK ] {msg}")


def log_warn(msg: str) -> None:
    print(f"[WARN] {msg}")


def log_err(msg: str) -> None:
    print(f"[ERR ] {msg}", file=sys.stderr)



PYTHON_EMBED_VERSION = "3.14.10"
PYTHON_EMBED_URL = (
    "https://www.python.org/ftp/python/3.14.10/"
    "python-3.14.10-embed-amd64.zip"
)
PYTHON_EMBED_SHA256 = "90905e64a840557d8c1bc4f5b74db3d7ee85e3ffb217611cac1c7813acea4159"


def prepare_windows_python_runtime(project_root: Path) -> int:
    """Fetch and verify the isolated CPython runtime used by the bundled cleaner."""
    if platform.system() != "Windows":
        log_info("Private Windows Python runtime is only bundled on Windows; skipping.")
        return 0

    runtime_dir = project_root / "tools" / "runtime" / "python"
    python_exe = runtime_dir / "python.exe"
    if python_exe.is_file():
        code, out, err = run_capture([
            str(python_exe), "-I", "-c",
            "import sys; print('.'.join(map(str, sys.version_info[:3]))); raise SystemExit(0 if sys.version_info >= (3, 10) else 1)"
        ])
        if code == 0 and out.startswith(PYTHON_EMBED_VERSION):
            log_ok(f"Private CPython {out} runtime is already prepared.")
            return 0

    log_info(f"Preparing isolated CPython {PYTHON_EMBED_VERSION} runtime for the Windows installer...")
    with tempfile.TemporaryDirectory(prefix="gitmanager-python-") as temporary:
        archive_path = Path(temporary) / "python-embed.zip"
        request = urllib.request.Request(
            PYTHON_EMBED_URL,
            headers={"User-Agent": "GitManager-Build/1.0"},
        )
        try:
            with urllib.request.urlopen(request, timeout=90) as response, archive_path.open("wb") as output:
                shutil.copyfileobj(response, output)
        except Exception as exc:
            log_err(f"Could not download the official Python embeddable package: {exc}")
            return 1

        digest = hashlib.sha256(archive_path.read_bytes()).hexdigest()
        if digest.lower() != PYTHON_EMBED_SHA256:
            log_err(
                "Python embeddable package checksum did not match the pinned python.org SHA-256. "
                f"Expected {PYTHON_EMBED_SHA256}, received {digest}."
            )
            return 1

        try:
            with zipfile.ZipFile(archive_path) as archive:
                root = runtime_dir.resolve()
                for member in archive.infolist():
                    target = (runtime_dir / member.filename).resolve()
                    try:
                        target.relative_to(root)
                    except ValueError:
                        log_err(f"Python runtime archive contains an unsafe path: {member.filename}")
                        return 1
                runtime_dir.mkdir(parents=True, exist_ok=True)
                # Keep the tracked directory marker so later repository status remains clean.
                for existing in runtime_dir.iterdir():
                    if existing.name == ".keep":
                        continue
                    if existing.is_dir() and not existing.is_symlink():
                        shutil.rmtree(existing)
                    else:
                        existing.unlink()
                archive.extractall(runtime_dir)
        except (OSError, zipfile.BadZipFile) as exc:
            log_err(f"Could not extract the verified Python runtime: {exc}")
            return 1

    required = [python_exe, runtime_dir / "python314.zip", runtime_dir / "python314._pth"]
    missing = [path.name for path in required if not path.is_file()]
    if missing:
        log_err("Python runtime is incomplete; missing: " + ", ".join(missing))
        return 1

    code, out, err = run_capture([
        str(python_exe), "-I", "-c",
        "import sys; print('.'.join(map(str, sys.version_info[:3]))); raise SystemExit(0 if sys.version_info >= (3, 10) else 1)"
    ])
    if code != 0 or not out.startswith(PYTHON_EMBED_VERSION):
        log_err(f"Bundled Python failed its isolated startup check: {err or out or code}")
        return 1

    runtime_bytes = sum(path.stat().st_size for path in runtime_dir.iterdir() if path.is_file())
    log_ok(f"Verified python.org CPython {out} embeddable runtime ({runtime_bytes:,} bytes extracted).")
    return 0


def stage_windows_mpdev_package(project_root: Path, toolchain_arg: str) -> int:
    """Stage the raw Windows app and every runtime/sidecar for MPDEV packaging."""
    if platform.system() != "Windows":
        return 0

    target_dir = Path(os.environ.get("CARGO_TARGET_DIR", project_root / "target"))
    if not target_dir.is_absolute():
        target_dir = project_root / target_dir
    profile_dir = target_dir / "release"
    main_exe = profile_dir / "git-manager.exe"
    if not main_exe.is_file():
        log_err(f"Windows desktop executable is missing: {main_exe}")
        return 1

    try:
        triple = resolve_rust_target_triple(toolchain_arg)
    except RuntimeError as error:
        log_err(str(error))
        return 1

    sidecar_dir = project_root / "src-tauri" / "binaries"
    sidecars = {
        "gitm.exe": sidecar_dir / f"gitm-{triple}.exe",
        "gitmanager-setup.exe": sidecar_dir / f"gitmanager-setup-{triple}.exe",
        "crash_helper.exe": sidecar_dir / f"crash_helper-{triple}.exe",
        "git-manager-service.exe": sidecar_dir / f"git-manager-service-{triple}.exe",
    }
    missing = [name for name, source in sidecars.items() if not source.is_file()]
    if missing:
        log_err("Windows sidecar staging is incomplete; missing: " + ", ".join(missing))
        return 1

    staging = target_dir / "windows-package" / "source"
    try:
        if staging.exists():
            shutil.rmtree(staging)
        staging.mkdir(parents=True, exist_ok=True)
        shutil.copy2(main_exe, staging / "git-manager.exe")
        resources = staging / "resources"
        resources.mkdir(parents=True, exist_ok=True)
        # Tauri's shell plugin resolves externalBin sidecars from the app
        # resource directory. crash_helper also searches this exact location.
        for name, source in sidecars.items():
            shutil.copy2(source, resources / name)
        (resources / "tools").mkdir(parents=True, exist_ok=True)
        cleaner = project_root / "tools" / "gitmanager_cleaner.py"
        if not cleaner.is_file():
            raise FileNotFoundError(f"GitManager cleaner resource is missing: {cleaner}")
        shutil.copy2(cleaner, resources / "tools" / cleaner.name)

        python_source = project_root / "tools" / "runtime" / "python"
        python_exe = python_source / "python.exe"
        required_python = (python_exe, python_source / "python314.zip", python_source / "python314._pth")
        missing_python = [path.name for path in required_python if not path.is_file()]
        if missing_python:
            raise FileNotFoundError("Bundled CPython runtime is incomplete: " + ", ".join(missing_python))
        shutil.copytree(python_source, resources / "python", dirs_exist_ok=True)

        # Fail closed before MPDEV if any packaged executable or the private
        # interpreter is missing or empty. No machine-wide Python is used.
        required = [
            "git-manager.exe",
            *(f"resources/{name}" for name in sidecars),
            "resources/python/python.exe",
            "resources/tools/gitmanager_cleaner.py",
        ]
        empty = [
            relative for relative in required
            if not (staging / relative).is_file() or (staging / relative).stat().st_size == 0
        ]
        if empty:
            raise FileNotFoundError("Windows package staging is incomplete: " + ", ".join(empty))
    except OSError as error:
        log_err(f"Could not stage the Windows application for Master Packager Dev: {error}")
        return 1

    log_ok(f"Staged complete Windows app payload for MPDEV: {staging}")
    log_info("Included git-manager.exe, crash_helper.exe, background service, gitm, setup helper, cleaner and private CPython.")
    return 0


def ensure_cargo_in_path() -> None:
    """Adds ~/.cargo/bin to PATH if present so rustc/cargo are found even in non-login shells."""
    cargo_bin = Path.home() / ".cargo" / "bin"
    if cargo_bin.is_dir():
        current_path = os.environ.get("PATH", "")
        if str(cargo_bin) not in current_path.split(os.pathsep):
            os.environ["PATH"] = f"{cargo_bin}{os.pathsep}{current_path}"


def parse_semver(text: str) -> Optional[Tuple[int, int, int]]:
    match = re.search(r"(\d+)\.(\d+)\.(\d+)", text)
    if not match:
        return None
    return (int(match.group(1)), int(match.group(2)), int(match.group(3)))


def format_semver(ver: Tuple[int, int, int]) -> str:
    return f"{ver[0]}.{ver[1]}.{ver[2]}"


def run_capture(cmd: List[str], cwd: Optional[Path] = None) -> Tuple[int, str, str]:
    try:
        proc = subprocess.run(
            cmd,
            cwd=str(cwd) if cwd else None,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            check=False,
        )
        return proc.returncode, proc.stdout.strip(), proc.stderr.strip()
    except FileNotFoundError:
        return 127, "", f"Command not found: {cmd[0]}"


def report_dependency_count(project_root: Path, toolchain_arg: str) -> Optional[int]:
    """Count the actual build graphs for the app and its packaged sidecars.

    Cargo's progress bar reports compilation units (including build scripts and
    proc-macro targets), not unique crates. This report uses package-scoped
    normal/build dependency graphs, never the entire workspace.
    """
    try:
        target_triple = os.environ.get("CARGO_BUILD_TARGET", "").strip()
        if not target_triple:
            target_triple = resolve_rust_target_triple(toolchain_arg)
    except RuntimeError as error:
        log_warn(f"Could not determine target for the Cargo dependency audit: {error}")
        return None

    command_prefix = ["cargo"]
    if toolchain_arg:
        command_prefix.append(toolchain_arg)

    package_graphs: Dict[str, set] = {}
    for package in ("git-manager", "gitm-cli", "gitmanager-setup", "crash-helper", "git-manager-service"):
        command = command_prefix + [
            "tree",
            "--package", package,
            "--edges", "normal,build",
            "--target", target_triple,
            "--prefix", "none",
            "--format", "{p}",
        ]
        code, output, error = run_capture(command, cwd=project_root)
        if code != 0:
            log_warn(f"Could not measure the {package} dependency graph: {error or code}")
            continue

        packages = {
            line.strip()
            for line in output.splitlines()
            if re.match(r"^.+ v[0-9]", line.strip())
        }
        package_graphs[package] = packages
        log_info(f"{package}: {len(packages)} unique normal/build packages for {target_triple}.")

    app_count = len(package_graphs.get("git-manager", set()))
    sidecar_packages = (
        package_graphs.get("gitm-cli", set())
        | package_graphs.get("gitmanager-setup", set())
        | package_graphs.get("crash-helper", set())
        | package_graphs.get("git-manager-service", set())
    ) - package_graphs.get("git-manager", set())
    complete_build_packages = set().union(*package_graphs.values()) if package_graphs else set()

    if app_count:
        if app_count < 500:
            log_ok(f"GitManager app graph: {app_count} unique packages (target: under 500).")
        else:
            log_warn(
                f"GitManager app graph: {app_count} unique packages; "
                "the under-500 target is not met yet."
            )
    if sidecar_packages:
        log_info(f"Packaged sidecar graphs: {len(sidecar_packages)} unique packages.")
    if complete_build_packages:
        log_info(
            f"All requested build targets together: {len(complete_build_packages)} "
            "unique packages (not Cargo compilation units)."
        )
    return app_count or None



RBE_CLOUD_NODE_RELEASE_TAG = "rbe-bin-v0.1.0"
RBE_CLOUD_NODE_TARGETS = {
    "windows-x64": ("windows-x64", "zip", "cloud_node.exe", "Windows x64 (MSVC)"),
    "linux-x64": ("linux-x64", "tar.gz", "cloud_node", "Linux x64 (glibc 2.35+)"),
    "linux-arm64": ("linux-arm64", "tar.gz", "cloud_node", "Linux ARM64 (glibc 2.39+)"),
    "macos-intel": ("macos-intel", "tar.gz", "cloud_node", "macOS Intel"),
    "macos-arm64": ("macos-arm64", "tar.gz", "cloud_node", "macOS Apple Silicon"),
}


def read_bounded_url(url: str, limit: int, *, headers: Optional[Dict[str, str]] = None) -> bytes:
    request = urllib.request.Request(
        url,
        headers={
            "User-Agent": "GitManager-Cloud-Node-Builder",
            "Accept": "application/vnd.github+json",
            **(headers or {}),
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            parsed = urllib.parse.urlsplit(response.geturl())
            allowed_hosts = {
                "api.github.com",
                "github.com",
                "release-assets.githubusercontent.com",
                "objects.githubusercontent.com",
                "github-cloud.s3.amazonaws.com",
            }
            if parsed.scheme != "https" or parsed.hostname not in allowed_hosts:
                raise ValueError(f"Refusing download redirect to untrusted host: {response.geturl()}")
            declared_size = response.headers.get("Content-Length")
            if declared_size and int(declared_size) > limit:
                raise ValueError(f"Remote file exceeds the {limit:,}-byte download limit.")
            data = response.read(limit + 1)
    except (urllib.error.URLError, TimeoutError) as error:
        raise RuntimeError(f"Could not download {url}: {error}") from error
    if len(data) > limit:
        raise ValueError(f"Remote file exceeds the {limit:,}-byte download limit.")
    return data


def parse_sha256_digest(value: str, label: str) -> str:
    match = re.fullmatch(r"sha256:([0-9a-fA-F]{64})", str(value or ""))
    if not match:
        raise ValueError(f"{label} has no valid GitHub SHA-256 digest.")
    return match.group(1).lower()


def fetch_rbe_cloud_node_release(tag: str) -> Dict[str, object]:
    if not re.fullmatch(r"rbe-bin-v[0-9]+\.[0-9]+\.[0-9]+", tag):
        raise ValueError("RBE_CLOUD_NODE_RELEASE_TAG must be a version tag such as rbe-bin-v0.1.0.")
    api_url = f"https://api.github.com/repos/Kastrick8/RBE/releases/tags/{tag}"
    try:
        release = json.loads(read_bounded_url(api_url, 2 * 1024 * 1024).decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError(f"RBE GitHub Release metadata was invalid: {error}") from error
    if release.get("tag_name") != tag or release.get("draft") or release.get("prerelease"):
        raise ValueError(f"RBE tag {tag} is not a published stable release.")
    if not isinstance(release.get("assets"), list):
        raise ValueError(f"RBE release {tag} has no asset list.")
    return release


def download_verified_rbe_asset(
    asset: Dict[str, object],
    cache_dir: Path,
    *,
    max_bytes: int = 128 * 1024 * 1024,
) -> Tuple[Path, str]:
    name = str(asset.get("name") or "")
    if not re.fullmatch(r"[A-Za-z0-9._-]+", name):
        raise ValueError(f"RBE release contains an unsafe asset name: {name!r}")
    expected_size = asset.get("size")
    if not isinstance(expected_size, int) or expected_size <= 0 or expected_size > max_bytes:
        raise ValueError(f"RBE asset {name} has an invalid or oversized declared size.")
    expected_hash = parse_sha256_digest(str(asset.get("digest") or ""), f"RBE asset {name}")
    download_url = str(asset.get("browser_download_url") or "")
    if not download_url.startswith("https://github.com/Kastrick8/RBE/releases/download/"):
        raise ValueError(f"RBE asset {name} points to an unexpected download URL.")

    cache_dir.mkdir(parents=True, exist_ok=True)
    destination = cache_dir / name
    if destination.is_file() and not destination.is_symlink():
        if destination.stat().st_size == expected_size and hashlib.sha256(destination.read_bytes()).hexdigest() == expected_hash:
            log_ok(f"Using cached and verified RBE asset: {name}")
            return destination, expected_hash
        destination.unlink()

    log_info(f"Downloading official RBE asset: {name} ({expected_size:,} bytes)")
    payload = read_bounded_url(download_url, max_bytes)
    if len(payload) != expected_size:
        raise ValueError(f"RBE asset {name} size mismatch: expected {expected_size}, received {len(payload)}.")
    actual_hash = hashlib.sha256(payload).hexdigest()
    if actual_hash != expected_hash:
        raise ValueError(f"RBE asset {name} failed GitHub Release SHA-256 verification.")
    temporary = destination.with_name(destination.name + ".part")
    try:
        with temporary.open("wb") as output:
            output.write(payload)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, destination)
    finally:
        if temporary.exists():
            temporary.unlink()
    log_ok(f"Downloaded and SHA-256 verified: {name}")
    return destination, actual_hash


def extract_rbe_cloud_node_archive(
    archive_path: Path,
    archive_kind: str,
    binary_name: str,
    destination: Path,
) -> Tuple[int, str]:
    max_binary_bytes = 256 * 1024 * 1024
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_name(destination.name + ".part")
    count = 0
    digest = hashlib.sha256()
    try:
        if archive_kind == "zip":
            with zipfile.ZipFile(archive_path, "r") as archive:
                matches = [item for item in archive.infolist() if item.filename == binary_name]
                if len(matches) != 1:
                    raise ValueError(f"{archive_path.name} must contain exactly one root-level {binary_name}.")
                item = matches[0]
                mode = (item.external_attr >> 16) & 0xFFFF
                if item.is_dir() or stat.S_ISLNK(mode) or (mode and not stat.S_ISREG(mode)):
                    raise ValueError(f"RBE asset contains a non-regular {binary_name}.")
                if item.file_size <= 0 or item.file_size > max_binary_bytes:
                    raise ValueError(f"RBE asset {binary_name} has an invalid size.")
                with archive.open(item, "r") as source, temporary.open("wb") as output:
                    while True:
                        chunk = source.read(1024 * 1024)
                        if not chunk:
                            break
                        count += len(chunk)
                        if count > max_binary_bytes:
                            raise ValueError(f"RBE asset {binary_name} exceeded the extraction size limit.")
                        digest.update(chunk)
                        output.write(chunk)
        elif archive_kind == "tar.gz":
            with tarfile.open(archive_path, "r:gz") as archive:
                matches = [item for item in archive.getmembers() if item.name == binary_name]
                if len(matches) != 1:
                    raise ValueError(f"{archive_path.name} must contain exactly one root-level {binary_name}.")
                item = matches[0]
                if not item.isfile() or item.issym() or item.islnk() or item.size <= 0 or item.size > max_binary_bytes:
                    raise ValueError(f"RBE asset contains an invalid {binary_name} entry.")
                source = archive.extractfile(item)
                if source is None:
                    raise ValueError(f"Could not read {binary_name} from {archive_path.name}.")
                with source, temporary.open("wb") as output:
                    while True:
                        chunk = source.read(1024 * 1024)
                        if not chunk:
                            break
                        count += len(chunk)
                        if count > max_binary_bytes:
                            raise ValueError(f"RBE asset {binary_name} exceeded the extraction size limit.")
                        digest.update(chunk)
                        output.write(chunk)
        else:
            raise ValueError(f"Unsupported RBE archive format: {archive_kind}")

        if count <= 0:
            raise ValueError(f"RBE asset contained an empty {binary_name}.")
        os.replace(temporary, destination)
        return count, digest.hexdigest()
    finally:
        if temporary.exists():
            temporary.unlink()


def prepare_cloud_node_deployment(project_root: Path) -> int:
    """Build a small bootstrap ZIP; setup.py downloads only the host's RBE runtime."""
    try:
        template_dir = project_root / "tools" / "cloud-node-deployment"
        required = (
            "README.md",
            "setup.py",
            "manifest.json",
            "settings.json",
            "setting.node.cn.json",
            "LICENSE-GITMANAGER.txt",
            "LICENSE-RBE.txt",
            "NOTICE-RBE.txt",
        )
        for name in required:
            source = template_dir / name
            if not source.is_file() or source.is_symlink():
                raise ValueError(f"RBE node bootstrap template is missing or unsafe: {source}")

        manifest = json.loads((template_dir / "manifest.json").read_text(encoding="utf-8"))
        if (
            manifest.get("formatVersion") != 2
            or manifest.get("kind") != "gitmanager-rbe-runtime-bootstrap"
            or manifest.get("rbeRepository") != "Kastrick8/RBE"
        ):
            raise ValueError("RBE node bootstrap manifest has an unsupported format.")

        route_root = template_dir / "api"
        if not route_root.is_dir() or route_root.is_symlink():
            raise ValueError("RBE node bootstrap package is missing its api/ route folder.")
        route_files = sorted(
            path for path in route_root.rglob("*.route")
            if path.is_file() and not path.is_symlink()
        )
        if (
            not route_files
            or not (route_root / "health.route").is_file()
            or not (route_root / "status.route").is_file()
        ):
            raise ValueError("RBE node bootstrap package must include api/health.route and api/status.route.")
        for route in route_files:
            if not route.resolve().is_relative_to(route_root.resolve()):
                raise ValueError(f"RBE API route escapes its template directory: {route}")

        target_dir = project_root / "target"
        output_dir = target_dir / "deployment"
        output_dir.mkdir(parents=True, exist_ok=True)
        archive_name = "GitManager-Cloud-Node-Setup.zip"
        final_archive = output_dir / archive_name
        temporary_archive = output_dir / (archive_name + ".part")
        if temporary_archive.exists():
            temporary_archive.unlink()

        package_files = [template_dir / name for name in required] + route_files
        with zipfile.ZipFile(
            temporary_archive, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=6
        ) as bundle:
            for file_path in sorted(package_files, key=lambda path: path.relative_to(template_dir).as_posix()):
                relative = file_path.relative_to(template_dir).as_posix()
                bundle.write(file_path, relative)

        with zipfile.ZipFile(temporary_archive, "r") as packaged:
            corrupt_entry = packaged.testzip()
            if corrupt_entry:
                raise ValueError(f"Generated RBE node bootstrap ZIP failed CRC verification at {corrupt_entry}.")
            names = set(packaged.namelist())
            if not {"api/health.route", "api/status.route"}.issubset(names):
                raise ValueError("Generated RBE node bootstrap ZIP is missing its health/status routes.")
            binary_names = {
                "backend", "backend.exe", "cloud_node", "cloud_node.exe",
                "container", "container.exe", "service", "service.exe",
            }
            if any(
                name.startswith("binaries/") or name.rsplit("/", 1)[-1].lower() in binary_names
                for name in names
            ):
                raise ValueError("Bootstrap ZIP must not pre-bundle RBE executables.")
            compile(packaged.read("setup.py"), "tools/cloud-node-deployment/setup.py", "exec")
            packaged_manifest = json.loads(packaged.read("manifest.json").decode("utf-8"))
            if packaged_manifest != manifest:
                raise ValueError("Generated RBE node bootstrap ZIP changed manifest.json.")

        os.replace(temporary_archive, final_archive)
        size = final_archive.stat().st_size
        log_ok(f"Created verified RBE node bootstrap ZIP: {final_archive} ({size:,} bytes)")
        log_info("Included: setup.py, RBE settings templates, /api routes and licenses.")
        log_info("RBE binaries are downloaded and checksum-verified for the target machine when setup.py runs.")
        return 0
    except (OSError, ValueError, RuntimeError, json.JSONDecodeError, zipfile.BadZipFile) as error:
        log_err(f"RBE node bootstrap packaging failed: {error}")
        return 1

def prepare_windows_tauri_signing_config(project_root: Path) -> Optional[Path]:
    """Write an explicit Windows signing override for every release build.

    Nulls in Tauri's merged config remove stale or externally inherited
    certificate/signCommand values. Unsigned builds must never invoke a signer
    and then fail SignTool verification because it produced no signature.
    """
    if platform.system() != "Windows":
        return None

    thumbprint = re.sub(r"\s", "", os.environ.get("WINDOWS_SIGNING_THUMBPRINT", "")).upper()
    if thumbprint and not re.fullmatch(r"[A-F0-9]{40}", thumbprint):
        log_err("WINDOWS_SIGNING_THUMBPRINT must be a 40-character SHA-1 thumbprint.")
        return None

    # Windows package formats belong to MPDEV. Tauri compiles the application
    # and sidecars but must not invoke NSIS or WiX or produce an installer.
    signing_config: Dict[str, object] = {"bundle": {"active": False}}
    log_info("Windows Tauri bundling is disabled; MPDEV will package the staged raw application.")

    config_path = project_root / "target" / "build-config" / "tauri.windows.signing.conf.json"
    try:
        config_path.parent.mkdir(parents=True, exist_ok=True)
        config_path.write_text(json.dumps(signing_config, indent=2) + "\n", encoding="utf-8")
    except OSError as error:
        log_err(f"Could not write the Windows signing override {config_path}: {error}")
        return None
    return config_path


def sanitize_tauri_v2_config(project_root: Path) -> None:
    """
    Validates and auto-fixes `src-tauri/tauri.conf.json` against the Tauri v2 schema:
    - Removes Tauri v1 legacy keys from `build` (such as `build.withGlobalTauri` or `build.distDir`).
    - Ensures the Engine-backed static export and Tauri build/dev hooks are configured.
    """
    conf_path = project_root / "src-tauri" / "tauri.conf.json"
    if not conf_path.is_file():
        return

    try:
        data = json.loads(conf_path.read_text(encoding="utf-8"))
        changed = False

        build_cfg = data.setdefault("build", {})
        if "withGlobalTauri" in build_cfg:
            del build_cfg["withGlobalTauri"]
            changed = True
            log_info("Sanitized `tauri.conf.json`: removed legacy `build.withGlobalTauri` (moved to `app.withGlobalTauri`).")

        if "distDir" in build_cfg:
            build_cfg["frontendDist"] = build_cfg.pop("distDir")
            changed = True

        expected_frontend_dist = "../.cache/nextjs-engine/workspace/out"
        if build_cfg.get("frontendDist") != expected_frontend_dist:
            build_cfg["frontendDist"] = expected_frontend_dist
            changed = True

        expected_hooks = {
            "beforeBuildCommand": "python tools/nextjs_engine_source.py build --project-root .",
            "beforeDevCommand": "python tools/nextjs_engine_source.py dev --project-root .",
            "devUrl": "http://localhost:3000",
        }
        for key, expected in expected_hooks.items():
            if build_cfg.get(key) != expected:
                build_cfg[key] = expected
                changed = True

        app_cfg = data.setdefault("app", {})
        if app_cfg.get("withGlobalTauri") is not True:
            app_cfg["withGlobalTauri"] = True
            changed = True

        if changed:
            conf_path.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
            log_ok("`src-tauri/tauri.conf.json` verified and updated for Tauri v2 schema.")
        else:
            log_ok("`src-tauri/tauri.conf.json` matches Tauri v2 schema.")
    except Exception as exc:
        log_warn(f"Could not inspect `tauri.conf.json`: {exc}")


def detect_installed_rust_toolchains() -> List[Tuple[str, Tuple[int, int, int]]]:
    """Queries `rustup toolchain list` (if rustup is installed) to see all locally installed toolchains."""
    discovered: List[Tuple[str, Tuple[int, int, int]]] = []
    if not shutil.which("rustup"):
        return discovered

    code, out, _ = run_capture(["rustup", "toolchain", "list"])
    if code != 0 or not out:
        return discovered

    for line in out.splitlines():
        toolchain_name = line.split()[0].strip()
        if not toolchain_name:
            continue
        c_code, c_out, _ = run_capture(["rustup", "run", toolchain_name, "rustc", "--version"])
        if c_code == 0:
            ver = parse_semver(c_out)
            if ver:
                discovered.append((toolchain_name, ver))
    return discovered


def detect_installed_toolchains_sorted() -> List[Tuple[str, Tuple[int, int, int]]]:
    items = detect_installed_rust_toolchains()
    items.sort(key=lambda x: x[1], reverse=True)
    return items


def relax_cargo_toml_rust_version(cargo_toml: Path) -> None:
    """Ensures `rust-version` in Cargo.toml never blocks an already-installed older Rust compiler."""
    if not cargo_toml.is_file():
        return
    content = cargo_toml.read_text(encoding="utf-8")
    updated = re.sub(r'(?m)^rust-version\s*=\s*"[^"]+"\s*\n?', "", content)
    if updated != content:
        cargo_toml.write_text(updated, encoding="utf-8")
        log_info(f"Adapted {cargo_toml.name} to allow building with your installed Rust compiler.")


def check_and_resolve_rust(project_root: Path) -> Optional[str]:
    """
    Checks if `rustc` and `cargo` are installed.
    - If a different version than RECOMMENDED_RUST_VERSION is installed, logs the difference
      and uses the version that is ALREADY installed on the system.
    - Returns an optional `+toolchain` arg if switching to another already-installed rustup
      toolchain is needed, or "" to use the default installed `cargo`/`rustc`.
    """
    ensure_cargo_in_path()

    rustc_path = shutil.which("rustc")
    cargo_path = shutil.which("cargo")

    if not rustc_path or not cargo_path:
        log_err("Rust (`rustc` / `cargo`) is NOT installed or not found in PATH.")
        log_info("Install Rust via https://rustup.rs or your OS package manager:")
        if platform.system() == "Windows":
            log_info("  winget install Rustlang.Rustup")
        else:
            log_info("  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh")
        return None

    _, rustc_out, _ = run_capture([rustc_path, "--version"])
    _, cargo_out, _ = run_capture([cargo_path, "--version"])
    installed_ver = parse_semver(rustc_out)

    if not installed_ver:
        log_warn(f"Could not parse exact semver from `{rustc_out}`, proceeding with installed compiler.")
        return ""

    rec_str = format_semver(RECOMMENDED_RUST_VERSION)
    inst_str = format_semver(installed_ver)

    if installed_ver == RECOMMENDED_RUST_VERSION:
        log_ok(f"Rust version matches target baseline exactly: {rustc_out} ({cargo_out})")
        return ""

    if installed_ver > RECOMMENDED_RUST_VERSION:
        log_ok(
            f"Different (newer) Rust version detected ({inst_str} vs baseline {rec_str}). "
            f"Using already-installed Rust {inst_str}!"
        )
        return ""

    log_warn(
        f"Active Rust version ({inst_str}) is older than target baseline ({rec_str}). "
        "Checking if another Rust version is already installed locally..."
    )
    installed_toolchains = detect_installed_toolchains_sorted()
    for tc_name, tc_ver in installed_toolchains:
        if tc_ver >= RECOMMENDED_RUST_VERSION:
            log_ok(
                f"Found already-installed toolchain '{tc_name}' (Rust {format_semver(tc_ver)}). "
                f"Using +{tc_name} automatically!"
            )
            return f"+{tc_name}"

    log_warn(
        f"No newer toolchain installed locally; adapting build to use your already-installed Rust {inst_str}."
    )
    relax_cargo_toml_rust_version(project_root / "src-tauri" / "Cargo.toml")
    return ""


def check_tauri_rust_cli(toolchain_arg: str, auto_install: bool = False) -> bool:
    """
    Checks for the Rust `tauri-cli` crate (`cargo tauri --version`).
    If a different version is installed, uses the already-installed version.
    """
    cmd = ["cargo"]
    if toolchain_arg:
        cmd.append(toolchain_arg)
    cmd.extend(["tauri", "--version"])

    code, out, _ = run_capture(cmd)
    if code == 0 and out:
        ver = parse_semver(out)
        rec_str = format_semver(RECOMMENDED_TAURI_CRATE_VERSION)
        if ver:
            inst_str = format_semver(ver)
            if ver != RECOMMENDED_TAURI_CRATE_VERSION:
                log_ok(
                    f"Rust `cargo-tauri` CLI detected ({out}, version {inst_str} vs baseline {rec_str}). "
                    f"Using already-installed `cargo-tauri` {inst_str}."
                )
            else:
                log_ok(f"Rust `cargo-tauri` CLI detected: {out}")
        else:
            log_ok(f"Rust `cargo-tauri` CLI detected: {out}")
        return True

    log_warn("Rust `tauri-cli` (`cargo tauri`) subcommand is not installed yet.")
    if auto_install:
        log_info("Installing Rust `tauri-cli` crate via `cargo install tauri-cli`...")
        install_cmd = ["cargo"]
        if toolchain_arg:
            install_cmd.append(toolchain_arg)
        install_cmd.extend(["install", "tauri-cli", "--locked"])
        rc = subprocess.call(install_cmd)
        if rc == 0:
            log_ok("Successfully installed Rust `tauri-cli` (`cargo tauri`).")
            return True
        log_warn("Could not install `tauri-cli`; falling back to direct `cargo` build.")
    else:
        log_info(
            "Will build/run directly with standard `cargo` (pass `--install-tauri-cli` if you want `cargo tauri` bundler installed)."
        )
    return False


def check_os_system_dependencies() -> None:
    sys_name = platform.system()
    log_info(f"Checking native platform dependencies for {sys_name} ({platform.release()})...")

    if sys_name == "Windows":
        log_ok("Windows 10/11 target detected (uses native Win32/WebView2 via Pure-Rust Tauri crate).")
    elif sys_name == "Darwin":
        code, out, _ = run_capture(["xcode-select", "-p"])
        if code == 0:
            log_ok(f"macOS Xcode Command Line Tools found at: {out}")
        else:
            log_warn("Xcode Command Line Tools not detected. Run: `xcode-select --install`")
    elif sys_name == "Linux":
        pkg_cfg = shutil.which("pkg-config")
        if pkg_cfg:
            for lib in ["webkit2gtk-4.1", "glib-2.0", "gtk+-3.0", "openssl"]:
                c, v, _ = run_capture([pkg_cfg, "--modversion", lib])
                if c == 0:
                    log_ok(f"Linux native library `{lib}` found (version {v} installed).")
                else:
                    log_warn(
                        f"Linux library `{lib}` not found via pkg-config. "
                        "On Debian/Ubuntu install: sudo apt-get install libwebkit2gtk-4.1-dev libssl-dev libgtk-3-dev"
                    )
        else:
            log_warn("`pkg-config` not found; skipping Linux `.pc` library version checks.")


def resolve_rust_target_triple(toolchain_arg: str) -> str:
    """Return the exact Rust target triple used for the GitManager sidecar."""
    commands: List[List[str]] = []
    if toolchain_arg.startswith("+") and shutil.which("rustup"):
        toolchain = toolchain_arg[1:]
        commands.append(["rustup", "run", toolchain, "rustc", "--print", "host-tuple"])
        commands.append(["rustup", "run", toolchain, "rustc", "-vV"])
    commands.append(["rustc", "--print", "host-tuple"])
    commands.append(["rustc", "-vV"])
    for command in commands:
        code, stdout, _ = run_capture(command)
        if code != 0 or not stdout:
            continue
        if "host-tuple" in command:
            value = stdout.strip()
            if value:
                return value
        match = re.search(r"(?m)^host:\s*(\S+)\s*$", stdout)
        if match:
            return match.group(1)
    raise RuntimeError("Could not determine the Rust target triple for the gitm sidecar.")


def build_gitm_sidecar(
    project_root: Path,
    toolchain_arg: str,
    release: bool,
) -> int:
    """Build and stage gitm, first-run setup, crash_helper and the independent service."""
    profile = "release" if release else "debug"
    common_prefix = ["cargo"]
    if toolchain_arg:
        common_prefix.append(toolchain_arg)

    commands = [
        (
            "native gitm CLI",
            common_prefix + ["build", "--package", "gitm-cli", "--bin", "gitm"],
        ),
        (
            "GitManager first-run setup companion",
            common_prefix + ["build", "--package", "gitmanager-setup", "--bin", "gitmanager-setup"],
        ),
        (
            "crash_helper UI supervisor",
            common_prefix + ["build", "--package", "crash-helper", "--bin", "crash_helper"],
        ),
    ]
    if release:
        for _, command in commands:
            command.append("--release")

    for description, command in commands:
        log_info(f"Building {description}: {' '.join(command)}")
        result = subprocess.call(command, cwd=str(project_root))
        if result != 0:
            log_err(f"The {description} failed to build.")
            return result

    try:
        host_triple = resolve_rust_target_triple(toolchain_arg)
    except RuntimeError as error:
        log_err(str(error))
        return 1

    target_dir = Path(os.environ.get("CARGO_TARGET_DIR", project_root / "target"))
    if not target_dir.is_absolute():
        target_dir = project_root / target_dir
    target_triple = os.environ.get("CARGO_BUILD_TARGET", "").strip()
    binary_triple = target_triple or host_triple
    binary_dir = target_dir / target_triple / profile if target_triple else target_dir / profile

    executable_suffix = ".exe" if "windows" in binary_triple else ""
    sidecar_dir = project_root / "src-tauri" / "binaries"
    sidecar_dir.mkdir(parents=True, exist_ok=True)

    artifacts = [
        (binary_dir / f"gitm{executable_suffix}", f"gitm-{binary_triple}{executable_suffix}"),
        (binary_dir / f"gitmanager-setup{executable_suffix}", f"gitmanager-setup-{binary_triple}{executable_suffix}"),
        (binary_dir / f"crash_helper{executable_suffix}", f"crash_helper-{binary_triple}{executable_suffix}"),
    ]
    staged: List[Path] = []

    # Tauri's build script validates every externalBin entry when the
    # standalone service crate builds its dependency on the Tauri library.
    # Stage the other tools first, then provide a placeholder for the service
    # sidecar while compiling the separate service package.
    for source_path, filename in artifacts:
        if not source_path.is_file():
            log_err(f"Cargo completed but the expected sidecar binary is missing: {source_path}")
            return 1
        destination = sidecar_dir / filename
        try:
            shutil.copy2(source_path, destination)
        except OSError as error:
            log_err(f"Could not stage sidecar at {destination}: {error}")
            return 1
        staged.append(destination)
        log_ok(f"Prepared Tauri sidecar: {destination} ({destination.stat().st_size:,} bytes)")

    service_filename = f"git-manager-service-{binary_triple}{executable_suffix}"
    service_sidecar = sidecar_dir / service_filename
    service_source = binary_dir / f"git-manager-service{executable_suffix}"

    # The service is a binary in the Tauri package, whose build script itself
    # requires every declared externalBin file to exist. Keep any previous
    # working sidecar as a backup, then supply a temporary placeholder so the
    # first service build can run. The real Cargo output replaces it immediately.
    service_backup: Optional[Path] = None
    try:
        if service_sidecar.is_file():
            service_backup = service_sidecar.with_name(service_sidecar.name + ".previous-build")
            if service_backup.exists():
                service_backup.unlink()
            shutil.copy2(service_sidecar, service_backup)
        service_sidecar.write_bytes(b"")
    except OSError as error:
        log_err(f"Could not prepare the service sidecar build placeholder: {error}")
        return 1

    service_cmd = common_prefix + ["build", "--package", "git-manager-service", "--bin", "git-manager-service"]
    if release:
        service_cmd.append("--release")
    log_info(f"Building independent GitManager background service: {' '.join(service_cmd)}")
    service_result = subprocess.call(service_cmd, cwd=str(project_root))
    if service_result != 0 or not service_source.is_file():
        try:
            if service_sidecar.exists():
                service_sidecar.unlink()
            if service_backup and service_backup.is_file():
                shutil.copy2(service_backup, service_sidecar)
                service_backup.unlink()
        except OSError as error:
            log_warn(f"Could not restore the previous service sidecar after a failed build: {error}")
        if service_result != 0:
            log_err("The standalone GitManager background service failed to build.")
            return service_result
        log_err(f"Cargo completed but the expected service binary is missing: {service_source}")
        return 1

    try:
        shutil.copy2(service_source, service_sidecar)
        if service_backup and service_backup.is_file():
            service_backup.unlink()
    except OSError as error:
        log_err(f"Could not stage the built background service at {service_sidecar}: {error}")
        return 1

    staged.append(service_sidecar)
    log_ok(f"Prepared Tauri sidecar: {service_sidecar} ({service_sidecar.stat().st_size:,} bytes)")

    # Publish a predictable standalone setup helper beside the Cargo outputs:
    # GitManager-Setup.exe on Windows and GitManager-Setup on Linux/macOS.
    setup_artifact_name = "GitManager-Setup.exe" if executable_suffix else "GitManager-Setup"
    setup_artifact = binary_dir / setup_artifact_name
    try:
        shutil.copy2(staged[1], setup_artifact)
    except OSError as error:
        log_err(f"Could not stage the standalone setup helper at {setup_artifact}: {error}")
        return 1

    # When CI has imported a production signing certificate, sign every sidecar
    # and the predictable setup-helper output before Tauri bundles the sidecars.
    if platform.system() == "Windows" and os.environ.get("WINDOWS_SIGNING_THUMBPRINT", "").strip():
        sign_script = project_root / "tools" / "sign_windows_file.ps1"
        if sign_script.is_file():
            for target in [*staged, setup_artifact]:
                sign_cmd = [
                    "powershell.exe", "-NoLogo", "-NoProfile", "-NonInteractive",
                    "-ExecutionPolicy", "Bypass", "-File", str(sign_script),
                    "-FilePath", str(target),
                ]
                sign_rc = subprocess.call(sign_cmd, cwd=str(project_root))
                if sign_rc != 0:
                    log_err(f"Could not sign GitManager build output: {target}")
                    return sign_rc

    log_ok(f"Created standalone setup helper: {setup_artifact} ({setup_artifact.stat().st_size:,} bytes)")
    return 0

def run_build_or_dev(
    project_root: Path,
    mode: str,
    toolchain_arg: str,
    has_cargo_tauri: bool,
    release: bool,
) -> int:
    src_tauri_dir = project_root / "src-tauri"

    if mode == "check":
        log_info("Running `cargo check` on Pure-Rust Tauri v2 backend...")
        cmd = ["cargo"]
        if toolchain_arg:
            cmd.append(toolchain_arg)
        cmd.extend(["check", "--manifest-path", str(src_tauri_dir / "Cargo.toml")])
        return subprocess.call(cmd)

    if mode == "build" and os.environ.get("GITMANAGER_SKIP_CLOUD_NODE_BUNDLE") != "1":
        cloud_node_result = prepare_cloud_node_deployment(project_root)
        if cloud_node_result != 0:
            return cloud_node_result

    if mode == "build" and platform.system() == "Windows":
        runtime_result = prepare_windows_python_runtime(project_root)
        if runtime_result != 0:
            return runtime_result

    if mode == "build" and not has_cargo_tauri:
        log_err("The Tauri CLI is required to create the Windows application bundle for Master Packager Dev. Install failed or pass --install-tauri-cli.")
        return 1

    if mode in ("build", "dev"):
        sidecar_result = build_gitm_sidecar(project_root, toolchain_arg, release)
        if sidecar_result != 0:
            return sidecar_result

    if has_cargo_tauri:
        cmd = ["cargo"]
        if toolchain_arg:
            cmd.append(toolchain_arg)
        if mode == "dev":
            cmd.extend(["tauri", "dev"])
        else:
            cmd.extend(["tauri", "build"])
            if not release:
                cmd.append("--debug")
            elif platform.system() == "Windows":
                signing_config = prepare_windows_tauri_signing_config(project_root)
                if signing_config is None:
                    log_err("Could not prepare an explicit Windows signing override; refusing to build with an ambiguous signing configuration.")
                    return 1
                cmd.extend(["--config", str(signing_config.resolve())])
        log_info(f"Running Pure-Rust Tauri CLI: {' '.join(cmd)}")
        rc = subprocess.call(cmd, cwd=str(src_tauri_dir))
        if rc != 0 or mode != "build":
            return rc

        # The dedicated service executable and crash_helper are bundled as
        # separate sidecars; the background service never shares the UI process tree.
        if platform.system() == "Windows":
            stage_result = stage_windows_mpdev_package(project_root, toolchain_arg)
            if stage_result != 0:
                return stage_result
        return 0

    # Fallback direct Cargo execution
    cmd = ["cargo"]
    if toolchain_arg:
        cmd.append(toolchain_arg)

    if mode == "dev":
        cmd.extend(["run", "--bin", "git-manager", "--manifest-path", str(src_tauri_dir / "Cargo.toml")])
    else:
        cmd.extend(["build", "--bins", "--manifest-path", str(src_tauri_dir / "Cargo.toml")])
        if release:
            cmd.append("--release")

    log_info(f"Running Pure-Rust Cargo directly: {' '.join(cmd)}")
    return subprocess.call(cmd, cwd=str(project_root))


def main() -> int:
    parser = argparse.ArgumentParser(
        description="GitManager native dependency, runtime and installer build script"
    )
    parser.add_argument(
        "mode",
        nargs="?",
        default="build",
        choices=["deps", "check", "dev", "build", "frontend-build", "frontend-dev", "prepare-runtime", "prepare-sidecar", "prepare-cloud-node"],
        help="Action: deps, check, dev, build, frontend-build, frontend-dev, prepare-runtime, prepare-sidecar, or prepare-cloud-node.",
    )
    parser.add_argument(
        "--release",
        action="store_true",
        default=True,
        help="Build in release mode (default: True).",
    )
    parser.add_argument(
        "--debug",
        action="store_true",
        help="Build in debug mode instead of release mode.",
    )
    parser.add_argument(
        "--install-tauri-cli",
        action="store_true",
        help="Install the Rust `tauri-cli` crate via `cargo install tauri-cli` if not already present.",
    )

    args = parser.parse_args()
    project_root = Path(__file__).resolve().parent
    os.environ["CARGO_TARGET_DIR"] = str(project_root / "build" / ".rcache")

    print("=" * 72)
    print(" GitManager — Pure-Rust Tauri v2 Dependency & Toolchain Manager")
    print("=" * 72)

    if args.mode == "prepare-runtime":
        return prepare_windows_python_runtime(project_root)
    if args.mode == "prepare-cloud-node":
        return prepare_cloud_node_deployment(project_root)
    if args.mode in ("frontend-build", "frontend-dev"):
        frontend_tool = project_root / "tools" / "nextjs_engine_source.py"
        frontend_mode = "build" if args.mode == "frontend-build" else "dev"
        return subprocess.call([
            sys.executable,
            str(frontend_tool),
            frontend_mode,
            "--project-root",
            str(project_root),
        ])

    check_os_system_dependencies()
    sanitize_tauri_v2_config(project_root)

    toolchain_arg = check_and_resolve_rust(project_root)
    if toolchain_arg is None:
        return 1

    report_dependency_count(project_root, toolchain_arg)
    log_info(
        "Cargo's later n/N progress meter counts compilation units (including build scripts and "
        "host/target variants), not unique crates; use the package-scoped dependency graph counts above."
    )

    has_cargo_tauri = check_tauri_rust_cli(
        toolchain_arg,
        auto_install=args.install_tauri_cli or args.mode == "build",
    )

    if args.mode == "deps":
        log_ok("All dependency checks finished.")
        return 0

    is_release = not args.debug
    if args.mode == "prepare-sidecar":
        return build_gitm_sidecar(project_root, toolchain_arg, is_release)

    return run_build_or_dev(
        project_root=project_root,
        mode=args.mode,
        toolchain_arg=toolchain_arg,
        has_cargo_tauri=has_cargo_tauri,
        release=is_release,
    )


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Install a target-specific RBE runtime from the official GitHub release."""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
import platform
import plistlib
import re
import secrets
import shlex
import shutil
import socket
import stat
import subprocess
import sys
import tarfile
import tempfile
import urllib.error
import urllib.request
import zipfile
from pathlib import Path, PurePosixPath
from urllib.parse import urlsplit

PACKAGE_ROOT = Path(__file__).resolve().parent
MANIFEST_PATH = PACKAGE_ROOT / "manifest.json"
KEY_ENV = "RBE_CLOUD_NODE_PRIVATE_KEY"
SETTINGS_ENV = "RBE_CN_SETTINGS"
NODE_ID_RE = re.compile(r"^[A-Za-z0-9_.-]{1,128}$")
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
MAX_RELEASE_METADATA = 2 * 1024 * 1024
MAX_CHECKSUM_BYTES = 1024 * 1024
MAX_ARCHIVE_BYTES = 128 * 1024 * 1024
MAX_BINARY_BYTES = 512 * 1024 * 1024
MAX_TOTAL_EXTRACT_BYTES = 1024 * 1024 * 1024
RBE_API_HOST = "api.github.com"
RBE_DOWNLOAD_HOSTS = {
    "github.com",
    "release-assets.githubusercontent.com",
    "objects.githubusercontent.com",
}
REQUIRED_FILES = {
    "windows-x64": ("backend.exe", "cloud_node.exe", "dep/container.exe", "dep/service.exe", "build-info.json"),
    "linux-x64": ("backend", "cloud_node", "dep/container", "dep/service", "build-info.json"),
    "linux-arm64": ("backend", "cloud_node", "dep/container", "dep/service", "build-info.json"),
    "macos-intel": ("backend", "cloud_node", "dep/container", "dep/service", "build-info.json"),
    "macos-arm64": ("backend", "cloud_node", "dep/container", "dep/service", "build-info.json"),
}


class SetupError(RuntimeError):
    pass


def say(message=""):
    print(message)


def ask(label, default=None):
    suffix = f" [{default}]" if default is not None else ""
    value = input(f"{label}{suffix}: ").strip()
    return value if value else (default if default is not None else "")


def ask_yes_no(label, default=True):
    choice = "Y/n" if default else "y/N"
    value = input(f"{label} ({choice}): ").strip().lower()
    if not value:
        return default
    if value in ("y", "yes"):
        return True
    if value in ("n", "no"):
        return False
    raise SetupError("Please answer yes or no.")


def run_checked(args, *, cwd=None, env=None, timeout=30, capture=True):
    try:
        return subprocess.run(
            [str(item) for item in args],
            cwd=str(cwd) if cwd is not None else None,
            env=env,
            stdout=subprocess.PIPE if capture else None,
            stderr=subprocess.PIPE if capture else None,
            text=True if capture else False,
            encoding="utf-8" if capture else None,
            errors="replace" if capture else None,
            timeout=timeout,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise SetupError(f"Could not run {args[0]}: {error}") from error


def run_interactive(args, *, cwd=None, env=None):
    """Run an installer-owned CLI with the user's terminal attached."""
    try:
        return subprocess.run(
            [str(item) for item in args],
            cwd=str(cwd) if cwd is not None else None,
            env=env,
            stdin=None,
            stdout=None,
            stderr=None,
            check=False,
        )
    except OSError as error:
        raise SetupError(f"Could not run {args[0]} interactively: {error}") from error


def initialize_backend_admin(backend_path, settings_path, install_dir):
    env = os.environ.copy()
    result = run_interactive(
        [backend_path, "--settings", settings_path, "--init-admin"],
        cwd=install_dir,
        env=env,
    )
    return result.returncode == 0


def sha256_bytes(payload):
    return hashlib.sha256(payload).hexdigest()


def sha256_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def load_manifest():
    try:
        manifest = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SetupError(f"Could not read package manifest.json: {error}") from error
    if not isinstance(manifest, dict):
        raise SetupError("Package manifest must be a JSON object.")
    if manifest.get("formatVersion") != 2 or manifest.get("kind") != "gitmanager-rbe-runtime-bootstrap":
        raise SetupError("Unsupported GitManager RBE bootstrap package format.")
    if manifest.get("rbeRepository") != "Kastrick8/RBE":
        raise SetupError("Package manifest points to an unexpected RBE repository.")
    expected_api = "https://api.github.com/repos/Kastrick8/RBE/releases/latest"
    if manifest.get("releaseApi") != expected_api:
        raise SetupError("Package manifest contains an unexpected RBE release API URL.")
    targets = manifest.get("targets")
    if not isinstance(targets, dict) or set(targets) != set(REQUIRED_FILES):
        raise SetupError("Package manifest does not contain the supported RBE target map.")
    return manifest


def linux_glibc_version():
    candidates = []
    try:
        value = os.confstr("CS_GNU_LIBC_VERSION")
        if value:
            candidates.append(value)
    except (AttributeError, OSError, ValueError):
        pass
    libc_name, libc_version = platform.libc_ver()
    if libc_name:
        candidates.append(f"{libc_name} {libc_version}")
    for candidate in candidates:
        match = re.search(r"glibc\s+(\d+)\.(\d+)", candidate, re.IGNORECASE)
        if match:
            return int(match.group(1)), int(match.group(2))
    return None


def detect_target():
    machine = platform.machine().lower()
    if sys.platform.startswith("win"):
        if machine in ("amd64", "x86_64", "x64"):
            return "windows-x64"
        raise SetupError(f"Windows architecture {machine!r} is not supported; the RBE release supplies Windows x64.")
    if sys.platform == "darwin":
        if machine in ("x86_64", "amd64"):
            return "macos-intel"
        if machine in ("arm64", "aarch64"):
            return "macos-arm64"
        raise SetupError(f"Unsupported macOS CPU architecture: {machine}")
    if sys.platform.startswith("linux"):
        if machine in ("x86_64", "amd64"):
            target, baseline = "linux-x64", (2, 35)
        elif machine in ("aarch64", "arm64"):
            target, baseline = "linux-arm64", (2, 39)
        else:
            raise SetupError(f"Unsupported Linux CPU architecture: {machine}")
        libc = linux_glibc_version()
        if libc is None:
            raise SetupError("Could not verify glibc. The current RBE release targets glibc Linux, not musl/Alpine.")
        if libc < baseline:
            raise SetupError(
                f"The RBE {target} binary requires glibc {baseline[0]}.{baseline[1]} or newer; "
                f"this host reports {libc[0]}.{libc[1]}."
            )
        return target
    raise SetupError(f"Unsupported operating system: {platform.system()}")


def default_install_directory():
    home = Path.home()
    if os.name == "nt":
        root = Path(os.environ.get("LOCALAPPDATA", home / "AppData" / "Local"))
        return root / "Kastrick" / "GitManager" / "RBE-Node"
    if sys.platform == "darwin":
        return home / "Library" / "Application Support" / "Kastrick" / "GitManager" / "RBE-Node"
    root = Path(os.environ.get("XDG_DATA_HOME", home / ".local" / "share"))
    return root / "kastrick" / "gitmanager" / "rbe-node"


def validate_node_id(value):
    if not NODE_ID_RE.fullmatch(value) or value in (".", ".."):
        raise SetupError("Node IDs must use 1-128 ASCII letters, digits, dots, underscores, or hyphens.")
    return value


def validate_port(value, label):
    try:
        port = int(value)
    except (TypeError, ValueError) as error:
        raise SetupError(f"{label} must be a number.") from error
    if not 1 <= port <= 65535:
        raise SetupError(f"{label} must be between 1 and 65535.")
    return port


def secure_write(path, content, *, secret=False):
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    fd, temporary_name = tempfile.mkstemp(prefix=path.name + ".", suffix=".tmp", dir=str(path.parent))
    temporary = Path(temporary_name)
    try:
        if secret and os.name != "nt":
            os.chmod(temporary, 0o600)
        elif os.name != "nt":
            os.chmod(temporary, 0o644)
        with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as stream:
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        if secret and os.name != "nt":
            os.chmod(path, 0o600)
        elif not secret and os.name != "nt":
            os.chmod(path, 0o644)
        if secret and os.name == "nt":
            restrict_windows_secret_acl(path)
    except Exception:
        try:
            os.close(fd)
        except OSError:
            pass
        try:
            temporary.unlink()
        except OSError:
            pass
        raise


def save_json(path, value, *, secret=False):
    secure_write(path, json.dumps(value, indent=2, ensure_ascii=False) + "\n", secret=secret)


def windows_current_user_sid():
    system_root = os.environ.get("SystemRoot")
    if not system_root:
        raise SetupError("Windows SystemRoot is unavailable; cannot secure the Cloud Node private key.")
    whoami = Path(system_root) / "System32" / "whoami.exe"
    result = run_checked([whoami, "/user", "/fo", "csv", "/nh"], timeout=15)
    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip()
        raise SetupError(f"Could not identify the current Windows user SID: {detail}")
    import csv
    for row in csv.reader(result.stdout.splitlines()):
        for field in row:
            sid = field.strip()
            if re.fullmatch(r"S-1-(?:[0-9]+-)*[0-9]+", sid):
                return sid
    raise SetupError("Windows did not return a valid current-user SID.")


def restrict_windows_secret_acl(path):
    system_root = os.environ.get("SystemRoot")
    if not system_root:
        raise SetupError("Windows SystemRoot is unavailable; cannot secure the Cloud Node private key.")
    icacls = Path(system_root) / "System32" / "icacls.exe"
    sid = windows_current_user_sid()
    reset = run_checked([icacls, path, "/reset"], timeout=15)
    if reset.returncode != 0:
        raise SetupError("Could not reset private-key file permissions: " + (reset.stderr.strip() or reset.stdout.strip()))
    result = run_checked([icacls, path, "/inheritance:r", "/grant:r", f"*{sid}:(F)"], timeout=15)
    if result.returncode != 0:
        raise SetupError("Could not restrict the private key to the current Windows user: " + (result.stderr.strip() or result.stdout.strip()))


def restrict_secret_file_permissions(path):
    if os.name == "nt":
        restrict_windows_secret_acl(path)
    else:
        os.chmod(path, 0o600)


def read_url(url, limit, allowed_hosts):
    parsed = urlsplit(url)
    if parsed.scheme != "https" or parsed.hostname not in allowed_hosts or parsed.username or parsed.password:
        raise SetupError("Refusing an unexpected or non-HTTPS download URL.")
    request = urllib.request.Request(
        url,
        headers={
            "User-Agent": "GitManager-RBE-Node-Setup",
            "Accept": "application/vnd.github+json, application/octet-stream, text/plain",
        },
    )
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            final = urlsplit(response.geturl())
            if final.scheme != "https" or final.hostname not in allowed_hosts:
                raise SetupError("The RBE download redirected to an untrusted host.")
            payload = response.read(limit + 1)
    except (OSError, urllib.error.URLError) as error:
        raise SetupError(f"Could not download {parsed.hostname}: {error}") from error
    if len(payload) > limit:
        raise SetupError(f"Download exceeded the {limit:,}-byte safety limit.")
    return payload


def parse_asset_digest(asset):
    digest = str(asset.get("digest") or "")
    match = re.fullmatch(r"sha256:([0-9a-fA-F]{64})", digest)
    if not match:
        raise SetupError(f"RBE asset {asset.get('name', '<unknown>')} has no valid GitHub SHA-256 digest.")
    size = asset.get("size")
    if not isinstance(size, int) or size <= 0:
        raise SetupError(f"RBE asset {asset.get('name', '<unknown>')} has an invalid size.")
    return match.group(1).lower(), size


def find_release_assets(release, target, manifest):
    tag = str(release.get("tag_name") or "")
    version_match = re.fullmatch(r"rbe-bin-v(\d+\.\d+\.\d+)", tag)
    if not version_match or release.get("draft") or release.get("prerelease"):
        raise SetupError("The latest RBE GitHub Release is not a published stable RBE binary release.")
    version = version_match.group(1)
    target_spec = manifest["targets"][target]
    build_suffix = r"(\d+\.\d+)"
    extension = re.escape(target_spec["extension"])
    pattern = re.compile(
        rf"^rbe-{re.escape(version)}-{re.escape(target_spec['slug'])}-build\.{build_suffix}\.{extension}$"
    )
    assets = release.get("assets")
    if not isinstance(assets, list):
        raise SetupError("The latest RBE release has no asset list.")
    matches = []
    for asset in assets:
        if not isinstance(asset, dict):
            continue
        match = pattern.fullmatch(str(asset.get("name") or ""))
        if match:
            matches.append((asset, match.group(1)))
    if len(matches) != 1:
        raise SetupError(f"The latest RBE release must contain exactly one {target} runtime archive.")
    archive, build_id = matches[0]
    checksum_name = f"SHA256SUMS-build.{build_id}.txt"
    checksum_matches = [asset for asset in assets if isinstance(asset, dict) and asset.get("name") == checksum_name]
    if len(checksum_matches) != 1:
        raise SetupError(f"The latest RBE release is missing its matching checksum manifest {checksum_name}.")
    return archive, checksum_matches[0], tag, version, build_id


def verify_release_download(asset, *, max_bytes, label):
    expected_hash, expected_size = parse_asset_digest(asset)
    if expected_size > max_bytes:
        raise SetupError(f"{label} is too large ({expected_size:,} bytes).")
    url = str(asset.get("browser_download_url") or "")
    if not url.startswith("https://github.com/Kastrick8/RBE/releases/download/"):
        raise SetupError(f"{label} points to an unexpected release download URL.")
    payload = read_url(url, max_bytes, RBE_DOWNLOAD_HOSTS)
    if len(payload) != expected_size:
        raise SetupError(f"{label} size mismatch ({len(payload):,} != {expected_size:,}).")
    actual_hash = sha256_bytes(payload)
    if actual_hash != expected_hash:
        raise SetupError(f"{label} failed GitHub Release SHA-256 verification.")
    return payload, actual_hash


def verify_checksum_manifest(payload, archive_name, archive_hash):
    try:
        text = payload.decode("utf-8")
    except UnicodeDecodeError as error:
        raise SetupError("RBE checksum manifest is not UTF-8 text.") from error
    matches = []
    for line in text.splitlines():
        parts = line.strip().split(None, 1)
        if len(parts) != 2:
            continue
        digest, name = parts
        if name.lstrip("*") == archive_name:
            matches.append(digest.lower())
    if len(matches) != 1 or not SHA256_RE.fullmatch(matches[0]) or matches[0] != archive_hash:
        raise SetupError(f"RBE SHA256SUMS does not confirm the downloaded runtime archive {archive_name}.")


def safe_relative_member(name):
    if not isinstance(name, str) or not name or "\\" in name or "\x00" in name:
        raise SetupError("RBE runtime archive contains an invalid path.")
    normalized = name[2:] if name.startswith("./") else name
    path = PurePosixPath(normalized)
    if path.is_absolute() or re.match(r"^[A-Za-z]:", normalized) or any(part in ("", ".", "..") for part in path.parts):
        raise SetupError(f"RBE runtime archive contains an unsafe path: {name!r}")
    return path.as_posix()


def write_extracted_file(stage, relative, stream, declared_size):
    if declared_size <= 0 or declared_size > MAX_BINARY_BYTES:
        raise SetupError(f"RBE runtime file has an invalid or oversized length: {relative}")
    destination = stage.joinpath(*PurePosixPath(relative).parts)
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.exists() or destination.is_symlink():
        raise SetupError(f"RBE runtime archive contains a duplicate path: {relative}")
    digest = hashlib.sha256()
    written = 0
    with destination.open("xb") as output:
        while True:
            chunk = stream.read(1024 * 1024)
            if not chunk:
                break
            written += len(chunk)
            if written > declared_size or written > MAX_BINARY_BYTES:
                raise SetupError(f"RBE runtime file exceeded its declared length: {relative}")
            digest.update(chunk)
            output.write(chunk)
        output.flush()
        os.fsync(output.fileno())
    if written != declared_size:
        raise SetupError(f"RBE runtime file length mismatch: {relative}")
    if relative != "build-info.json" and os.name != "nt":
        os.chmod(destination, 0o755)
    return written


def extract_runtime_archive(payload, archive_kind, target, stage):
    required = set(REQUIRED_FILES[target])
    found = set()
    total = 0
    try:
        if archive_kind == "zip":
            with zipfile.ZipFile(io.BytesIO(payload), "r") as archive:
                for item in archive.infolist():
                    if item.is_dir():
                        continue
                    relative = safe_relative_member(item.filename)
                    if relative not in required:
                        continue
                    mode = (item.external_attr >> 16) & 0xFFFF
                    if stat.S_ISLNK(mode) or (mode and not stat.S_ISREG(mode)):
                        raise SetupError(f"RBE runtime archive contains a non-regular file: {relative}")
                    if relative in found:
                        raise SetupError(f"RBE runtime archive contains a duplicate file: {relative}")
                    if total + item.file_size > MAX_TOTAL_EXTRACT_BYTES:
                        raise SetupError("RBE runtime archive exceeded the total extraction-size limit.")
                    with archive.open(item, "r") as stream:
                        total += write_extracted_file(stage, relative, stream, item.file_size)
                    found.add(relative)
        else:
            with tarfile.open(fileobj=__import__("io").BytesIO(payload), mode="r:gz") as archive:
                for item in archive.getmembers():
                    if item.isdir():
                        continue
                    relative = safe_relative_member(item.name)
                    if relative not in required:
                        continue
                    if not item.isfile() or item.issym() or item.islnk():
                        raise SetupError(f"RBE runtime archive contains a non-regular file: {relative}")
                    if relative in found:
                        raise SetupError(f"RBE runtime archive contains a duplicate file: {relative}")
                    if total + item.size > MAX_TOTAL_EXTRACT_BYTES:
                        raise SetupError("RBE runtime archive exceeded the total extraction-size limit.")
                    stream = archive.extractfile(item)
                    if stream is None:
                        raise SetupError(f"RBE runtime archive could not read {relative}.")
                    with stream:
                        total += write_extracted_file(stage, relative, stream, item.size)
                    found.add(relative)
    except (OSError, tarfile.TarError, zipfile.BadZipFile, RuntimeError) as error:
        raise SetupError(f"Could not safely extract the RBE runtime archive: {error}") from error
    missing = required - found
    if missing:
        raise SetupError("RBE runtime archive is incomplete; missing: " + ", ".join(sorted(missing)))
    if total > MAX_TOTAL_EXTRACT_BYTES:
        raise SetupError("RBE runtime archive exceeded the total extraction-size limit.")
    info_path = stage / "build-info.json"
    try:
        json.loads(info_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise SetupError(f"RBE build-info.json is invalid: {error}") from error


def download_runtime(manifest, target, stage):
    release_api = manifest["releaseApi"]
    metadata = read_url(release_api, MAX_RELEASE_METADATA, {RBE_API_HOST})
    try:
        release = json.loads(metadata.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise SetupError(f"RBE GitHub Release metadata is invalid: {error}") from error
    archive_asset, checksum_asset, tag, version, build_id = find_release_assets(release, target, manifest)
    say(f"Downloading official RBE {tag} build {build_id} for {target}…")
    checksum_payload, _ = verify_release_download(
        checksum_asset, max_bytes=MAX_CHECKSUM_BYTES, label="RBE checksum manifest"
    )
    archive_payload, archive_hash = verify_release_download(
        archive_asset, max_bytes=MAX_ARCHIVE_BYTES, label="RBE runtime archive"
    )
    verify_checksum_manifest(checksum_payload, str(archive_asset["name"]), archive_hash)
    extract_runtime_archive(
        archive_payload,
        manifest["targets"][target]["archive"],
        target,
        stage,
    )
    return {
        "repository": "Kastrick8/RBE",
        "tag": tag,
        "version": version,
        "buildTag": f"{tag}-build.{build_id}",
        "archive": str(archive_asset["name"]),
        "archiveSha256": archive_hash,
        "publishedAt": release.get("published_at"),
        "releaseUrl": release.get("html_url"),
    }


def default_node_id():
    host = re.sub(r"[^A-Za-z0-9_.-]+", "-", platform.node().strip().lower()).strip(".-_")
    return ("gitmanager-nas-" + host)[:128] if host else "gitmanager-nas"


def load_json_file(path, label):
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SetupError(f"Could not read {label}: {error}") from error
    if not isinstance(value, dict):
        raise SetupError(f"{label} must contain a JSON object.")
    return value


def copy_api_routes(install_dir):
    source = PACKAGE_ROOT / "api"
    if not source.is_dir() or source.is_symlink():
        raise SetupError("The bootstrap ZIP is missing its bundled api/ route directory.")
    destination = install_dir / "api"
    if destination.is_symlink():
        raise SetupError("Refusing to copy API routes through a symbolic link.")
    destination.mkdir(parents=True, exist_ok=True)
    copied = 0
    for route in source.rglob("*.route"):
        if route.is_symlink() or not route.is_file():
            raise SetupError(f"API route must be a regular file: {route}")
        relative = route.relative_to(source)
        target = destination / relative
        current = destination
        for part in relative.parts[:-1]:
            current = current / part
            if current.is_symlink():
                raise SetupError(f"Refusing to copy API routes through a symbolic link: {current}")
            current.mkdir(parents=True, exist_ok=True)
        if target.is_symlink() or (target.exists() and not target.is_file()):
            raise SetupError(f"Refusing to replace a non-regular API route path: {target}")
        if target.exists():
            say(f"[KEEP] Existing API route was not overwritten: {target}")
            continue
        target.write_bytes(route.read_bytes())
        copied += 1
    missing_routes = [
        name for name in ("health.route", "status.route")
        if not (destination / name).is_file()
    ]
    if missing_routes:
        raise SetupError("The installed api/ directory is missing: " + ", ".join(missing_routes))
    return copied


def install_runtime(stage, install_dir, target):
    expected = set(REQUIRED_FILES[target])
    for relative in sorted(expected):
        source = stage.joinpath(*PurePosixPath(relative).parts)
        destination = install_dir.joinpath(*PurePosixPath(relative).parts)
        current = install_dir
        for part in PurePosixPath(relative).parts[:-1]:
            current = current / part
            if current.is_symlink():
                raise SetupError(f"Refusing to write through a symbolic link in the install directory: {current}")
        destination.parent.mkdir(parents=True, exist_ok=True)
        if destination.is_symlink() or (destination.exists() and not destination.is_file()):
            raise SetupError(f"Refusing to replace a non-regular runtime path: {destination}")
        os.replace(source, destination)
        if destination.name != "build-info.json" and os.name != "nt":
            os.chmod(destination, 0o755)


def initialize_configuration(install_dir, node_id, api_port):
    settings_path = install_dir / "settings.json"
    cn_settings_path = install_dir / "setting.node.cn.json"
    key_path = install_dir / "cloud-node-private-key.secret"
    settings_template = load_json_file(PACKAGE_ROOT / "settings.json", "settings.json template")
    cn_template = load_json_file(PACKAGE_ROOT / "setting.node.cn.json", "setting.node.cn.json template")

    if settings_path.is_symlink() or (settings_path.exists() and not settings_path.is_file()):
        raise SetupError("Existing settings.json is not a regular file; it was not followed or overwritten.")
    if settings_path.exists():
        settings = load_json_file(settings_path, "existing settings.json")
        say("[KEEP] Existing settings.json preserved.")
    else:
        settings = settings_template
        settings.setdefault("api", {})["port"] = api_port
        save_json(settings_path, settings)

    if cn_settings_path.is_symlink() or (cn_settings_path.exists() and not cn_settings_path.is_file()):
        raise SetupError("Existing setting.node.cn.json is not a regular file; it was not followed or overwritten.")
    if cn_settings_path.exists():
        cn_settings = load_json_file(cn_settings_path, "existing setting.node.cn.json")
        say("[KEEP] Existing setting.node.cn.json preserved.")
    else:
        cn_settings = cn_template
        cn_settings.setdefault("node", {})["id"] = node_id
        cn_settings["node"]["storageRoot"] = str((install_dir / "data" / "cloud-node").resolve())
        save_json(cn_settings_path, cn_settings)

    if key_path.exists():
        if key_path.is_symlink() or not key_path.is_file():
            raise SetupError("Existing Cloud Node private-key path is not a regular file.")
        restrict_secret_file_permissions(key_path)
        if not key_path.read_text(encoding="utf-8").strip():
            raise SetupError("Existing Cloud Node private-key file is empty; it was not replaced.")
        say("[KEEP] Existing Cloud Node private key preserved.")
    else:
        secure_write(key_path, secrets.token_hex(32) + "\n", secret=True)

    storage_root = Path(str(cn_settings.get("node", {}).get("storageRoot", ""))).expanduser()
    if not storage_root.is_absolute():
        storage_root = (install_dir / storage_root).resolve()
        cn_settings["node"]["storageRoot"] = str(storage_root)
        save_json(cn_settings_path, cn_settings)
    storage_root.mkdir(parents=True, exist_ok=True)
    return settings_path, cn_settings_path, key_path, settings, cn_settings


def validate_cloud_node_configuration(binary, config):
    result = run_checked(
        [binary, "--config=" + str(config), "evaluate"],
        cwd=config.parent,
        env=os.environ.copy(),
        timeout=30,
    )
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip()
        raise SetupError(f"RBE Cloud Node rejected setting.node.cn.json: {detail}")
    return result.stdout.strip()


def derive_public_key(binary, config, key_file):
    env = os.environ.copy()
    env[KEY_ENV] = "file:" + str(key_file.resolve())
    env[SETTINGS_ENV] = str(config.resolve())
    result = run_checked([binary, "--config=" + str(config), "public-key"], cwd=config.parent, env=env, timeout=20)
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip()
        raise SetupError(f"Cloud Node could not derive its public key: {detail}")
    value = result.stdout.strip().splitlines()[-1] if result.stdout.strip() else ""
    if not re.fullmatch(r"[0-9a-fA-F]{64}", value):
        raise SetupError("Cloud Node returned an invalid Ed25519 public key.")
    return value.lower()


def powershell_literal(value):
    return "'" + str(value).replace("'", "''") + "'"


def write_launchers(install_dir, backend_path, settings_path, cn_settings_path, key_path):
    logs = install_dir / "logs"
    logs.mkdir(parents=True, exist_ok=True, mode=0o700)
    log_file = logs / "backend.log"
    if os.name == "nt":
        launcher = install_dir / "run-rbe-node.ps1"
        content = (
            "$ErrorActionPreference = 'Stop'\n"
            f"$env:{KEY_ENV} = {powershell_literal('file:' + str(key_path))}\n"
            f"$env:{SETTINGS_ENV} = {powershell_literal(cn_settings_path)}\n"
            f"Set-Location -LiteralPath {powershell_literal(install_dir)}\n"
            f"& {powershell_literal(backend_path)} '--settings' {powershell_literal(settings_path)} "
            f"*>> {powershell_literal(log_file)}\n"
            "exit $LASTEXITCODE\n"
        )
    else:
        launcher = install_dir / "run-rbe-node.sh"
        content = (
            "#!/bin/sh\nset -eu\n"
            f"export {KEY_ENV}={shlex.quote('file:' + str(key_path))}\n"
            f"export {SETTINGS_ENV}={shlex.quote(str(cn_settings_path))}\n"
            f"cd {shlex.quote(str(install_dir))}\n"
            f"exec {shlex.quote(str(backend_path))} --settings {shlex.quote(str(settings_path))} "
            f">> {shlex.quote(str(log_file))} 2>&1\n"
        )
    secure_write(launcher, content)
    if os.name != "nt":
        os.chmod(launcher, 0o700)
    return launcher


def systemd_literal(value):
    return (
        '"'
        + str(value)
        .replace("\\", "\\\\")
        .replace('"', '\\"')
        .replace("%", "%%")
        .replace("\n", "")
        + '"'
    )


def register_windows_backend_service(launcher):
    schtasks = shutil.which("schtasks.exe") or shutil.which("schtasks")
    if not schtasks:
        return False, "Task Scheduler CLI was not found. Run the generated PowerShell launcher manually."
    task_command = (
        "powershell.exe -NoLogo -NoProfile -NonInteractive -WindowStyle Hidden "
        f"-ExecutionPolicy Bypass -File \"{launcher}\""
    )
    create = run_checked([
        schtasks,
        "/Create",
        "/SC", "ONLOGON",
        "/TN", "Kastrick GitManager RBE Backend",
        "/TR", task_command,
        "/RL", "LIMITED",
        "/F",
    ], timeout=20)
    if create.returncode != 0:
        detail = (create.stderr or create.stdout).strip()
        return False, f"Could not register the current-user logon task: {detail}"
    start = run_checked([
        schtasks, "/Run", "/TN", "Kastrick GitManager RBE Backend"
    ], timeout=20)
    if start.returncode != 0:
        detail = (start.stderr or start.stdout).strip()
        return False, f"Task was registered but could not be started: {detail}"
    return True, "Registered and started a per-user Task Scheduler task; it runs again at user logon."


def register_linux_backend_service(install_dir, launcher):
    systemctl = shutil.which("systemctl")
    if not systemctl:
        return False, "systemctl was not found. The manual launcher is ready."
    unit_dir = Path.home() / ".config" / "systemd" / "user"
    unit_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
    unit = unit_dir / "gitmanager-rbe-backend.service"
    unit_text = (
        "[Unit]\n"
        "Description=GitManager RBE Backend\n"
        "Wants=network-online.target\n"
        "After=network-online.target\n\n"
        "[Service]\n"
        "Type=simple\n"
        f"WorkingDirectory={systemd_literal(install_dir)}\n"
        f"ExecStart={systemd_literal(launcher)}\n"
        "Restart=on-failure\n"
        "RestartSec=5\n"
        "UMask=0077\n"
        "NoNewPrivileges=true\n\n"
        "[Install]\n"
        "WantedBy=default.target\n"
    )
    secure_write(unit, unit_text)
    reload = run_checked([systemctl, "--user", "daemon-reload"], timeout=20)
    if reload.returncode != 0:
        detail = (reload.stderr or reload.stdout).strip()
        return False, f"Created {unit}, but systemd user manager is unavailable: {detail}"
    enable = run_checked([
        systemctl, "--user", "enable", "--now", unit.name
    ], timeout=30)
    if enable.returncode != 0:
        detail = (enable.stderr or enable.stdout).strip()
        return False, f"Created {unit}, but could not enable/start it: {detail}"
    return True, "Registered and started the per-user systemd unit gitmanager-rbe-backend.service."


def register_macos_backend_service(install_dir, launcher):
    uid = getattr(os, "getuid", lambda: None)()
    if uid is None:
        return False, "Could not determine the current macOS user ID."
    label = "com.kastrick.gitmanager-rbe-backend"
    agent_dir = Path.home() / "Library" / "LaunchAgents"
    agent_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
    plist_path = agent_dir / (label + ".plist")
    log_file = install_dir / "logs" / "backend.log"
    plist = {
        "Label": label,
        "ProgramArguments": [str(launcher)],
        "WorkingDirectory": str(install_dir),
        "RunAtLoad": True,
        "KeepAlive": {"SuccessfulExit": False},
        "StandardOutPath": str(log_file),
        "StandardErrorPath": str(log_file),
    }
    temporary = plist_path.with_suffix(".plist.tmp")
    with temporary.open("wb") as stream:
        plistlib.dump(plist, stream, sort_keys=True)
        stream.flush()
        os.fsync(stream.fileno())
    os.chmod(temporary, 0o600)
    os.replace(temporary, plist_path)
    domain = f"gui/{uid}"
    run_checked(["launchctl", "bootout", domain + "/" + label], timeout=15)
    bootstrap = run_checked(["launchctl", "bootstrap", domain, plist_path], timeout=20)
    if bootstrap.returncode != 0:
        detail = (bootstrap.stderr or bootstrap.stdout).strip()
        return False, f"Created {plist_path}, but could not load the LaunchAgent: {detail}"
    enable = run_checked(["launchctl", "enable", domain + "/" + label], timeout=15)
    if enable.returncode != 0:
        detail = (enable.stderr or enable.stdout).strip()
        return False, f"Loaded the LaunchAgent but could not enable it: {detail}"
    kickstart = run_checked(["launchctl", "kickstart", "-k", domain + "/" + label], timeout=15)
    if kickstart.returncode != 0:
        detail = (kickstart.stderr or kickstart.stdout).strip()
        return False, f"LaunchAgent was enabled but could not be started: {detail}"
    return True, "Registered and started the per-user macOS LaunchAgent."


def register_backend_service(install_dir, launcher):
    if os.name == "nt":
        return register_windows_backend_service(launcher)
    if sys.platform == "darwin":
        return register_macos_backend_service(install_dir, launcher)
    return register_linux_backend_service(install_dir, launcher)


def local_addresses():
    addresses = set()
    try:
        hostname = socket.gethostname()
        for item in socket.getaddrinfo(hostname, None, family=socket.AF_INET, type=socket.SOCK_STREAM):
            address = item[4][0]
            if not address.startswith("127.") and address != "0.0.0.0":
                addresses.add(address)
    except OSError:
        pass
    return sorted(addresses)


def main():
    parser = argparse.ArgumentParser(
        description="Download and install the matching official RBE backend + Cloud Node runtime."
    )
    parser.add_argument("--install-dir", help="Override the RBE node installation directory.")
    parser.add_argument("--api-port", default="8080", help="Backend API port (default: 8080).")
    parser.add_argument("--node-id", help="Cloud Node identity for this NAS.")
    parser.add_argument("--verify-only", action="store_true", help="Download and verify the matching runtime without installing.")
    parser.add_argument("--skip-admin-init", action="store_true", help="Do not prompt for local RBE administrator initialization.")
    parser.add_argument("--no-service", action="store_true", help="Do not register or start an OS background service.")
    args = parser.parse_args()

    try:
        manifest = load_manifest()
        target = detect_target()
        api_port = validate_port(args.api_port, "Backend API port")
        node_id = validate_node_id(args.node_id or default_node_id())
        requested_install_dir = Path(args.install_dir or default_install_directory()).expanduser()
        if requested_install_dir.is_symlink():
            raise SetupError("Choose a normal, non-symlink installation directory.")
        install_dir = requested_install_dir.resolve()
        if install_dir == Path(install_dir.anchor):
            raise SetupError("Choose a normal, non-root installation directory.")

        say("GitManager — RBE NAS runtime setup")
        say(f"Detected host: {target} · {manifest['targets'][target]['compatibility']}")
        say("This ZIP contains the /api routes and settings templates; platform binaries are downloaded now.")
        if args.verify_only:
            with tempfile.TemporaryDirectory(prefix="gitmanager-rbe-verify-") as temporary:
                release = download_runtime(manifest, target, Path(temporary))
            say("[ OK ] The matching RBE runtime passed both GitHub digest and SHA256SUMS verification.")
            say(json.dumps(release, indent=2))
            return 0

        install_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
        if os.name != "nt":
            os.chmod(install_dir, 0o700)
        if any(install_dir.iterdir()) and not (install_dir / "backend.exe").exists() and not (install_dir / "backend").exists():
            if not ask_yes_no(f"{install_dir} is not empty. Install into this directory without deleting its contents?", False):
                raise SetupError("Setup cancelled; existing files were left untouched.")

        with tempfile.TemporaryDirectory(prefix="gitmanager-rbe-runtime-", dir=str(install_dir)) as temporary:
            stage = Path(temporary)
            release = download_runtime(manifest, target, stage)
            install_runtime(stage, install_dir, target)

        api_count = copy_api_routes(install_dir)
        settings_path, cn_settings_path, key_path, settings, cn_settings = initialize_configuration(
            install_dir, node_id, api_port
        )
        backend_name = "backend.exe" if os.name == "nt" else "backend"
        cloud_node_name = "cloud_node.exe" if os.name == "nt" else "cloud_node"
        backend_path = install_dir / backend_name
        cloud_node_path = install_dir / cloud_node_name
        if not backend_path.is_file() or not cloud_node_path.is_file():
            raise SetupError("The RBE backend or Cloud Node binary was not installed at its expected location.")
        cloud_node_config_status = validate_cloud_node_configuration(
            cloud_node_path, cn_settings_path
        )
        public_key = derive_public_key(cloud_node_path, cn_settings_path, key_path)
        launcher = write_launchers(
            install_dir, backend_path, settings_path, cn_settings_path, key_path
        )
        release_path = install_dir / "rbe-release.json"
        save_json(release_path, release)
        registration = {
            "nodeId": cn_settings["node"]["id"],
            "publicKey": public_key,
            "storageRoot": cn_settings["node"]["storageRoot"],
            "apiHost": settings.get("api", {}).get("host", "0.0.0.0"),
            "apiPort": settings.get("api", {}).get("port", api_port),
            "cloudNodeSettings": cn_settings_path.name,
        }
        save_json(install_dir / "node-registration.json", registration)

        say("")
        say("[ OK ] Full RBE runtime installed and verified.")
        say(f"RBE release: {release['tag']} · {release['buildTag']}")
        say(f"Backend: {backend_path}")
        say(f"Cloud Node: {cloud_node_path}")
        say(f"Container: {install_dir / 'dep' / ('container.exe' if os.name == 'nt' else 'container')}")
        say(f"Service runtime: {install_dir / 'dep' / ('service.exe' if os.name == 'nt' else 'service')}")
        say(f"API routes installed: {api_count} · {install_dir / 'api'}")
        say(f"Backend settings: {settings_path}")
        say(f"Cloud Node settings: {cn_settings_path}")
        say(f"Cloud Node private key: {key_path} (never share this file)")
        say(f"Cloud Node public key: {public_key}")
        if cloud_node_config_status:
            say("Cloud Node config validation:")
            for line in cloud_node_config_status.splitlines()[:12]:
                say(f"  {line}")
        say(f"Node registration: {install_dir / 'node-registration.json'}")
        say(f"Start script: {launcher}")

        admin_initialized = False
        if args.skip_admin_init:
            say("RBE administrator initialization skipped by --skip-admin-init.")
        elif not sys.stdin.isatty():
            say("No interactive terminal is available; administrator initialization was skipped.")
            say("Run backend --init-admin in a terminal before starting the background service.")
        elif ask_yes_no(
            "Initialize or confirm the RBE administrator account now?",
            True,
        ):
            say("RBE will handle the password prompt directly; GitManager does not read or store the password.")
            try:
                admin_initialized = initialize_backend_admin(
                    backend_path, settings_path, install_dir
                )
            except SetupError as error:
                say(f"[ WARNING ] Administrator initialization failed: {error}")
            if admin_initialized:
                say("[ OK ] RBE administrator credentials are initialized or were already configured.")
            else:
                say("[ WARNING ] Administrator initialization did not complete; the backend will not be auto-started.")
        else:
            say("RBE administrator initialization skipped.")

        service_registered = False
        if args.no_service:
            say("OS service registration skipped by --no-service.")
        elif not admin_initialized:
            say("OS service registration skipped until the RBE administrator account is initialized.")
            say("After initializing, rerun setup.py or start the backend manually with the launcher.")
        elif ask_yes_no(
            "Register and start the initialized RBE backend in the background at user login?",
            True,
        ):
            service_ok, service_message = register_backend_service(install_dir, launcher)
            service_registered = service_ok
            say(("[ OK ] " if service_ok else "[ WARNING ] ") + service_message)
        else:
            say("OS service registration skipped. Start the backend manually with the launcher.")
        say("")
        say("RBE status endpoints after the backend starts:")
        say(f"  http://127.0.0.1:{settings.get('api', {}).get('port', api_port)}/health")
        say(f"  http://127.0.0.1:{settings.get('api', {}).get('port', api_port)}/api/health")
        for address in local_addresses():
            say(f"  http://{address}:{settings.get('api', {}).get('port', api_port)}/health")
        say("")
        say("IMPORTANT: the current RBE release uses its CN peer receiver only during backend maintenance/boot recovery.")
        say("A permanent LAN peer-listener/pairing mode is not part of this binary release yet; the normal backend API/status service is installed separately.")
        say("The installer did not open firewall ports, configure TLS, or copy private keys.")
        say("")
        if admin_initialized:
            say("The RBE administrator is initialized. The generated backend service/launcher can now start normally.")
        else:
            say("Before starting the backend, initialize the RBE administrator from a terminal:")
            say(f"  {backend_path} --settings {settings_path} --init-admin")
            say("The background service was deliberately not started before this step.")
            say("After initialization, rerun setup.py to register the OS service, or start the launcher manually.")
        if not service_registered:
            say(f"Backend log: {install_dir / 'logs' / 'backend.log'}")
        return 0
    except (SetupError, OSError, ValueError, json.JSONDecodeError) as error:
        say(f"\n[ERROR] {error}")
        return 1


if __name__ == "__main__":
    sys.exit(main())

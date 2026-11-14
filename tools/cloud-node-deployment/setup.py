#!/usr/bin/env python3
"""Install the RBE Cloud Node binary bundled for this host by GitManager."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import plistlib
import re
import secrets
import shlex
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from urllib.parse import urlsplit

PACKAGE_ROOT = Path(__file__).resolve().parent
MANIFEST_PATH = PACKAGE_ROOT / "manifest.json"
KEY_ENV = "RBE_CLOUD_NODE_PRIVATE_KEY"
NODE_ID_RE = re.compile(r"^[A-Za-z0-9_.-]{1,128}$")
PUBLIC_KEY_RE = re.compile(r"^[0-9a-fA-F]{64}$")


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
    return value in ("y", "yes")


def run_checked(args, *, env=None, timeout=30):
    try:
        return subprocess.run(
            [str(item) for item in args],
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=timeout,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise SetupError(f"Could not run {args[0]}: {error}") from error


def sha256_file(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def safe_manifest_path(raw):
    if not isinstance(raw, str) or not raw or "\\" in raw:
        raise SetupError("Package manifest contains an invalid binary path.")
    relative = Path(raw)
    if relative.is_absolute() or any(part in ("", ".", "..") for part in relative.parts):
        raise SetupError("Package manifest contains an unsafe binary path.")
    candidate = PACKAGE_ROOT / relative
    if candidate.is_symlink():
        raise SetupError("Package manifest binary path must not be a symbolic link.")
    resolved = candidate.resolve()
    try:
        resolved.relative_to(PACKAGE_ROOT)
    except ValueError as error:
        raise SetupError("Package manifest binary path escapes this ZIP.") from error
    if not resolved.is_file():
        raise SetupError(f"Bundled binary is missing or is not a regular file: {raw}")
    return resolved


def load_manifest():
    try:
        manifest = json.loads(MANIFEST_PATH.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise SetupError(f"Could not read package manifest.json: {error}") from error
    if not isinstance(manifest, dict):
        raise SetupError("Package manifest must be a JSON object.")
    if manifest.get("formatVersion") != 1 or manifest.get("kind") != "gitmanager-cloud-node-deployment":
        raise SetupError("Unsupported Cloud Node deployment package format.")
    if not isinstance(manifest.get("targets"), dict):
        raise SetupError("Package manifest has no target map.")
    return manifest


def verify_target_binary(manifest, target):
    entry = manifest["targets"].get(target)
    if not isinstance(entry, dict):
        raise SetupError(f"This ZIP does not contain the required host target: {target}")
    binary = safe_manifest_path(entry.get("path"))
    expected_hash = str(entry.get("sha256", "")).lower()
    if not re.fullmatch(r"[0-9a-f]{64}", expected_hash):
        raise SetupError(f"Invalid SHA-256 in manifest for {target}.")
    expected_size = entry.get("size")
    if not isinstance(expected_size, int) or expected_size <= 0:
        raise SetupError(f"Invalid binary size in manifest for {target}.")
    actual_size = binary.stat().st_size
    if actual_size != expected_size:
        raise SetupError(f"Bundled {target} binary size mismatch ({actual_size} != {expected_size}).")
    if sha256_file(binary) != expected_hash:
        raise SetupError(f"Bundled {target} binary SHA-256 mismatch; refusing to execute it.")
    return binary


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
        raise SetupError(f"Windows architecture {machine!r} is not supported; the current RBE release supplies Windows x64.")
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
                f"This RBE {target} binary requires glibc {baseline[0]}.{baseline[1]} or newer; "
                f"this host reports {libc[0]}.{libc[1]}."
            )
        return target
    raise SetupError(f"Unsupported operating system: {platform.system()}")


def default_install_directory():
    home = Path.home()
    if os.name == "nt":
        root = Path(os.environ.get("LOCALAPPDATA", home / "AppData" / "Local"))
        return root / "kastrick's_software" / "CloudNode"
    if sys.platform == "darwin":
        return home / "Library" / "Application Support" / "Kastrick" / "CloudNode"
    root = Path(os.environ.get("XDG_DATA_HOME", home / ".local" / "share"))
    return root / "kastrick" / "cloud-node"


def default_storage_directory():
    return Path.home() / "CloudNodeStorage"


def validate_node_id(value):
    if not NODE_ID_RE.fullmatch(value) or value in (".", ".."):
        raise SetupError("Node IDs must use 1-128 ASCII letters, digits, dots, underscores, or hyphens.")
    return value


def validate_peer_url(value, label):
    parsed = urlsplit(value)
    if not parsed.hostname or parsed.username or parsed.password or parsed.query or parsed.fragment:
        raise SetupError(f"{label} must be a base URL without credentials, query strings, or fragments.")
    if parsed.scheme == "https":
        return value.rstrip("/")
    if parsed.scheme == "http" and parsed.hostname.lower() in ("localhost", "127.0.0.1", "::1"):
        return value.rstrip("/")
    raise SetupError(f"{label} must use HTTPS (HTTP is allowed only for loopback development).")


def validate_public_key(value, label):
    value = value.strip()
    if not PUBLIC_KEY_RE.fullmatch(value):
        raise SetupError(f"{label} must be exactly 64 hexadecimal characters (an Ed25519 public key).")
    return value.lower()


def secure_write(path, content, *, secret=False):
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    fd, temporary_name = tempfile.mkstemp(prefix=path.name + ".", suffix=".tmp", dir=str(path.parent))
    temporary = Path(temporary_name)
    try:
        if os.name != "nt":
            os.chmod(temporary, 0o600 if secret else 0o644)
        with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as stream:
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        if os.name != "nt":
            os.chmod(path, 0o600 if secret else 0o644)
    except Exception:
        try:
            temporary.unlink()
        except OSError:
            pass
        raise


def save_json(path, value, *, secret=False):
    secure_write(path, json.dumps(value, indent=2, ensure_ascii=False) + "\n", secret=secret)


def powershell_literal(value):
    return "'" + str(value).replace("'", "''") + "'"


def systemd_literal(value):
    return '"' + str(value).replace("\\", "\\\\").replace('"', '\\"').replace("\n", "") + '"'


def shell_launcher_text(binary, config, key_file):
    return (
        "#!/bin/sh\n"
        "set -eu\n"
        f"export {KEY_ENV}={shlex.quote('file:' + str(key_file))}\n"
        f"exec {shlex.quote(str(binary))} {shlex.quote('--config=' + str(config))} run\n"
    )


def powershell_launcher_text(binary, config, key_file, log_file):
    return (
        "$ErrorActionPreference = 'Stop'\n"
        f"$env:{KEY_ENV} = {powershell_literal('file:' + str(key_file))}\n"
        f"& {powershell_literal(binary)} {powershell_literal('--config=' + str(config))} 'run' "
        f"*>> {powershell_literal(log_file)}\n"
        "exit $LASTEXITCODE\n"
    )


def create_service_files(install_dir, binary, config, key_file):
    logs = install_dir / "logs"
    logs.mkdir(parents=True, exist_ok=True, mode=0o700)
    if os.name == "nt":
        launcher = install_dir / "run-cloud-node.ps1"
        secure_write(launcher, powershell_launcher_text(binary, config, key_file, logs / "cloud-node.log"))
        return {"launcher": launcher, "logs": logs}
    launcher = install_dir / "run-cloud-node.sh"
    secure_write(launcher, shell_launcher_text(binary, config, key_file))
    os.chmod(launcher, 0o700)
    return {"launcher": launcher, "logs": logs}


def register_windows_service(files):
    schtasks = shutil.which("schtasks.exe") or shutil.which("schtasks")
    if not schtasks:
        return False, "Task Scheduler CLI was not found. Run the generated run-cloud-node.ps1 manually."
    task_command = (
        "powershell.exe -NoLogo -NoProfile -NonInteractive -WindowStyle Hidden "
        f"-ExecutionPolicy Bypass -File \"{files['launcher']}\""
    )
    create = run_checked([
        schtasks, "/Create", "/SC", "ONLOGON", "/TN", "Kastrick Cloud Node",
        "/TR", task_command, "/RL", "LIMITED", "/F",
    ])
    if create.returncode != 0:
        detail = (create.stderr or create.stdout).strip()
        return False, f"Could not register the user logon task: {detail}"
    start = run_checked([schtasks, "/Run", "/TN", "Kastrick Cloud Node"])
    if start.returncode != 0:
        detail = (start.stderr or start.stdout).strip()
        return False, f"Task was registered but could not be started: {detail}"
    return True, "Registered and started a per-user Task Scheduler task (runs at user logon)."


def register_linux_service(files, install_dir):
    systemctl = shutil.which("systemctl")
    if not systemctl:
        return False, "systemctl was not found. The launcher is ready at " + str(files["launcher"])
    unit_dir = Path.home() / ".config" / "systemd" / "user"
    unit_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
    unit = unit_dir / "cloud-node.service"
    unit_text = (
        "[Unit]\nDescription=Kastrick RBE Cloud Node\n"
        "Wants=network-online.target\nAfter=network-online.target\n\n"
        "[Service]\nType=simple\n"
        f"WorkingDirectory={systemd_literal(install_dir)}\n"
        f"ExecStart={systemd_literal(files['launcher'])}\n"
        "Restart=on-failure\nRestartSec=5\nUMask=0077\nNoNewPrivileges=true\n\n"
        "[Install]\nWantedBy=default.target\n"
    )
    secure_write(unit, unit_text)
    reload = run_checked([systemctl, "--user", "daemon-reload"])
    if reload.returncode != 0:
        return False, (
            f"Created {unit}, but systemd --user is unavailable in this session. "
            "Run: systemctl --user daemon-reload && systemctl --user enable --now cloud-node.service"
        )
    enable = run_checked([systemctl, "--user", "enable", "--now", "cloud-node.service"])
    if enable.returncode != 0:
        detail = (enable.stderr or enable.stdout).strip()
        return False, f"Created {unit}, but could not enable it: {detail}"
    return True, "Registered and started the per-user systemd unit cloud-node.service."


def register_macos_service(files, install_dir, key_file):
    uid = getattr(os, "getuid", lambda: None)()
    if uid is None:
        return False, "Could not determine the current macOS user ID."
    label = "com.kastrick.cloud-node"
    agent_dir = Path.home() / "Library" / "LaunchAgents"
    agent_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
    plist_path = agent_dir / (label + ".plist")
    log_path = files["logs"] / "cloud-node.log"
    plist = {
        "Label": label,
        "ProgramArguments": [str(files["launcher"])],
        "WorkingDirectory": str(install_dir),
        "RunAtLoad": True,
        "KeepAlive": True,
        "EnvironmentVariables": {KEY_ENV: "file:" + str(key_file)},
        "StandardOutPath": str(log_path),
        "StandardErrorPath": str(log_path),
    }
    temporary = plist_path.with_suffix(".plist.tmp")
    with temporary.open("wb") as stream:
        plistlib.dump(plist, stream, sort_keys=True)
    os.chmod(temporary, 0o600)
    os.replace(temporary, plist_path)
    domain = f"gui/{uid}"
    run_checked(["launchctl", "bootout", domain + "/" + label])
    bootstrap = run_checked(["launchctl", "bootstrap", domain, plist_path])
    if bootstrap.returncode != 0:
        detail = (bootstrap.stderr or bootstrap.stdout).strip()
        return False, f"Created {plist_path}, but could not load the LaunchAgent: {detail}"
    enable = run_checked(["launchctl", "enable", domain + "/" + label])
    if enable.returncode != 0:
        detail = (enable.stderr or enable.stdout).strip()
        return False, f"Loaded the LaunchAgent but could not enable it: {detail}"
    return True, "Registered and started the per-user macOS LaunchAgent."


def register_service(files, install_dir, key_file):
    if os.name == "nt":
        return register_windows_service(files)
    if sys.platform == "darwin":
        return register_macos_service(files, install_dir, key_file)
    return register_linux_service(files, install_dir)


def default_node_id():
    host = re.sub(r"[^A-Za-z0-9_.-]+", "-", platform.node().strip().lower()).strip(".-_")
    return ("nas-" + host)[:128] if host else "nas-cloud-node"


def collect_new_settings():
    say("\nRBE backend connection")
    node_id = validate_node_id(ask("This Cloud Node's ID", default_node_id()))
    mode = ask("Node mode (primary/replica/relay/archive)", "primary").lower()
    if mode not in ("primary", "replica", "relay", "archive"):
        raise SetupError("Node mode must be primary, replica, relay, or archive.")
    storage_input = ask("Cloud Node internal storage/backup directory", str(default_storage_directory()))
    storage_root = Path(storage_input).expanduser().resolve()
    watched_default = Path.home() / "CloudNodeData"
    watched_input = ask(
        "Directory on this machine whose files Cloud Node should back up",
        str(watched_default),
    )
    watched_directory = Path(watched_input).expanduser().resolve()
    if (
        storage_root == watched_directory
        or storage_root.is_relative_to(watched_directory)
        or watched_directory.is_relative_to(storage_root)
    ):
        raise SetupError(
            "Keep the watched folder and Cloud Node internal storage in separate, non-overlapping directories."
        )
    say("")
    say("IMPORTANT DATA-SYNC SAFETY:")
    say("Cloud Node scans every regular file under the watched folder and syncs it to the configured RBE backend.")
    say("Symlinks are skipped, but the current RBE localSync scanner has no ignore-file or secret-file exclusion.")
    say("Do not select your home directory, a whole drive, .ssh, credential stores, or folders containing .env/private keys.")
    say("Use a dedicated folder containing only data you intend to replicate to the RBE server.")
    if not ask_yes_no("I understand that regular files in this folder may be transferred to the RBE backend", False):
        raise SetupError("Setup cancelled before creating or starting the Cloud Node.")
    logical_prefix = ask("Logical backup namespace/prefix", "nas-data").replace("\\", "/")
    if (
        not logical_prefix
        or len(logical_prefix) > 512
        or logical_prefix.startswith("/")
        or logical_prefix.endswith("/")
        or ":" in logical_prefix
        or any(part in ("", ".", "..") for part in logical_prefix.split("/"))
        or any(ord(character) < 32 or ord(character) == 127 for character in logical_prefix)
    ):
        raise SetupError("The logical backup namespace must be a relative path without empty, '.', '..', drive or control segments.")
    backend_url = validate_peer_url(ask("RBE backend HTTPS base URL"), "RBE backend URL")
    backend_node_id = validate_node_id(ask("RBE backend Cloud Node ID"))
    backend_public_key = validate_public_key(ask("RBE backend Ed25519 public key"), "RBE backend public key")
    try:
        backup_versions = int(ask("Backup versions to retain", "5"))
    except ValueError as error:
        raise SetupError("Backup versions must be a number.") from error
    if not 1 <= backup_versions <= 1024:
        raise SetupError("Backup versions must be between 1 and 1024.")
    return {
        "formatVersion": 1,
        "node": {
            "id": node_id,
            "mode": mode,
            "storageRoot": str(storage_root),
            "backupVersions": backup_versions,
            "preserveOriginal": True,
            "videoChunkBytes": 4194304,
        },
        "localSync": {
            "directory": str(watched_directory),
            "logicalPrefix": logical_prefix,
        },
        "upstream": {
            "url": backend_url,
            "nodeId": backend_node_id,
            "publicKey": backend_public_key,
            "autoReconnect": True,
            "syncOnConnect": True,
            "reconnectDelayMs": 2000,
            "maxReconnectDelayMs": 60000,
            "pollIntervalMs": 30000,
        },
        "replication": {
            "requireBootRecovery": False,
            "bootRecoveryTimeoutMs": 60000,
            "targets": [],
        },
    }


def derive_public_key(binary, config, key_file):
    env = os.environ.copy()
    env[KEY_ENV] = "file:" + str(key_file.resolve())
    result = run_checked([binary, "--config=" + str(config), "public-key"], env=env, timeout=20)
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip()
        raise SetupError(f"Cloud Node could not derive this node's public key: {detail}")
    value = result.stdout.strip().splitlines()[-1] if result.stdout.strip() else ""
    if not PUBLIC_KEY_RE.fullmatch(value):
        raise SetupError("Cloud Node returned an invalid Ed25519 public key.")
    return value.lower()


def validate_settings_with_binary(binary, config, key_file):
    env = os.environ.copy()
    env[KEY_ENV] = "file:" + str(key_file.resolve())
    result = run_checked([binary, "--config=" + str(config), "evaluate"], env=env, timeout=60)
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip()
        raise SetupError(f"Cloud Node rejected the generated settings: {detail}")


def main():
    parser = argparse.ArgumentParser(description="Install the bundled RBE Cloud Node for this computer.")
    parser.add_argument("--verify-only", action="store_true", help="Verify every bundled platform binary without installing.")
    parser.add_argument("--install-dir", help="Override the per-user installation directory.")
    parser.add_argument("--no-service", action="store_true", help="Install and configure the node without registering an OS service.")
    args = parser.parse_args()

    try:
        manifest = load_manifest()
        say("GitManager — RBE Cloud Node deployment")
        release = manifest.get("rbeRelease", {})
        say(f"RBE release: {release.get('tag', 'unknown')} · {release.get('buildTag', 'build unknown')}")
        if args.verify_only:
            for target in sorted(manifest["targets"]):
                verify_target_binary(manifest, target)
                say(f"[ OK ] {target}")
            say("[ OK ] All bundled Cloud Node binaries match manifest.json.")
            return 0

        target = detect_target()
        binary_source = verify_target_binary(manifest, target)
        say(f"Detected host: {target}")
        install_input = args.install_dir or ask("Install directory", str(default_install_directory()))
        install_dir = Path(install_input).expanduser().resolve()
        config_path = install_dir / "setting.cn.json"
        key_path = install_dir / "cloud-node-private-key.secret"
        binary_name = "cloud_node.exe" if os.name == "nt" else "cloud_node"
        binary_path = install_dir / binary_name

        reuse = False
        if install_dir.exists() and (config_path.exists() or key_path.exists()):
            if not (config_path.is_file() and key_path.is_file()) or config_path.is_symlink() or key_path.is_symlink():
                raise SetupError(
                    f"{install_dir} contains an incomplete or linked Cloud Node install. "
                    "Choose another install directory; this script will not delete or reset it."
                )
            reuse = ask_yes_no(
                "An existing Cloud Node identity/settings were found. Keep them and update only the binary?",
                True,
            )
            if not reuse:
                raise SetupError(
                    "Existing node data was left untouched. Rerun with a different --install-dir to create another node."
                )
        elif install_dir.exists() and any(install_dir.iterdir()):
            raise SetupError(f"Install directory {install_dir} is not empty. Choose a fresh directory; existing files are never removed.")

        install_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
        if os.name != "nt":
            os.chmod(install_dir, 0o700)
        binary_fd, temporary_name = tempfile.mkstemp(
            prefix=binary_path.name + ".",
            suffix=".new",
            dir=str(install_dir),
        )
        temporary_binary = Path(temporary_name)
        try:
            with os.fdopen(binary_fd, "wb") as output, binary_source.open("rb") as source:
                shutil.copyfileobj(source, output, length=1024 * 1024)
                output.flush()
                os.fsync(output.fileno())
            os.replace(temporary_binary, binary_path)
        finally:
            if temporary_binary.exists():
                temporary_binary.unlink()
        if os.name != "nt":
            os.chmod(binary_path, 0o755)

        if reuse:
            try:
                config = json.loads(config_path.read_text(encoding="utf-8"))
            except (OSError, json.JSONDecodeError) as error:
                raise SetupError(f"Could not read existing Cloud Node settings: {error}") from error
            if not isinstance(config.get("node"), dict) or not isinstance(config.get("upstream"), dict):
                raise SetupError("Existing settings are not an RBE peer-mode Cloud Node configuration.")
            if not isinstance(config.get("localSync"), dict):
                raise SetupError("Existing settings do not include a watched localSync directory.")
            node_id = validate_node_id(str(config["node"].get("id", "")))
            say(f"Keeping existing Cloud Node ID: {node_id}")
        else:
            config = collect_new_settings()
            node_id = config["node"]["id"]
            key_value = secrets.token_hex(32)
            secure_write(key_path, key_value + "\n", secret=True)
            save_json(config_path, config)
            Path(config["node"]["storageRoot"]).expanduser().mkdir(parents=True, exist_ok=True)

        public_key = derive_public_key(binary_path, config_path, key_path)
        validate_settings_with_binary(binary_path, config_path, key_path)

        old_registration = {}
        registration_path = install_dir / "rbe-peer-registration.json"
        if registration_path.is_file():
            try:
                old_registration = json.loads(registration_path.read_text(encoding="utf-8"))
            except (OSError, json.JSONDecodeError):
                old_registration = {}
        default_peer_url = old_registration.get("url") or f"https://{node_id}.invalid"
        say("\nRBE backend peer registration")
        say("The RBE backend must trust this node's public key before authentication will succeed.")
        advertised_url = validate_peer_url(
            ask("Peer URL metadata (HTTPS; current inbound recovery does not dial it)", default_peer_url),
            "Peer URL",
        )
        durable = ask_yes_no(
            "Is this node backed by persistent storage?",
            bool(old_registration.get("durable", True)),
        )
        registration = {
            "nodeId": node_id,
            "url": advertised_url,
            "publicKey": public_key,
            "durable": durable,
        }
        save_json(registration_path, registration)
        instructions = (
            "# Register this Cloud Node on the RBE backend\n\n"
            "Add the following object to the RBE backend's setting.cn.json under "
            "replication.targets. Preserve existing targets.\n\n"
            "JSON registration object:\n\n"
            + json.dumps(registration, indent=2)
            + "\n\n"
            "The backend must also have its own RBE_CLOUD_NODE_PRIVATE_KEY configured. "
            "Its matching public key is the upstream.publicKey in this node's setting.cn.json. "
            "Do not copy either machine's private key to the other machine.\n"
        )
        secure_write(install_dir / "RBE-SERVER-PEER-REGISTRATION.md", instructions)

        say("\n[ OK ] Cloud Node binary installed and verified.")
        say(f"Settings: {config_path}")
        say(f"Cloud Node internal storage: {config['node']['storageRoot']}")
        say(f"Watched folder: {config['localSync']['directory']}")
        say(f"Logical backup prefix: {config['localSync']['logicalPrefix']}")
        say(f"Private key: {key_path} (never copy this file to the RBE backend)")
        say(f"Public key:  {public_key}")
        say(f"Peer registration: {registration_path}")
        say("\nRegister the public peer object on the RBE backend before expecting sync to authenticate.")

        if args.no_service:
            say("Service registration skipped by --no-service.")
            return 0
        files = create_service_files(install_dir, binary_path, config_path, key_path)
        if ask_yes_no(
            "Register and start the Cloud Node background service now? It will retry until the RBE backend trusts this peer.",
            True,
        ):
            ok, message = register_service(files, install_dir, key_path)
            say(("[ OK ] " if ok else "[WARN] ") + message)
            if not ok:
                say(f"Manual launcher: {files['launcher']}")
        else:
            say(f"Service files prepared but not enabled. Manual launcher: {files['launcher']}")

        if sys.platform == "darwin":
            say("Note: the upstream macOS binary is not Apple-notarized. This installer does not bypass Gatekeeper.")
        say("\nCloud Node setup completed. Existing storage and configuration were not deleted.")
        return 0
    except (SetupError, OSError, ValueError) as error:
        say(f"\n[ERROR] {error}")
        return 1


if __name__ == "__main__":
    sys.exit(main())

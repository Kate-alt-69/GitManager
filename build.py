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
import queue
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import zipfile
from pathlib import Path
from typing import Dict, Optional, Tuple, List

try:
    from tools.build_targets import BuildTarget, can_build_locally, host_platform, parse_build_selection, mpdev_platform_name
except ModuleNotFoundError:
    from build_targets import BuildTarget, can_build_locally, host_platform, parse_build_selection, mpdev_platform_name

try:
    from tools.setup_build_environment import setup_build_environment
except ModuleNotFoundError:
    from setup_build_environment import setup_build_environment

RECOMMENDED_RUST_VERSION = (1, 88, 0)
RECOMMENDED_TAURI_CRATE_VERSION = (2, 0, 0)


_ANSI_COLORS = {
    "white": "\033[97m",
    "green": "\033[92m",
    "yellow": "\033[93m",
    "red": "\033[91m",
    "gray": "\033[90m",
}
_ANSI_RESET = "\033[0m"
VERBOSE_LOGGING = os.environ.get("GITMANAGER_BUILD_DEBUG", "").strip().lower() in ("1", "true", "yes", "on")


def _supports_color(stream) -> bool:
    if os.environ.get("NO_COLOR") is not None:
        return False
    if os.environ.get("FORCE_COLOR", "").strip().lower() in ("1", "true", "yes", "on"):
        return True
    try:
        return bool(stream.isatty()) and os.environ.get("TERM", "").lower() != "dumb"
    except (AttributeError, OSError):
        return False


def _paint(message: str, color: str, stream) -> str:
    if not _supports_color(stream):
        return message
    return _ANSI_COLORS[color] + message + _ANSI_RESET


_PROGRESS_LOCK = threading.RLock()
_ACTIVE_PROGRESS_STREAM = None
_ACTIVE_PROGRESS_WORK = None
_ACTIVE_PROGRESS_WIDTH = 0
_LAST_NONINTERACTIVE_PROGRESS = {}


def _supports_inline_progress(stream) -> bool:
    """Return true only when carriage-return updates can be shown in a real terminal."""
    try:
        return bool(stream.isatty()) and os.environ.get("TERM", "").lower() != "dumb"
    except (AttributeError, OSError):
        return False


def _finish_progress_line_locked() -> None:
    global _ACTIVE_PROGRESS_STREAM, _ACTIVE_PROGRESS_WORK, _ACTIVE_PROGRESS_WIDTH
    if _ACTIVE_PROGRESS_STREAM is not None:
        _ACTIVE_PROGRESS_STREAM.write("\n")
        _ACTIVE_PROGRESS_STREAM.flush()
    _ACTIVE_PROGRESS_STREAM = None
    _ACTIVE_PROGRESS_WORK = None
    _ACTIVE_PROGRESS_WIDTH = 0


def _emit(message: str, *, color: str = "white", stream=None) -> None:
    target = stream or sys.stdout
    with _PROGRESS_LOCK:
        # Any ordinary log or child-process output gets its own clean line.
        _finish_progress_line_locked()
        target.write(_paint(message, color, target) + "\\n")
        target.flush()


def log_info(msg: str) -> None:
    _emit(f"[INFO] {msg}", color="white")


def log_ok(msg: str) -> None:
    _emit(f"[ OK ] {msg}", color="green")


def log_warn(msg: str) -> None:
    _emit(f"[WARN] {msg}", color="yellow")


def log_err(msg: str) -> None:
    _emit(f"[ERR ] {msg}", color="red", stream=sys.stderr)


def log_debug(msg: str) -> None:
    if VERBOSE_LOGGING:
        _emit(f"[DBG ] {msg}", color="gray")


def log_work(work: str, progress: str, status: str, message: str) -> None:
    """Update one live terminal row; use de-duplicated lines in non-interactive logs."""
    global _ACTIVE_PROGRESS_STREAM, _ACTIVE_PROGRESS_WORK, _ACTIVE_PROGRESS_WIDTH
    normalized = status.strip().upper()
    colors = {
        "PENDING": "yellow",
        "WORKING": "white",
        "DONE": "green",
        "FAILED": "red",
        "CANCELLED": "red",
        "DEBUG": "gray",
        "SKIPPED": "gray",
    }
    color = colors.get(normalized, "white")
    target = sys.stdout
    rendered = f"[ {work} ] [{progress}] [{normalized}] {message}"
    terminal = _supports_inline_progress(target)

    with _PROGRESS_LOCK:
        if not terminal:
            previous = _LAST_NONINTERACTIVE_PROGRESS.get(work)
            # Heartbeats with an unchanged stage are useful on a TTY but become
            # repetitive log spam in CI and redirected output. Keep stage changes
            # and real percentage/step updates, while dropping identical repeats.
            if normalized == "WORKING" and previous == (normalized, progress):
                return
            _LAST_NONINTERACTIVE_PROGRESS[work] = (normalized, progress)
            _finish_progress_line_locked()
            target.write(_paint(rendered, color, target) + "\\n")
            target.flush()
        else:
            if _ACTIVE_PROGRESS_STREAM is not None and _ACTIVE_PROGRESS_WORK != work:
                _finish_progress_line_locked()

            try:
                columns = shutil.get_terminal_size(fallback=(120, 24)).columns
            except OSError:
                columns = 120
            max_width = max(40, columns - 1)
            if len(rendered) > max_width:
                rendered = rendered[:max_width - 1] + "…"
            padding = " " * max(0, _ACTIVE_PROGRESS_WIDTH - len(rendered))

            target.write("\\r" + _paint(rendered, color, target) + padding)
            target.flush()

            if normalized in {"DONE", "FAILED", "CANCELLED", "SKIPPED"}:
                target.write("\n")
                target.flush()
                _ACTIVE_PROGRESS_STREAM = None
                _ACTIVE_PROGRESS_WORK = None
                _ACTIVE_PROGRESS_WIDTH = 0
            else:
                _ACTIVE_PROGRESS_STREAM = target
                _ACTIVE_PROGRESS_WORK = work
                _ACTIVE_PROGRESS_WIDTH = len(rendered)



def run_command(
    command: List[str],
    cwd: Optional[Path] = None,
    env: Optional[Dict[str, str]] = None,
    *,
    work: Optional[str] = None,
    progress: str = "1/1",
    heartbeat_seconds: float = 5.0,
) -> int:
    """Run a build command with live, prefixed stdout/stderr and periodic heartbeats."""
    args = [str(part) for part in command]
    label = work or (Path(args[0]).name if args else "Command")
    if not args:
        log_work(label, progress, "FAILED", "Refusing to run an empty command.")
        return 2

    log_work(label, progress, "PENDING", "Queued command.")
    log_work(label, progress, "WORKING", "Starting process; output will stream below.")
    log_debug(f"Command: {' '.join(args)}")
    if cwd is not None:
        log_debug(f"Working directory: {cwd}")

    started_at = time.monotonic()
    try:
        process = subprocess.Popen(
            args,
            cwd=str(cwd) if cwd else None,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            bufsize=1,
        )
    except OSError as error:
        message = f"Could not start {label}: {error}"
        log_work(label, progress, "FAILED", message)
        log_err(message)
        return 127

    output_queue = queue.Queue()

    def forward_output(channel: str, stream) -> None:
        try:
            for raw_line in iter(stream.readline, ""):
                output_queue.put((channel, raw_line.rstrip("\r\n")))
        finally:
            stream.close()
            output_queue.put((channel, None))

    readers = [
        threading.Thread(target=forward_output, args=("stdout", process.stdout), name="gitmanager-build-stdout", daemon=True),
        threading.Thread(target=forward_output, args=("stderr", process.stderr), name="gitmanager-build-stderr", daemon=True),
    ]
    for reader in readers:
        reader.start()

    remaining_streams = {"stdout", "stderr"}
    heartbeat_seconds = max(float(heartbeat_seconds), 0.25)
    try:
        while remaining_streams:
            try:
                channel, line = output_queue.get(timeout=heartbeat_seconds)
            except queue.Empty:
                elapsed = int(time.monotonic() - started_at)
                log_work(
                    label, progress, "WORKING",
                    f"Process is still running ({elapsed}s elapsed); waiting for tool output.",
                )
                continue
            if line is None:
                remaining_streams.discard(channel)
                continue
            if channel == "stderr":
                _emit(f"[ {label} ] [ERR ] {line}", color="red", stream=sys.stderr)
            else:
                _emit(f"[ {label} ] {line}", color="white")
        return_code = process.wait()
    except KeyboardInterrupt:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        log_work(label, progress, "CANCELLED", "Process stopped after user cancellation.")
        raise
    finally:
        for reader in readers:
            reader.join(timeout=1.0)

    elapsed = max(time.monotonic() - started_at, 0.001)
    if return_code == 0:
        log_work(label, progress, "DONE", f"Process completed successfully in {elapsed:.1f}s.")
    else:
        log_work(label, progress, "FAILED", f"Process exited with code {return_code} after {elapsed:.1f}s.")
    return return_code


def _download_response_to_file(response, destination: Path, work: str = "CPython download") -> int:
    """Stream a download to disk and report byte progress instead of silently waiting."""
    raw_length = response.headers.get("Content-Length", "")
    try:
        total_bytes = int(raw_length) if raw_length else 0
    except (TypeError, ValueError):
        total_bytes = 0

    downloaded = 0
    last_percentage_bucket = -10
    last_reported_bytes = 0
    started_at = time.monotonic()
    last_reported_at = started_at
    chunk_size = 256 * 1024

    detail = "Waiting for the first response bytes..."
    if total_bytes:
        detail = f"Receiving {total_bytes / (1024 * 1024):.1f} MiB from python.org..."
    log_work(work, "0%", "WORKING", detail)

    with destination.open("wb") as output:
        while True:
            chunk = response.read(chunk_size)
            if not chunk:
                break
            output.write(chunk)
            downloaded += len(chunk)

            now = time.monotonic()
            if total_bytes:
                percentage = min(100, int(downloaded * 100 / total_bytes))
                bucket = (percentage // 10) * 10
                should_report = bucket >= last_percentage_bucket + 10 or percentage == 100
                progress = f"{percentage}%"
                last_percentage_bucket = max(last_percentage_bucket, bucket)
                status_detail = (
                    f"{downloaded / (1024 * 1024):.1f} / "
                    f"{total_bytes / (1024 * 1024):.1f} MiB"
                )
            else:
                should_report = (
                    downloaded - last_reported_bytes >= 1024 * 1024
                    or now - last_reported_at >= 2.0
                )
                progress = f"{downloaded / (1024 * 1024):.1f} MiB"
                status_detail = "received (server did not provide Content-Length)"

            if should_report:
                log_work(work, progress, "WORKING", status_detail)
                last_reported_bytes = downloaded
                last_reported_at = now

    if total_bytes and downloaded != total_bytes:
        raise OSError(
            f"Download ended early: received {downloaded:,} of {total_bytes:,} bytes."
        )

    elapsed = max(time.monotonic() - started_at, 0.001)
    log_work(
        work,
        "100%",
        "DONE",
        f"Downloaded {downloaded:,} bytes ({downloaded / (1024 * 1024):.1f} MiB).",
    )
    log_debug(f"{work} transfer completed in approximately {elapsed:.1f}s.")
    return downloaded


def _run_with_work_heartbeat(work: str, progress: str, message: str, operation, interval_seconds: float = 5.0) -> None:
    """Keep a visible WORKING status while a blocking network operation waits."""
    stop = threading.Event()
    started_at = time.monotonic()

    def report_heartbeat() -> None:
        while not stop.wait(interval_seconds):
            elapsed = int(time.monotonic() - started_at)
            log_work(
                work,
                progress,
                "WORKING",
                f"{message} Still active ({elapsed}s elapsed; waiting for the next response).",
            )

    worker = threading.Thread(
        target=report_heartbeat,
        name="gitmanager-build-progress",
        daemon=True,
    )
    worker.start()
    try:
        return operation()
    finally:
        stop.set()
        worker.join(timeout=max(interval_seconds, 1.0))


PYTHON_EMBED_VERSION = "3.14.10"
PYTHON_EMBED_BASE_URL = "https://www.python.org/ftp/python/3.14.10/"
PYTHON_EMBED_PACKAGES = {
    "x86_64": ("python-3.14.10-embed-amd64.zip", "90905e64a840557d8c1bc4f5b74db3d7ee85e3ffb217611cac1c7813acea4159", 0x8664),
    "x86": ("python-3.14.10-embed-win32.zip", "11a28e8c00a6d8f56b0a6164d52546133a9ed015a5b33e04536917e4fd6e306c", 0x014C),
    "aarch64": ("python-3.14.10-embed-arm64.zip", "bbe116bb1248820359cbf25127ac5c7794ea161869d83686cfbc9a45203ff836", 0xAA64),
}


def windows_target_architecture() -> str:
    triple = os.environ.get("CARGO_BUILD_TARGET", "").strip()
    if "aarch64" in triple:
        return "aarch64"
    if triple.startswith(("i686-", "i586-")):
        return "x86"
    if triple.startswith("x86_64-"):
        return "x86_64"
    machine = platform.machine().lower()
    return {
        "amd64": "x86_64", "x86_64": "x86_64",
        "x86": "x86", "i386": "x86", "i686": "x86",
        "arm64": "aarch64", "aarch64": "aarch64",
    }.get(machine, "x86_64")


def windows_pe_machine(path: Path) -> Optional[int]:
    """Read the PE machine field without executing a foreign-architecture binary."""
    try:
        with path.open("rb") as stream:
            if stream.read(2) != b"MZ":
                return None
            stream.seek(0x3C)
            offset_bytes = stream.read(4)
            if len(offset_bytes) != 4:
                return None
            stream.seek(int.from_bytes(offset_bytes, "little"))
            if stream.read(4) != b"PE\0\0":
                return None
            machine = stream.read(2)
            return int.from_bytes(machine, "little") if len(machine) == 2 else None
    except OSError:
        return None


def prepare_windows_python_runtime(project_root: Path) -> int:
    """Fetch and verify the isolated CPython runtime matching the Windows app architecture."""
    if platform.system() != "Windows":
        log_info("Windows CPython runtime preparation runs only on a Windows build runner; skipping.")
        return 0

    log_work("CPython runtime", "1/6", "WORKING", "Resolving the Windows target architecture...")
    target_arch = windows_target_architecture()
    package = PYTHON_EMBED_PACKAGES.get(target_arch)
    if package is None:
        message = f"No pinned Windows Python runtime is available for target architecture {target_arch}."
        log_work("CPython runtime", "1/6", "FAILED", message)
        log_err(message)
        return 1

    archive_name, expected_sha256, expected_machine = package
    runtime_dir = project_root / "tools" / "runtime" / "python"
    python_exe = runtime_dir / "python.exe"

    machine = platform.machine().lower()
    host_arch = {
        "amd64": "x86_64", "x86_64": "x86_64",
        "x86": "x86", "i386": "x86", "i686": "x86",
        "arm64": "aarch64", "aarch64": "aarch64",
    }.get(machine, "")

    target_triple = os.environ.get("CARGO_BUILD_TARGET", "").strip() or "(native target)"
    log_debug(
        f"Windows host={host_arch or machine}; target={target_arch}; "
        f"Rust target={target_triple}; runtime directory={runtime_dir}."
    )

    log_work("CPython runtime", "2/6", "WORKING", "Checking whether a verified private runtime can be reused...")
    if python_exe.is_file() and windows_pe_machine(python_exe) == expected_machine:
        if host_arch == target_arch or (host_arch == "x86_64" and target_arch == "x86"):
            code, out, err = run_capture([
                str(python_exe), "-I", "-c",
                "import sys; print('.'.join(map(str, sys.version_info[:3]))); raise SystemExit(0 if sys.version_info >= (3, 10) else 1)"
            ])
            if code == 0 and out.startswith(PYTHON_EMBED_VERSION):
                log_work("CPython runtime", "2/6", "DONE", f"Reusing prepared CPython {out} ({target_arch}).")
                return 0
            log_debug(f"Existing Python runtime failed its reuse check: {err or out or code}")
        elif all((runtime_dir / name).is_file() for name in ("python314.zip", "python314._pth")):
            log_work(
                "CPython runtime", "2/6", "DONE",
                f"Reusing verified {PYTHON_EMBED_VERSION} PE runtime for {target_arch}; foreign binaries are not executed.",
            )
            return 0

    log_work("CPython runtime", "2/6", "DONE", "No reusable target runtime found; a fresh pinned package is required.")

    with tempfile.TemporaryDirectory(prefix="gitmanager-python-") as temporary:
        archive_path = Path(temporary) / "python-embed.zip"
        archive_url = PYTHON_EMBED_BASE_URL + archive_name
        request = urllib.request.Request(
            archive_url,
            headers={"User-Agent": "GitManager-Build/1.0"},
        )
        log_work(
            "CPython download", "3/6", "PENDING",
            f"Queued {archive_name} from python.org.",
        )
        log_work(
            "CPython download", "3/6", "WORKING",
            "Opening the HTTPS connection; request timeout is 90 seconds.",
        )
        log_debug(f"CPython download URL: {archive_url}")

        def download_package() -> None:
            with urllib.request.urlopen(request, timeout=90) as response:
                log_debug(
                    f"python.org response status={getattr(response, 'status', 'unknown')}; "
                    f"Content-Length={response.headers.get('Content-Length', 'unknown')}."
                )
                _download_response_to_file(response, archive_path)

        try:
            _run_with_work_heartbeat(
                "CPython download",
                "3/6",
                "Connecting to python.org or waiting for more download bytes.",
                download_package,
            )
        except Exception as exc:
            message = f"Could not download the official Python embeddable package: {exc}"
            log_work("CPython download", "3/6", "FAILED", message)
            log_err(message)
            return 1

        log_work("CPython checksum", "4/6", "WORKING", "Computing SHA-256 for the downloaded archive...")
        digest = hashlib.sha256(archive_path.read_bytes()).hexdigest()
        if digest.lower() != expected_sha256:
            message = (
                "Python embeddable package checksum did not match the pinned python.org SHA-256. "
                f"Expected {expected_sha256}, received {digest}."
            )
            log_work("CPython checksum", "4/6", "FAILED", message)
            log_err(message)
            return 1
        log_work("CPython checksum", "4/6", "DONE", "Pinned SHA-256 verified.")

        log_work("CPython extraction", "5/6", "WORKING", "Validating ZIP entries before touching the runtime directory...")
        try:
            with zipfile.ZipFile(archive_path) as archive:
                members = archive.infolist()
                root = runtime_dir.resolve()
                for member in members:
                    target = (runtime_dir / member.filename).resolve()
                    try:
                        target.relative_to(root)
                    except ValueError:
                        message = f"Python runtime archive contains an unsafe path: {member.filename}"
                        log_work("CPython extraction", "5/6", "FAILED", message)
                        log_err(message)
                        return 1

                runtime_dir.mkdir(parents=True, exist_ok=True)
                existing_entries = list(runtime_dir.iterdir())
                log_debug(f"Replacing {len(existing_entries)} existing entries in the private runtime directory.")
                for existing in existing_entries:
                    if existing.name == ".keep":
                        continue
                    if existing.is_dir() and not existing.is_symlink():
                        shutil.rmtree(existing)
                    else:
                        existing.unlink()

                total_members = max(len(members), 1)
                last_extraction_bucket = -10
                for index, member in enumerate(members, start=1):
                    archive.extract(member, runtime_dir)
                    percentage = int(index * 100 / total_members)
                    bucket = (percentage // 10) * 10
                    if bucket >= last_extraction_bucket + 10 or index == total_members:
                        log_work(
                            "CPython extraction",
                            f"{percentage}%",
                            "WORKING",
                            f"Extracted {index} of {len(members)} archive entries.",
                        )
                        last_extraction_bucket = max(last_extraction_bucket, bucket)
        except (OSError, zipfile.BadZipFile, RuntimeError) as exc:
            message = f"Could not extract the verified Python runtime: {exc}"
            log_work("CPython extraction", "5/6", "FAILED", message)
            log_err(message)
            return 1
    log_work("CPython extraction", "5/6", "DONE", "Runtime archive extracted.")

    log_work("CPython verification", "6/6", "WORKING", "Checking required files and PE architecture...")
    required = [python_exe, runtime_dir / "python314.zip", runtime_dir / "python314._pth"]
    missing = [path.name for path in required if not path.is_file()]
    if missing:
        message = "Python runtime is incomplete; missing: " + ", ".join(missing)
        log_work("CPython verification", "6/6", "FAILED", message)
        log_err(message)
        return 1
    if windows_pe_machine(python_exe) != expected_machine:
        message = f"The bundled Python executable is not the expected {target_arch} PE architecture."
        log_work("CPython verification", "6/6", "FAILED", message)
        log_err(message)
        return 1

    if host_arch == target_arch or (host_arch == "x86_64" and target_arch == "x86"):
        code, out, err = run_capture([
            str(python_exe), "-I", "-c",
            "import sys; print('.'.join(map(str, sys.version_info[:3]))); raise SystemExit(0 if sys.version_info >= (3, 10) else 1)"
        ])
        if code != 0 or not out.startswith(PYTHON_EMBED_VERSION):
            message = f"Bundled Python failed its isolated startup check: {err or out or code}"
            log_work("CPython verification", "6/6", "FAILED", message)
            log_err(message)
            return 1
        version_label = out
    else:
        log_debug("Target Python is foreign to this host; runtime will be validated by archive hash and PE headers only.")
        version_label = PYTHON_EMBED_VERSION

    runtime_bytes = sum(path.stat().st_size for path in runtime_dir.iterdir() if path.is_file())
    log_work(
        "CPython verification", "6/6", "DONE",
        f"Verified python.org CPython {version_label} ({target_arch}); {runtime_bytes:,} bytes extracted.",
    )
    return 0


def cargo_target_root(project_root: Path) -> Path:
    value = Path(os.environ.get("CARGO_TARGET_DIR", project_root / "build" / ".rcache"))
    return value if value.is_absolute() else project_root / value


def stage_windows_mpdev_package(project_root: Path, toolchain_arg: str, release: bool = True) -> int:
    """Stage the raw Windows app and every runtime/sidecar for MPDEV packaging."""
    if platform.system() != "Windows":
        return 0

    target_dir = cargo_target_root(project_root)
    profile_dir = target_profile_dir(project_root, release=release)
    main_exe = profile_dir / "gitmanager.exe"
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
        "gitmanager.service.exe": sidecar_dir / f"gitmanager.service-{triple}.exe",
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
        shutil.copy2(main_exe, staging / "gitmanager.exe")
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
            "gitmanager.exe",
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
    log_info("Included gitmanager.exe, crash_helper.exe, background service, gitm, setup helper, cleaner and private CPython.")
    return 0



def target_profile_dir(project_root: Path, release: bool = True) -> Path:
    root = cargo_target_root(project_root)
    triple = os.environ.get("CARGO_BUILD_TARGET", "").strip()
    profile = "release" if release else "debug"
    return root / triple / profile if triple else root / profile


def name_windows_setup_installer(project_root: Path, release: bool = True) -> Optional[Path]:
    """Expose a stable, predictable setup.exe name after Tauri has bundled NSIS."""
    if platform.system() != "Windows":
        return None
    output_dir = target_profile_dir(project_root, release) / "bundle" / "nsis"
    if not output_dir.is_dir():
        log_err(f"NSIS installer output directory does not exist: {output_dir}")
        return None
    candidates = sorted(
        path for path in output_dir.glob("*-setup.exe")
        if path.name.lower() != "gitmanager-setup.exe"
    )
    if not candidates:
        existing = output_dir / "GitManager-Setup.exe"
        if existing.is_file() and existing.stat().st_size > 0:
            return existing
        log_err("Tauri did not create an NSIS *-setup.exe. The Windows installer is missing.")
        return None
    source = candidates[0]
    destination = output_dir / "GitManager-Setup.exe"
    try:
        shutil.copy2(source, destination)
    except OSError as exc:
        log_err(f"Could not create GitManager-Setup.exe: {exc}")
        return None
    if not destination.is_file() or destination.stat().st_size < 1024 * 1024:
        log_err("GitManager-Setup.exe is missing or unexpectedly small after bundling.")
        return None
    log_ok(f"Created Windows installer: {destination} ({destination.stat().st_size:,} bytes)")
    return destination


WINDOWS_PACKAGING_WINGET_PACKAGES = (
    ("NSIS fallback installer", "NSIS.NSIS"),
    ("Master Packager Dev (required MSI/MSIX packager)", "MasterPackager.MasterPackagerDev"),
)


def windows_packaging_search_directories() -> List[Path]:
    """Return likely install locations for NSIS and Master Packager Dev."""
    roots = []
    for env_name in ("ProgramFiles(x86)", "ProgramFiles"):
        value = os.environ.get(env_name)
        if value:
            roots.append(Path(value))
    local_app_data = os.environ.get("LOCALAPPDATA")
    if local_app_data:
        roots.append(Path(local_app_data) / "Programs")

    directories = []
    for root in roots:
        directories.extend([
            root / "NSIS",
            root / "NSIS" / "Bin",
            root / "Master Packager Ltd" / "Master Packager Dev",
            root / "Master Packager" / "Master Packager Dev",
            root / "Master Packager Dev",
        ])
    if local_app_data:
        directories.append(Path(local_app_data) / "Microsoft" / "WinGet" / "Links")
    return directories


def refresh_windows_packaging_path() -> None:
    """Refresh this Python process PATH after WinGet installs tools."""
    current = os.environ.get("PATH", "").split(os.pathsep)
    discovered = [str(path) for path in windows_packaging_search_directories() if path.is_dir()]
    os.environ["PATH"] = os.pathsep.join(dict.fromkeys(discovered + current))
    for directory in discovered:
        if (Path(directory) / "makensis.exe").is_file():
            os.environ["NSISDIR"] = directory
            break


def find_master_packager_dev_cli() -> Optional[Path]:
    """Find mpdev even if WinGet added its install directory after this shell started."""
    refresh_windows_packaging_path()
    discovered = shutil.which("mpdev") or shutil.which("mpdev.exe")
    if discovered:
        return Path(discovered)
    for directory in windows_packaging_search_directories():
        for name in ("mpdev.exe", "mpdev"):
            candidate = directory / name
            if candidate.is_file():
                return candidate
    return None


def setup_windows_packaging_tools() -> int:
    """Install the required MPDEV packager and NSIS fallback using exact WinGet package IDs."""
    if platform.system() != "Windows":
        log_err("Windows packaging tools can only be installed on Windows.")
        return 1
    winget = shutil.which("winget")
    if not winget:
        log_err(
            "WinGet was not found. Install or update Microsoft's App Installer, then run "
            "py build.py setup-packaging-tools again."
        )
        return 1

    for package_index, (display_name, package_id) in enumerate(WINDOWS_PACKAGING_WINGET_PACKAGES, start=1):
        code, output, _ = run_capture([
            winget, "list", "--id", package_id, "--exact",
            "--accept-source-agreements",
        ])
        if code == 0 and package_id.lower() in output.lower():
            log_ok(f"{display_name} is already installed ({package_id}).")
            continue
        log_info(f"Installing {display_name} through WinGet ({package_id})...")
        command = [
            winget, "install", "--id", package_id, "--exact", "--source", "winget",
            "--silent", "--accept-package-agreements", "--accept-source-agreements",
            "--disable-interactivity",
        ]
        result = run_command(command, work=f"Install {display_name}", progress=f"{package_index}/{len(WINDOWS_PACKAGING_WINGET_PACKAGES)}")
        if result != 0:
            log_err(f"WinGet could not install {display_name} (exit code {result}).")
            return result

    refresh_windows_packaging_path()
    nsis_dirs = [
        directory for directory in windows_packaging_search_directories()
        if (directory / "makensis.exe").is_file()
    ]
    mpdev = find_master_packager_dev_cli()
    missing = []
    if not nsis_dirs:
        missing.append("NSIS (makensis.exe)")
    if mpdev is None:
        missing.append("Master Packager Dev (mpdev.exe)")
    if missing:
        log_err(
            "WinGet completed, but these packaging tools could not be located: "
            + ", ".join(missing)
            + ". Open a new terminal or repair the corresponding WinGet package."
        )
        return 1

    code, version, error = run_capture([str(mpdev), "version"])
    if code != 0:
        log_err(f"Master Packager Dev could not report its version: {error or version or code}")
        return 1
    log_ok("NSIS fallback and required Master Packager Dev are installed and discoverable.")
    log_info(f"Master Packager Dev: {version}")
    return 0


def verify_windows_packaging_inputs(project_root: Path) -> int:
    """Fail before Tauri bundling if the Python resource or installer hook is misconfigured."""
    runtime_dir = project_root / "tools" / "runtime" / "python"
    required_runtime = ("python.exe", "python314.zip", "python314._pth")
    missing = [name for name in required_runtime if not (runtime_dir / name).is_file()]
    if missing:
        log_err(
            "The private Python runtime is incomplete before packaging; missing: "
            + ", ".join(missing)
        )
        return 1

    cleaner = project_root / "tools" / "gitmanager_cleaner.py"
    if not cleaner.is_file():
        log_err(f"GitManager Cleaner source is missing: {cleaner}")
        return 1

    try:
        main_config = json.loads(
            (project_root / "src-tauri" / "tauri.conf.json").read_text(encoding="utf-8")
        )
        windows_config = json.loads(
            (project_root / "src-tauri" / "tauri.windows.conf.json").read_text(encoding="utf-8")
        )
        hooks = (project_root / "src-tauri" / "windows" / "hooks.nsh").read_text(encoding="utf-8")
        nsis_template = (project_root / "src-tauri" / "windows" / "installer.nsi").read_text(encoding="utf-8")
    except (OSError, json.JSONDecodeError) as error:
        log_err(f"Could not verify Windows installer configuration: {error}")
        return 1

    main_resources = main_config.get("bundle", {}).get("resources", {})
    if main_resources.get("../tools/gitmanager_cleaner.py") != "tools/gitmanager_cleaner.py":
        log_err("The shared Tauri configuration must map the GitManager Cleaner resource.")
        return 1

    resources = windows_config.get("bundle", {}).get("resources", {})
    if resources.get("../tools/runtime/python/") != "python/":
        log_err(
            "The Windows Tauri configuration must map the Python runtime directory recursively as "
            "'../tools/runtime/python/': 'python/'."
        )
        return 1

    targets = set(windows_config.get("bundle", {}).get("targets", []))
    if "nsis" not in targets or "msi" in targets:
        log_err("The Windows Tauri configuration must keep NSIS enabled as the fallback and leave MSI/MSIX to Master Packager Dev.")
        return 1

    nsis_config = windows_config.get("bundle", {}).get("windows", {}).get("nsis", {})
    if nsis_config.get("template") != "windows/installer.nsi":
        log_err("The Windows Tauri configuration must use the version-matched custom NSIS template.")
        return 1
    if nsis_config.get("installerHooks") != "windows/hooks.nsh":
        log_err("The Windows Tauri configuration must load the GitManager NSIS installer hooks.")
        return 1
    if r'StrCpy $INSTDIR "$LOCALAPPDATA\kastrick' not in nsis_template:
        log_err("The NSIS template does not set GitManager's expected per-user install directory before the directory page.")
        return 1
    if 'StrCpy $INSTDIR' in hooks:
        log_err("The NSIS pre-install hook must not overwrite a user's directory-page selection.")
        return 1
    expected_python_machine = PYTHON_EMBED_PACKAGES[windows_target_architecture()][2]
    if windows_pe_machine(runtime_dir / "python.exe") != expected_python_machine:
        log_err("The bundled Python runtime architecture does not match the selected Windows target.")
        return 1

    if "resources\\python\\python.exe" not in hooks:
        log_err("The NSIS post-install hook does not verify the bundled Python runtime.")
        return 1

    log_ok("Windows installer inputs are complete: private Python runtime, cleaner, NSIS fallback and MPDEV package path.")
    return 0


def prepare_master_packager_manifest(project_root: Path) -> Tuple[int, Optional[Path]]:
    """Generate the architecture-specific MPDEV manifest from the staged Windows payload."""
    if platform.system() != "Windows":
        return 0, None

    mpdev = find_master_packager_dev_cli()
    if mpdev is None:
        log_err(
            "Master Packager Dev is required for Windows MSI/MSIX installers and was not found. "
            "Install it with 'winget install --id MasterPackager.MasterPackagerDev --exact', "
            "then reopen PowerShell and retry."
        )
        return 1, None

    target_triple = os.environ.get("CARGO_BUILD_TARGET", "").strip()
    if target_triple.startswith("x86_64-"):
        package_arch = "x64"
    elif target_triple.startswith(("i686-", "i586-")):
        package_arch = "x86"
    elif target_triple.startswith("aarch64-"):
        package_arch = "arm64"
    else:
        log_err(f"Cannot determine the MPDEV package architecture from Rust target '{target_triple}'.")
        return 1, None

    staging = cargo_target_root(project_root) / "windows-package" / "source"
    if not (staging / "gitmanager.exe").is_file():
        log_err(f"Windows app payload was not staged for MPDEV: {staging}")
        return 1, None

    source_manifest = project_root / "installer" / "master-packager" / "package.json"
    if not source_manifest.is_file():
        log_err(f"Master Packager Dev package definition is missing: {source_manifest}")
        return 1, None

    try:
        package_config = json.loads(source_manifest.read_text(encoding="utf-8"))
        tauri_config = json.loads(
            (project_root / "src-tauri" / "tauri.conf.json").read_text(encoding="utf-8")
        )
        version_parts = str(tauri_config.get("version", "")).split(".")
        if not version_parts or len(version_parts) > 4 or any(not part.isdigit() for part in version_parts):
            raise ValueError("Tauri version must contain one to four numeric components for MSI/MSIX.")
        version_parts.extend(["0"] * (4 - len(version_parts)))
        if any(int(part) > 65535 for part in version_parts):
            raise ValueError("Windows Installer version components cannot exceed 65535.")

        package_config["version"] = ".".join(version_parts)
        package_config["platform"] = mpdev_platform_name(package_arch)
        package_config["outputTypes"] = ["msi", "msix"]
        store_publisher = os.environ.get("MSIX_PUBLISHER", "").strip()
        store_identity = os.environ.get("MSIX_IDENTITY_NAME", "").strip()
        if bool(store_publisher) != bool(store_identity):
            raise ValueError("Set both MSIX_PUBLISHER and MSIX_IDENTITY_NAME, or leave both unset.")
        if store_publisher and store_identity:
            package_config["publisher"] = store_publisher
            package_config["packageName"] = store_identity
            package_config.setdefault("msix", {}).pop("digitalSignature", None)
            log_info("Partner Center identity detected; MPDEV will create an unsigned Store-ready MSIX.")
        else:
            package_config.setdefault("msix", {}).setdefault(
                "digitalSignature",
                {
                    "signWith": "GenerateTestCertificate",
                    "timestampServer": "http://timestamp.sectigo.com",
                },
            )
            log_warn("MPDEV MSIX will use its development certificate; configure Partner Center identity for Store submission.")
        generated_manifest = cargo_target_root(project_root) / "windows-package" / "mpdev-package.json"
        generated_manifest.parent.mkdir(parents=True, exist_ok=True)
        generated_manifest.write_text(
            json.dumps(package_config, indent=2) + "\n",
            encoding="utf-8",
        )
    except (OSError, json.JSONDecodeError, ValueError) as error:
        log_err(f"Could not prepare the MPDEV MSI/MSIX definition: {error}")
        return 1, None

    log_ok(f"Prepared MPDEV {package_arch} MSI/MSIX manifest: {generated_manifest}")
    return 0, generated_manifest


def build_master_packager_installers(
    project_root: Path,
    generated_manifest: Path,
) -> Tuple[int, Optional[Dict[str, Path]]]:
    """Build both mandatory Windows MSI and MSIX packages with Master Packager Dev."""
    if platform.system() != "Windows":
        return 0, None

    mpdev = find_master_packager_dev_cli()
    if mpdev is None:
        log_err("Master Packager Dev is required but mpdev.exe could not be found.")
        return 1, None

    target_triple = os.environ.get("CARGO_BUILD_TARGET", "").strip()
    if target_triple.startswith("x86_64-"):
        package_arch = "x64"
    elif target_triple.startswith(("i686-", "i586-")):
        package_arch = "x86"
    elif target_triple.startswith("aarch64-"):
        package_arch = "arm64"
    else:
        log_err(f"Cannot determine the MPDEV package architecture from Rust target '{target_triple}'.")
        return 1, None

    staging = cargo_target_root(project_root) / "windows-package" / "source"
    output_dir = project_root / "build" / ".rcache" / "windows-installers"
    if not generated_manifest.is_file() or not (staging / "gitmanager.exe").is_file():
        log_err("MPDEV manifest or staged Windows app payload is missing.")
        return 1, None

    output_dir.mkdir(parents=True, exist_ok=True)
    try:
        for pattern in ("*.msi", "*.msix"):
            for existing in output_dir.glob(pattern):
                existing.unlink()
    except OSError as error:
        log_err(f"Could not clear previous MPDEV installer outputs: {error}")
        return 1, None

    environment = os.environ.copy()
    environment["GITMANAGER_APP_SOURCE"] = str(staging.resolve())
    command = [
        str(mpdev), "build", str(generated_manifest),
        "--working-dir", str(project_root),
        "--use-msbuild-message-format", "true",
    ]
    log_info(
        f"Master Packager Dev is building the required {package_arch} MSI and MSIX "
        "from the staged Windows application payload..."
    )
    result = run_command(command, cwd=project_root, env=environment, work=f"MPDEV {package_arch} MSI/MSIX", progress="1/1")
    if result != 0:
        log_err(f"Master Packager Dev failed to build Windows MSI/MSIX (exit code {result}).")
        return result, None

    packages: Dict[str, Path] = {}
    for extension in ("msi", "msix"):
        candidates = sorted(
            path for path in output_dir.glob(f"*.{extension}")
            if path.is_file() and path.stat().st_size >= 1024 * 1024
        )
        if len(candidates) != 1:
            log_err(
                f"MPDEV must produce exactly one non-empty .{extension.upper()} package; "
                f"found {len(candidates)} in {output_dir}."
            )
            return 1, None
        packages[extension] = candidates[0]
        log_ok(f"Created MPDEV {extension.upper()}: {candidates[0]} ({candidates[0].stat().st_size:,} bytes)")

    return 0, packages



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
    """Capture a short probe while still reporting when it starts and finishes."""
    label = f"Probe {Path(str(cmd[0])).name}" if cmd else "Command probe"
    log_work(label, "1/1", "WORKING", f"Checking {' '.join(str(part) for part in cmd)}...")
    started_at = time.monotonic()
    try:
        proc = subprocess.run(
            [str(part) for part in cmd],
            cwd=str(cwd) if cwd else None,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="replace",
            check=False,
        )
        stdout = proc.stdout.strip()
        stderr = proc.stderr.strip()
        elapsed = max(time.monotonic() - started_at, 0.001)
        log_work(label, "1/1", "DONE", f"Probe returned exit code {proc.returncode} in {elapsed:.1f}s.")
        log_debug(f"{label} stdout: {stdout or '(empty)'}")
        if stderr:
            log_debug(f"{label} stderr: {stderr}")
        return proc.returncode, stdout, stderr
    except OSError as error:
        message = f"Could not run {cmd[0] if cmd else 'command'}: {error}"
        log_work(label, "1/1", "FAILED", message)
        return 127, "", message


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
    for package in ("git-manager", "gitm-cli", "gitmanager-setup", "crash-helper", "gitmanager-service"):
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
        | package_graphs.get("gitmanager-service", set())
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

        output_dir = project_root / "build" / "dist" / "deployment"
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
    signing_config: Dict[str, object] = {
        "bundle": {
            "windows": {
                "certificateThumbprint": None,
                "digestAlgorithm": None,
                "signCommand": None,
                "timestampUrl": None,
            }
        }
    }

    if thumbprint:
        if not re.fullmatch(r"[A-F0-9]{40}", thumbprint):
            log_err("WINDOWS_SIGNING_THUMBPRINT must be a 40-character SHA-1 thumbprint.")
            return None
        sign_script = project_root / "tools" / "sign_windows_file.ps1"
        if not sign_script.is_file():
            log_err(f"Windows signing script is missing: {sign_script}")
            return None
        signing_config["bundle"] = {
            "windows": {
                "certificateThumbprint": None,
                "digestAlgorithm": None,
                "timestampUrl": None,
                "signCommand": {
                    "cmd": "powershell.exe",
                    "args": [
                        "-NoLogo",
                        "-NoProfile",
                        "-NonInteractive",
                        "-ExecutionPolicy",
                        "Bypass",
                        "-File",
                        str(sign_script.resolve()),
                        "-FilePath",
                        "%1",
                    ],
                },
            }
        }
        log_ok("Enabled Tauri's custom signing command because a Windows certificate is configured.")
    else:
        log_info("No Windows signing certificate configured; explicitly disabling Tauri signing for this unsigned build.")

    config_path = cargo_target_root(project_root) / "build-config" / "tauri.windows.signing.conf.json"
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
        # tauri-cli is a host-side developer tool. Do not cross-compile the
        # CLI itself just because CARGO_BUILD_TARGET selects the app target.
        build_target = os.environ.pop("CARGO_BUILD_TARGET", None)
        try:
            rc = run_command(install_cmd, work="Install Rust Tauri CLI", progress="1/1")
        finally:
            if build_target is not None:
                os.environ["CARGO_BUILD_TARGET"] = build_target
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
    configured_target = os.environ.get("CARGO_BUILD_TARGET", "").strip()
    if configured_target:
        return configured_target

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
    target_triple = os.environ.get("CARGO_BUILD_TARGET", "").strip()
    target_args = ["--target", target_triple] if target_triple else []

    commands = [
        (
            "native gitm CLI",
            common_prefix + ["build", "--package", "gitm-cli", "--bin", "gitm"] + target_args,
        ),
        (
            "GitManager first-run setup companion",
            common_prefix + ["build", "--package", "gitmanager-setup", "--bin", "gitmanager-setup"] + target_args,
        ),
        (
            "crash_helper UI supervisor",
            common_prefix + ["build", "--package", "crash-helper", "--bin", "crash_helper"] + target_args,
        ),
    ]
    if release:
        for _, command in commands:
            command.append("--release")

    for command_index, (description, command) in enumerate(commands, start=1):
        result = run_command(
            command,
            cwd=project_root,
            work=f"Build {description}",
            progress=f"{command_index}/{len(commands) + 1}",
        )
        if result != 0:
            log_err(f"The {description} failed to build.")
            return result

    try:
        host_triple = resolve_rust_target_triple(toolchain_arg)
    except RuntimeError as error:
        log_err(str(error))
        return 1

    target_dir = cargo_target_root(project_root)
    if not target_dir.is_absolute():
        target_dir = project_root / target_dir
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

    # The service is compiled independently from Tauri. A placeholder is kept
    # only so the separate desktop build can validate its declared externalBin.
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

    service_filename = f"gitmanager.service-{binary_triple}{executable_suffix}"
    service_sidecar = sidecar_dir / service_filename
    service_source = binary_dir / f"gitmanager-service{executable_suffix}"

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

    service_cmd = common_prefix + ["build", "--package", "gitmanager-service", "--bin", "gitmanager-service"] + target_args
    if release:
        service_cmd.append("--release")
    service_result = run_command(
        service_cmd,
        cwd=project_root,
        work="Build GitManager background service",
        progress=f"{len(commands) + 1}/{len(commands) + 1}",
    )
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
            sign_targets = [*staged, setup_artifact]
            for sign_index, target in enumerate(sign_targets, start=1):
                sign_cmd = [
                    "powershell.exe", "-NoLogo", "-NoProfile", "-NonInteractive",
                    "-ExecutionPolicy", "Bypass", "-File", str(sign_script),
                    "-FilePath", str(target),
                ]
                sign_rc = run_command(
                    sign_cmd, cwd=project_root, work=f"Sign {target.name}",
                    progress=f"{sign_index}/{len(sign_targets)}",
                )
                if sign_rc != 0:
                    log_err(f"Could not sign GitManager build output: {target}")
                    return sign_rc

    log_ok(f"Created standalone setup helper: {setup_artifact} ({setup_artifact.stat().st_size:,} bytes)")
    return 0



def ensure_rust_target_components(targets: List[BuildTarget], toolchain_arg: str) -> int:
    """Install missing Rust target std components with the already-selected rustup toolchain."""
    rustup = shutil.which("rustup")
    if not rustup:
        try:
            host_triple = resolve_rust_target_triple(toolchain_arg)
        except RuntimeError as error:
            log_err(str(error))
            return 1
        missing = [target.triple for target in targets if target.triple != host_triple]
        if missing:
            log_err(
                "rustup is unavailable, and these selected architecture targets are not the active "
                "compiler host target: " + ", ".join(missing)
            )
            log_info("Install rustup or add the required Rust target components manually.")
            return 1
        return 0

    command = [rustup, "target", "list", "--installed"]
    selected_toolchain = toolchain_arg[1:] if toolchain_arg.startswith("+") else ""
    if selected_toolchain:
        command.extend(["--toolchain", selected_toolchain])
    code, output, error = run_capture(command)
    if code != 0:
        log_err(f"Could not inspect installed Rust target components: {error or output or code}")
        return code or 1

    installed = {line.strip() for line in output.splitlines() if line.strip()}
    missing = sorted({target.triple for target in targets if target.triple not in installed})
    for target_index, triple in enumerate(missing, start=1):
        add_command = [rustup, "target", "add", triple]
        if selected_toolchain:
            add_command.extend(["--toolchain", selected_toolchain])
        log_info(f"Installing missing Rust target component locally: {triple}")
        result = run_command(add_command, work=f"Install Rust target {triple}", progress=f"{target_index}/{len(missing)}")
        if result != 0:
            log_err(
                f"Could not install Rust target {triple}. Confirm the selected toolchain supports it "
                "and retry after installing its native linker/SDK dependencies."
            )
            return result
    if not missing:
        log_ok("All selected Rust target components are already installed.")
    return 0


def prompt_build_selection() -> str:
    """Ask which local architecture set to build; Enter selects every locally supported architecture."""
    host = host_platform()
    _, host_targets = parse_build_selection(host)
    available = []
    unavailable = []
    for target in host_targets:
        can_build, reason = can_build_locally(target)
        if can_build:
            available.append(target)
        else:
            unavailable.append((target, reason))

    print()
    print(f"Detected build host: {host} ({platform.machine()})")
    print("Choose the local build architecture:")
    print("  [A] All architectures supported by this machine (default)")
    for index, target in enumerate(available, start=1):
        print(f"  [{index}] {target.selector}  [{target.triple}]")
    for target, reason in unavailable:
        print(f"  [-] {target.selector}  [not available locally: {reason}]")

    if not available:
        raise RuntimeError(f"No supported {host} target can be built on this machine.")

    if not sys.stdin.isatty():
        log_info("Non-interactive build: selecting all architectures supported by this machine.")
        return host

    try:
        choice = input("Architecture selection [A]: ").strip().lower()
    except (EOFError, KeyboardInterrupt):
        print()
        raise RuntimeError("Build selection was cancelled.")

    if not choice or choice in {"a", "all"}:
        return host
    if choice.isdigit():
        index = int(choice)
        if 1 <= index <= len(available):
            return available[index - 1].selector
    raise ValueError("Invalid architecture selection. Choose A or one of the listed numbers.")


def resolve_local_build_targets(selection: str) -> List[BuildTarget]:
    """Resolve a selector into targets that the current machine can actually build."""
    kind, selected = parse_build_selection(selection)
    host = host_platform()

    if kind == "exact":
        target = selected[0]
        can_build, reason = can_build_locally(target)
        if not can_build:
            if target.platform == "linux" and host == "windows":
                raise ValueError(
                    reason + " Prepare it with "
                    + f"py build.py setup-build-tools --build='{target.selector}', "
                    + "then run the build from the Ubuntu WSL2 checkout."
                )
            if target.platform == "macos" and host != "macos":
                raise ValueError(reason + " Apple's SDK must be used on a real Mac.")
            raise ValueError(reason)
        return [target]

    if kind == "group":
        group_platform = selected[0].platform if selected else ""
        if group_platform != host:
            raise ValueError(
                f"Local builds cannot target {group_platform} from this {host} machine. "
                "Run the build on a machine with the target operating system and native SDK."
            )

    local_candidates = [target for target in selected if target.platform == host]
    local_targets = []
    skipped = []
    for target in local_candidates:
        can_build, reason = can_build_locally(target)
        if can_build:
            local_targets.append(target)
        else:
            skipped.append((target, reason))
    foreign = [target for target in selected if target.platform != host]

    if foreign:
        platforms = ", ".join(sorted({target.platform for target in foreign}))
        log_warn(
            f"Skipping {platforms}: this machine is {host}. Cross-OS desktop builds require "
            "the corresponding operating system and native SDK; no GitHub Actions job will be started."
        )
    for target, reason in skipped:
        log_warn(f"Skipping {target.selector}: {reason}")

    if not local_targets:
        raise ValueError(f"No selected target can be built locally on this {host} machine.")
    return local_targets


def configure_local_target(project_root: Path, target: BuildTarget) -> None:
    os.environ["CARGO_BUILD_TARGET"] = target.triple
    os.environ["CARGO_TARGET_DIR"] = str(project_root / "build" / ".rcache")
    os.environ["GITMANAGER_BUILD_SELECTOR"] = target.selector
    log_info(f"Selected local target: {target.selector} ({target.triple})")
    log_info(f"Rust build cache: {os.environ['CARGO_TARGET_DIR']}")
    log_info(f"Artifacts: {project_root / 'build' / 'dist' / target.output_name}")


def build_local_store_package(
    project_root: Path,
    store: str,
    target: BuildTarget,
    toolchain_arg: str,
    has_cargo_tauri: bool,
    release: bool,
) -> int:
    """Build Flatpak or Store MSIX on the native local host, without workflow dispatch."""
    if store == "flatpak":
        if target.platform != "linux":
            log_err("Flatpak packaging is supported only from a native Linux build host.")
            return 2
        if target.architecture not in ("x64", "arm64"):
            log_err(
                "The current GNOME 51 Flatpak runtime supports Linux x64 and ARM64 only; "
                f"{target.architecture} is available as a DEB/RPM desktop build, not a Flatpak target."
            )
            return 2
        result = run_build_or_dev(
            project_root=project_root,
            mode="build",
            toolchain_arg=toolchain_arg,
            has_cargo_tauri=has_cargo_tauri,
            release=release,
            bundle=True,
            bundle_formats="deb",
        )
        if result != 0:
            return result
        deb_dir = project_root / "build" / "dist" / target.output_name / "installer"
        debs = sorted(path for path in deb_dir.glob("*.deb") if path.is_file() and path.stat().st_size > 0)
        if len(debs) != 1:
            log_err(f"Expected exactly one Linux DEB payload for Flatpak staging; found {len(debs)} in {deb_dir}.")
            return 1
        command = [
            sys.executable,
            str(project_root / "tools" / "package_flatpak.py"),
            "--deb",
            str(debs[0]),
        ]
        log_info("Building the Flatpak bundle locally from the native Linux DEB payload...")
        return run_command(command, cwd=project_root, work="Build Flatpak bundle", progress="1/1")

    if store == "msix":
        if target.platform != "windows":
            log_err("Microsoft Store MSIX packaging is supported only from a native Windows build host.")
            return 2
        mpdev = find_master_packager_dev_cli()
        if mpdev is None:
            log_err(
                "Master Packager Dev is required for Microsoft Store MSIX packaging. "
                "Install it with 'winget install --id MasterPackager.MasterPackagerDev --exact'."
            )
            return 1
        result = run_build_or_dev(
            project_root=project_root,
            mode="build",
            toolchain_arg=toolchain_arg,
            has_cargo_tauri=has_cargo_tauri,
            release=release,
            bundle=False,
        )
        if result != 0:
            return result

        app_dir = project_root / "build" / "dist" / target.output_name / "app"
        platform_name = target.architecture
        mpdev_platform = mpdev_platform_name(platform_name)
        stage_command = [
            sys.executable,
            str(project_root / "tools" / "package_windows_store.py"),
            "--app-dir", str(app_dir),
            "--platform", platform_name,
        ]
        staged = run_command(stage_command, cwd=project_root, work="Stage Microsoft Store app payload", progress="1/2")
        if staged != 0:
            return staged

        manifest = cargo_target_root(project_root) / "windows-store" / "package.json"
        output_dir = project_root / "build" / "dist" / "store" / "msix"
        output_dir.mkdir(parents=True, exist_ok=True)
        for existing in output_dir.glob("*.msix"):
            if existing.stem.casefold().endswith(f"_{mpdev_platform.casefold()}"):
                existing.unlink()
        environment = os.environ.copy()
        environment["GITMANAGER_APP_SOURCE"] = str(
            (cargo_target_root(project_root) / "windows-store" / "source").resolve()
        )
        if not (Path(environment["GITMANAGER_APP_SOURCE"]) / "gitmanager.exe").is_file():
            log_err("The staged Microsoft Store app payload is missing gitmanager.exe.")
            return 1
        command = [
            str(mpdev), "build", str(manifest),
            "--working-dir", str(project_root),
            "--use-msbuild-message-format", "true",
        ]
        log_info(f"Building the Microsoft Store {platform_name} MSIX locally with MPDEV...")
        result = run_command(
            command, cwd=project_root, env=environment,
            work=f"Build Microsoft Store {platform_name} MSIX", progress="2/2",
        )
        if result != 0:
            return result
        packages = sorted(
            path for path in output_dir.glob("*.msix")
            if path.is_file()
            and path.stem.casefold().endswith(f"_{mpdev_platform.casefold()}")
            and path.stat().st_size > 1024 * 1024
        )
        if len(packages) != 1:
            log_err(f"Expected one non-empty Microsoft Store MSIX; found {len(packages)} in {output_dir}.")
            return 1
        log_ok(f"Created Microsoft Store MSIX: {packages[0]} ({packages[0].stat().st_size:,} bytes)")
        return 0

    log_err(f"Unsupported local store build selection: {store}")
    return 2


def publish_build_outputs(
    project_root: Path,
    release: bool,
    bundle: bool,
    include_mpdev: bool = False,
    bundle_formats: Optional[str] = None,
) -> int:
    """Copy target outputs out of Cargo's cache into a stable build/dist layout."""
    selector = os.environ.get("GITMANAGER_BUILD_SELECTOR", "").strip()
    try:
        _, targets = parse_build_selection(selector)
        target = targets[0]
    except (ValueError, IndexError):
        log_err("Could not resolve the selected target while publishing build outputs.")
        return 1

    profile_dir = target_profile_dir(project_root, release)
    output_root = project_root / "build" / "dist" / target.output_name
    marker = output_root / ".gitmanager-generated"
    if output_root.exists():
        if not marker.is_file():
            log_err(f"Refusing to replace an output directory without GitManager's marker: {output_root}")
            return 1
        try:
            shutil.rmtree(output_root)
        except OSError as error:
            log_err(f"Could not clear previous GitManager build outputs: {error}")
            return 1

    try:
        output_root.mkdir(parents=True, exist_ok=True)
        marker.write_text(
            "Generated by GitManager build.py. Safe to recreate through the build script.\n",
            encoding="utf-8",
        )
        if bundle:
            bundle_source = profile_dir / "bundle"
            if not bundle_source.is_dir():
                log_err(f"Tauri completed without producing the expected bundle directory: {bundle_source}")
                return 1
            bundle_files = [path for path in bundle_source.rglob("*") if path.is_file()]
            if not bundle_files:
                log_err(f"Tauri created an empty installer bundle directory: {bundle_source}")
                return 1
            if target.platform == "linux":
                required_extensions = (
                    {f".{item.strip().lower()}" for item in bundle_formats.split(",") if item.strip()}
                    if bundle_formats
                    else {".deb", ".rpm"}
                )
            elif target.platform == "macos":
                required_extensions = {".dmg"}
            else:
                required_extensions = set()
            missing_extensions = sorted(
                extension for extension in required_extensions
                if not any(path.suffix.lower() == extension for path in bundle_files)
            )
            if missing_extensions:
                log_err(
                    "Tauri did not produce the expected installer formats: "
                    + ", ".join(missing_extensions)
                )
                return 1
            installer_dir = output_root / "installer"
            shutil.copytree(bundle_source, installer_dir)
            if target.platform == "windows" and include_mpdev:
                mpdev_outputs = cargo_target_root(project_root) / "windows-installers"
                for pattern in ("*.msi", "*.msix"):
                    for package in sorted(mpdev_outputs.glob(pattern)):
                        if package.is_file() and package.stat().st_size > 0:
                            shutil.copy2(package, installer_dir / package.name)
            log_ok(f"Published installer bundles: {installer_dir}")

        suffix = ".exe" if target.platform == "windows" else ""
        setup_alias = "GitManager-Setup" + suffix
        sidecar_dir = project_root / "src-tauri" / "binaries"
        sidecar_sources = {
            f"gitm{suffix}": sidecar_dir / f"gitm-{target.triple}{suffix}",
            f"gitmanager-setup{suffix}": sidecar_dir / f"gitmanager-setup-{target.triple}{suffix}",
            f"crash_helper{suffix}": sidecar_dir / f"crash_helper-{target.triple}{suffix}",
            f"gitmanager.service{suffix}": sidecar_dir / f"gitmanager.service-{target.triple}{suffix}",
        }
        names = (
            f"gitmanager{suffix}",
            f"gitm{suffix}",
            f"gitmanager-setup{suffix}",
            f"crash_helper{suffix}",
            f"gitmanager.service{suffix}",
            setup_alias,
        )
        app_dir = output_root / "app"
        app_dir.mkdir(parents=True, exist_ok=True)
        missing = []
        for name in names:
            source = profile_dir / name
            if not source.is_file() and name in sidecar_sources:
                source = sidecar_sources[name]
            if not source.is_file() or source.stat().st_size == 0:
                missing.append(name)
                continue
            shutil.copy2(source, app_dir / name)
        if missing:
            log_err("Build output is missing required app executables: " + ", ".join(missing))
            return 1

        resources_dir = app_dir / "resources"
        if target.platform == "windows":
            staged_resources = cargo_target_root(project_root) / "windows-package" / "source" / "resources"
            if not staged_resources.is_dir():
                log_err(f"Windows app resources were not staged: {staged_resources}")
                return 1
            shutil.copytree(staged_resources, resources_dir)
        else:
            resources_dir.mkdir(parents=True, exist_ok=True)
            for name in names[1:5]:
                shutil.copy2(app_dir / name, resources_dir / name)
            cleaner = project_root / "tools" / "gitmanager_cleaner.py"
            if not cleaner.is_file():
                log_err(f"Cleaner resource is missing: {cleaner}")
                return 1
            tools_dir = resources_dir / "tools"
            tools_dir.mkdir()
            shutil.copy2(cleaner, tools_dir / cleaner.name)
        log_ok(f"Published complete app payload: {app_dir}")

        manifest = {
            "product": "GitManager",
            "target": target.triple,
            "selector": target.selector,
            "type": "installer" if bundle else "app",
            "profile": "release" if release else "debug",
        }
        (output_root / "build-manifest.json").write_text(
            json.dumps(manifest, indent=2) + "\n",
            encoding="utf-8",
        )
    except OSError as error:
        log_err(f"Could not publish GitManager build outputs: {error}")
        return 1
    return 0

def run_build_or_dev(
    project_root: Path,
    mode: str,
    toolchain_arg: str,
    has_cargo_tauri: bool,
    release: bool,
    bundle: bool = True,
    bundle_formats: Optional[str] = None,
) -> int:
    src_tauri_dir = project_root / "src-tauri"

    if mode == "check":
        log_info("Running `cargo check` on Pure-Rust Tauri v2 backend...")
        cmd = ["cargo"]
        if toolchain_arg:
            cmd.append(toolchain_arg)
        cmd.extend(["check", "--manifest-path", str(src_tauri_dir / "Cargo.toml")])
        return run_command(cmd, work="Cargo check", progress="1/1")

    if mode == "build" and os.environ.get("GITMANAGER_SKIP_CLOUD_NODE_BUNDLE") != "1":
        cloud_node_result = prepare_cloud_node_deployment(project_root)
        if cloud_node_result != 0:
            return cloud_node_result

    if mode == "build" and platform.system() == "Windows" and bundle:
        refresh_windows_packaging_path()
        mpdev_cli = find_master_packager_dev_cli()
        if mpdev_cli is None:
            log_err(
                "Master Packager Dev is mandatory for Windows MSI/MSIX builds. "
                "Install it with 'winget install --id MasterPackager.MasterPackagerDev --exact'."
            )
            return 1
        nsis_available = bool(shutil.which("makensis")) or any(
            (directory / "makensis.exe").is_file()
            for directory in windows_packaging_search_directories()
        )
        if not nsis_available:
            log_err(
                "NSIS is required as the fallback Windows setup installer. "
                "Install it with 'winget install --id NSIS.NSIS --exact' "
                "or run 'py build.py setup-packaging-tools'."
            )
            return 1

    if mode == "build" and platform.system() == "Windows":
        runtime_result = prepare_windows_python_runtime(project_root)
        if runtime_result != 0:
            return runtime_result
        packaging_inputs_result = verify_windows_packaging_inputs(project_root)
        if packaging_inputs_result != 0:
            return packaging_inputs_result

    if mode == "build" and not has_cargo_tauri:
        log_err("The Tauri CLI is required to create GitManager-Setup.exe. Install failed or pass --install-tauri-cli.")
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
            target_triple = os.environ.get("CARGO_BUILD_TARGET", "").strip()
            if target_triple:
                cmd.extend(["--target", target_triple])
            if not bundle:
                cmd.append("--no-bundle")
            elif bundle_formats:
                cmd.extend(["--bundles", bundle_formats])
            elif target_triple == "armv7-unknown-linux-gnueabihf":
                # Cross-built ARMv7 AppImages need a QEMU runtime; ship native
                # Debian/RPM packages for this target in the first matrix pass.
                cmd.extend(["--bundles", "deb,rpm"])
            if not release:
                cmd.append("--debug")
            elif platform.system() == "Windows":
                signing_config = prepare_windows_tauri_signing_config(project_root)
                if signing_config is None:
                    log_err("Could not prepare an explicit Windows signing override; refusing to build with an ambiguous signing configuration.")
                    return 1
                cmd.extend(["--config", str(signing_config.resolve())])
        rc = run_command(cmd, cwd=src_tauri_dir, work=f"Tauri {mode}", progress="1/1")
        if rc != 0 or mode != "build":
            return rc

        # Stage the Windows app resources for both app-only and installer outputs.
        if platform.system() == "Windows":
            stage_result = stage_windows_mpdev_package(project_root, toolchain_arg, release)
            if stage_result != 0:
                return stage_result

        # NSIS remains the fallback setup executable. MPDEV is the required
        # producer of both Windows MSI and MSIX installers for every Windows
        # architecture selected by the local build script.
        include_mpdev = False
        if bundle and platform.system() == "Windows":
            if name_windows_setup_installer(project_root, release) is None:
                return 1
            manifest_result, mpdev_manifest = prepare_master_packager_manifest(project_root)
            if manifest_result != 0 or mpdev_manifest is None:
                return manifest_result or 1
            if os.environ.get("GITMANAGER_DEFER_MPDEV_PACKAGING") == "1":
                # Release CI signs the staged binaries before it invokes MPDEV.
                # Ordinary local builds always package MSI/MSIX immediately.
                log_info("MPDEV manifest is ready; release signing runs before MSI/MSIX generation.")
            else:
                mpdev_result, mpdev_packages = build_master_packager_installers(project_root, mpdev_manifest)
                if mpdev_result != 0 or not mpdev_packages:
                    return mpdev_result or 1
                include_mpdev = True
        if publish_build_outputs(
            project_root,
            release,
            bundle,
            include_mpdev,
            bundle_formats=bundle_formats,
        ) != 0:
            return 1
        return 0

    # Fallback direct Cargo execution
    cmd = ["cargo"]
    if toolchain_arg:
        cmd.append(toolchain_arg)

    if mode == "dev":
        cmd.extend(["run", "--bin", "gitmanager", "--manifest-path", str(src_tauri_dir / "Cargo.toml")])
    else:
        cmd.extend(["build", "--bins", "--manifest-path", str(src_tauri_dir / "Cargo.toml")])
        if release:
            cmd.append("--release")

    return run_command(cmd, cwd=project_root, work=f"Cargo {mode}", progress="1/1")


def main() -> int:
    parser = argparse.ArgumentParser(
        description="GitManager local dependency, architecture and installer build script"
    )
    parser.add_argument(
        "mode",
        nargs="?",
        default="build",
        choices=["deps", "check", "dev", "build", "frontend-build", "frontend-dev", "prepare-runtime", "prepare-sidecar", "prepare-cloud-node", "setup-packaging-tools", "setup-build-tools"],
        help="Action: deps, check, dev, build, frontend-build, frontend-dev, prepare-runtime, prepare-sidecar, prepare-cloud-node, setup-packaging-tools, or setup-build-tools.",
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
        "--verbose",
        action="store_true",
        help="Show gray diagnostic/debug lines during the build.",
    )
    parser.add_argument(
        "--install-tauri-cli",
        action="store_true",
        help="Install the Rust tauri-cli crate if it is not already present.",
    )
    parser.add_argument(
        "--install-packaging-tools",
        action="store_true",
        help="Install the required Windows MPDEV packager and NSIS fallback through WinGet.",
    )
    parser.add_argument(
        "-build", "--build",
        dest="build_target",
        default=None,
        help="Local target: all architectures supported by this host (default), windows, linux, macos, or an exact target such as windows(10,x64), windows(11,arm64), linux(arm64), or macos(x64). No GitHub Actions dispatch is performed.",
    )
    parser.add_argument(
        "-type", "--type",
        dest="build_type",
        choices=["installer", "app"],
        default="installer",
        help="Output type: installer (default) or app (no installer bundle).",
    )
    parser.add_argument(
        "--master-packager",
        action="store_true",
        help="Compatibility flag. MPDEV always builds Windows MSI and MSIX for --type=installer; this option is no longer required.",
    )
    parser.add_argument(
        "-store", "--store",
        dest="store_target",
        choices=["flatpak", "msix", "all"],
        help="Build a store package locally: flatpak on Linux or msix on Windows. 'all' requires separate native Linux and Windows builds.",
    )

    args = parser.parse_args()
    global VERBOSE_LOGGING
    VERBOSE_LOGGING = VERBOSE_LOGGING or bool(args.verbose)
    project_root = Path(__file__).resolve().parent
    os.environ["CARGO_TARGET_DIR"] = str(project_root / "build" / ".rcache")

    if args.store_target and args.mode not in ("build", "setup-build-tools"):
        log_err("--store can only be used with the build or setup-build-tools actions.")
        return 2
    if args.store_target == "all" and args.mode == "build":
        log_err(
            "--store=all cannot build both store formats from one operating system. "
            "Run --store=flatpak on Linux and --store=msix on Windows; both build locally."
        )
        return 2

    if args.mode == "setup-packaging-tools":
        return setup_windows_packaging_tools()
    if args.mode == "setup-build-tools":
        return setup_build_environment(args.build_target, args.store_target)
    if args.mode == "prepare-runtime":
        return prepare_windows_python_runtime(project_root)
    if args.install_packaging_tools:
        packaging_setup_result = setup_windows_packaging_tools()
        if packaging_setup_result != 0:
            return packaging_setup_result
    if args.mode == "prepare-cloud-node":
        return prepare_cloud_node_deployment(project_root)
    if args.mode in ("frontend-build", "frontend-dev"):
        frontend_tool = project_root / "tools" / "nextjs_engine_source.py"
        frontend_mode = "build" if args.mode == "frontend-build" else "dev"
        return run_command(
            [sys.executable, str(frontend_tool), frontend_mode, "--project-root", str(project_root)],
            cwd=project_root, work=f"Frontend {frontend_mode}", progress="1/1",
        )

    selected_targets = []
    if args.mode == "build":
        try:
            selection = args.build_target or prompt_build_selection()
            selected_targets = resolve_local_build_targets(selection)
        except (RuntimeError, ValueError) as error:
            log_err(str(error))
            return 2

        if args.store_target:
            expected_platform = "linux" if args.store_target == "flatpak" else "windows"
            if host_platform() != expected_platform:
                log_err(
                    f"--store={args.store_target} must be built locally on {expected_platform}; "
                    f"this computer is {host_platform()}. The build script will not dispatch a remote build."
                )
                return 2
            wrong_targets = [target.selector for target in selected_targets if target.platform != expected_platform]
            if wrong_targets:
                log_err(
                    f"--store={args.store_target} cannot package target(s) for another operating system: "
                    + ", ".join(wrong_targets)
                )
                return 2

    print("=" * 72)
    print(" GitManager — Local Tauri v2 Dependency & Toolchain Manager")
    print("=" * 72)

    check_os_system_dependencies()
    sanitize_tauri_v2_config(project_root)

    toolchain_arg = check_and_resolve_rust(project_root)
    if toolchain_arg is None:
        return 1

    report_dependency_count(project_root, toolchain_arg)
    log_info(
        "Cargo's n/N progress meter counts compilation units (including build scripts and "
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
    if args.mode == "build":
        target_result = ensure_rust_target_components(selected_targets, toolchain_arg)
        if target_result != 0:
            return target_result

    if args.mode == "prepare-sidecar":
        return build_gitm_sidecar(project_root, toolchain_arg, is_release)

    if args.mode == "build":
        results = []
        for target in selected_targets:
            configure_local_target(project_root, target)
            if args.store_target:
                result = build_local_store_package(
                    project_root=project_root,
                    store=args.store_target,
                    target=target,
                    toolchain_arg=toolchain_arg,
                    has_cargo_tauri=has_cargo_tauri,
                    release=is_release,
                )
            else:
                result = run_build_or_dev(
                    project_root=project_root,
                    mode="build",
                    toolchain_arg=toolchain_arg,
                    has_cargo_tauri=has_cargo_tauri,
                    release=is_release,
                    bundle=args.build_type == "installer",
                )
            results.append((target, result))
            if result == 0:
                log_ok(f"Completed local build: {target.selector}")
            else:
                log_err(f"Local build failed for {target.selector} (exit code {result}).")

        failed = [(target, result) for target, result in results if result != 0]
        if failed:
            log_err(
                f"{len(failed)} of {len(results)} selected local target(s) failed: "
                + ", ".join(target.selector for target, _ in failed)
            )
            return failed[0][1] or 1
        log_ok(f"Completed {len(results)} local build target(s).")
        return 0

    return run_build_or_dev(
        project_root=project_root,
        mode=args.mode,
        toolchain_arg=toolchain_arg,
        has_cargo_tauri=has_cargo_tauri,
        release=is_release,
        bundle=args.build_type == "installer",
    )


if __name__ == "__main__":
    sys.exit(main())

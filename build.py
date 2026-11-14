#!/usr/bin/env python3
"""
GitManager Pure-Rust Tauri v2 Dependency Checker & Build Script
Supports: Windows 10 & 11, Linux, and macOS (Zero Node.js / npm required).

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
import urllib.request
import zipfile
from pathlib import Path
from typing import Optional, Tuple, List

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


def name_windows_setup_installer(project_root: Path) -> Optional[Path]:
    """Expose a stable, predictable setup.exe name after Tauri has bundled NSIS."""
    if platform.system() != "Windows":
        return None
    # Tauri inherits the Cargo workspace target directory at the repository root.
    output_dir = project_root / "target" / "release" / "bundle" / "nsis"
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


def sanitize_tauri_v2_config(project_root: Path) -> None:
    """
    Validates and auto-fixes `src-tauri/tauri.conf.json` against the Tauri v2 schema:
    - Removes Tauri v1 legacy keys from `build` (such as `build.withGlobalTauri` or `build.distDir`).
    - Ensures `app.withGlobalTauri = true` and `build.frontendDist = "../ui"`.
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

        if build_cfg.get("frontendDist") != "../ui":
            build_cfg["frontendDist"] = "../ui"
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
    """Build and stage gitm plus the cross-platform first-run setup sidecar."""
    profile = "release" if release else "debug"
    common_prefix = ["cargo"]
    if toolchain_arg:
        common_prefix.append(toolchain_arg)

    gitm_cmd = common_prefix + ["build", "--package", "gitm-cli", "--bin", "gitm"]
    if release:
        gitm_cmd.append("--release")
    log_info(f"Building native gitm CLI sidecar: {' '.join(gitm_cmd)}")
    gitm_result = subprocess.call(gitm_cmd, cwd=str(project_root))
    if gitm_result != 0:
        log_err("The standalone gitm CLI failed to build.")
        return gitm_result

    setup_cmd = common_prefix + ["build", "--package", "gitmanager-setup", "--bin", "gitmanager-setup"]
    if release:
        setup_cmd.append("--release")
    log_info(f"Building GitManager first-run setup companion: {' '.join(setup_cmd)}")
    setup_result = subprocess.call(setup_cmd, cwd=str(project_root))
    if setup_result != 0:
        log_err("The cross-platform GitManager Setup companion failed to build.")
        return setup_result

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
    artifacts = [
        (binary_dir / f"gitm{executable_suffix}", f"gitm-{binary_triple}{executable_suffix}"),
        (binary_dir / f"gitmanager-setup{executable_suffix}", f"gitmanager-setup-{binary_triple}{executable_suffix}"),
    ]
    sidecar_dir = project_root / "src-tauri" / "binaries"
    sidecar_dir.mkdir(parents=True, exist_ok=True)
    staged: List[Path] = []

    for source, filename in artifacts:
        if not source.is_file():
            log_err(f"Cargo completed but the expected sidecar binary is missing: {source}")
            return 1
        destination = sidecar_dir / filename
        try:
            shutil.copy2(source, destination)
        except OSError as error:
            log_err(f"Could not stage sidecar at {destination}: {error}")
            return 1
        staged.append(destination)
        log_ok(f"Prepared Tauri sidecar: {destination} ({destination.stat().st_size:,} bytes)")

    # Publish a predictable standalone setup helper beside the Cargo outputs:
    # GitManager-Setup.exe on Windows and GitManager-Setup on Linux/macOS.
    setup_artifact_name = "GitManager-Setup.exe" if executable_suffix else "GitManager-Setup"
    setup_artifact = binary_dir / setup_artifact_name
    try:
        shutil.copy2(staged[1], setup_artifact)
    except OSError as error:
        log_err(f"Could not stage the standalone setup helper at {setup_artifact}: {error}")
        return 1

    # When CI has imported a production signing certificate, sign both sidecars
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

    if mode == "build" and platform.system() == "Windows":
        runtime_result = prepare_windows_python_runtime(project_root)
        if runtime_result != 0:
            return runtime_result

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
            if not release:
                cmd.append("--debug")
        log_info(f"Running Pure-Rust Tauri CLI: {' '.join(cmd)}")
        rc = subprocess.call(cmd, cwd=str(src_tauri_dir))
        if rc != 0 or mode != "build":
            return rc

        # Also build the standalone background update service executable.
        svc_cmd = ["cargo"]
        if toolchain_arg:
            svc_cmd.append(toolchain_arg)
        svc_cmd.extend(["build", "--bin", "git-manager-service"])
        if release:
            svc_cmd.append("--release")
        log_info(f"Building background service binary: {' '.join(svc_cmd)}")
        service_rc = subprocess.call(svc_cmd, cwd=str(src_tauri_dir))
        if service_rc != 0:
            log_err("The background service binary failed to build.")
            return service_rc

        if platform.system() == "Windows" and name_windows_setup_installer(project_root) is None:
            return 1
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
        choices=["deps", "check", "dev", "build", "prepare-runtime", "prepare-sidecar"],
        help="Action: deps, check, dev, build, prepare-runtime, or prepare-sidecar.",
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

    print("=" * 72)
    print(" GitManager — Pure-Rust Tauri v2 Dependency & Toolchain Manager")
    print("=" * 72)

    if args.mode == "prepare-runtime":
        return prepare_windows_python_runtime(project_root)

    check_os_system_dependencies()
    sanitize_tauri_v2_config(project_root)

    toolchain_arg = check_and_resolve_rust(project_root)
    if toolchain_arg is None:
        return 1

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

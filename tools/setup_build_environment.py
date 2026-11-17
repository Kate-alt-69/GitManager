#!/usr/bin/env python3
"""Install and verify GitManager's native desktop build prerequisites.

This command prepares the current OS. On Windows it can also provision an
Ubuntu 24.04 WSL2 environment for Linux/Flatpak builds. It does not attempt to
install or emulate Apple's macOS SDK on a non-Apple host.
"""

from __future__ import annotations

import os
import platform
import re
import shutil
import subprocess
import tempfile
from pathlib import Path
from typing import Optional, Sequence


WINDOWS_VS_COMPONENTS = (
    "Microsoft.VisualStudio.Workload.VCTools",
    "Microsoft.VisualStudio.Component.Windows11SDK.26100",
)

WINDOWS_WINGET_PACKAGES = (
    ("Rustup", "Rustlang.Rustup"),
    ("Node.js 22", "OpenJS.NodeJS.22"),
    ("Master Packager Dev", "MasterPackager.MasterPackagerDev"),
    ("NSIS", "NSIS.NSIS"),
    ("Microsoft Edge WebView2 Runtime", "Microsoft.EdgeWebView2Runtime"),
)

WINDOWS_RUST_TARGETS = (
    "x86_64-pc-windows-msvc",
    "i686-pc-windows-msvc",
)

UBUNTU_BUILD_PACKAGES = (
    "build-essential",
    "curl",
    "wget",
    "file",
    "pkg-config",
    "libssl-dev",
    "libgtk-3-dev",
    "libwebkit2gtk-4.1-dev",
    "libayatana-appindicator3-dev",
    "librsvg2-dev",
    "libxdo-dev",
    "rpm",
    "patchelf",
    "flatpak",
    "flatpak-builder",
    "dpkg-dev",
    "git",
    "python3",
    "python3-venv",
    "systemd",
    "systemd-sysv",
)

FLATPAK_RUNTIME_REPOSITORY = "https://dl.flathub.org/repo/flathub.flatpakrepo"
FLATPAK_RUNTIME_REF = "org.gnome.Platform//51"
FLATPAK_SDK_REF = "org.gnome.Sdk//51"
UBUNTU_WSL_DISTRIBUTION = "Ubuntu-24.04"


def _log(message: str) -> None:
    print("[SETUP] " + message, flush=True)


def _run(command: Sequence[str], *, capture: bool = False) -> tuple[int, str, str]:
    rendered = " ".join(str(part) for part in command)
    _log("$ " + rendered)
    try:
        result = subprocess.run(
            [str(part) for part in command],
            check=False,
            text=True,
            stdout=subprocess.PIPE if capture else None,
            stderr=subprocess.PIPE if capture else None,
        )
    except FileNotFoundError:
        return 127, "", "Command not found: " + str(command[0])
    return (
        result.returncode,
        (result.stdout or "").strip() if capture else "",
        (result.stderr or "").strip() if capture else "",
    )


def _find_rustup() -> Optional[str]:
    found = shutil.which("rustup")
    if found:
        return found
    if platform.system() == "Windows":
        user_profile = os.environ.get("USERPROFILE")
        if user_profile:
            candidate = Path(user_profile) / ".cargo" / "bin" / "rustup.exe"
            if candidate.is_file():
                return str(candidate)
    else:
        candidate = Path.home() / ".cargo" / "bin" / "rustup"
        if candidate.is_file():
            return str(candidate)
    return None


def _install_rustup_unix() -> int:
    curl = shutil.which("curl")
    shell = shutil.which("sh")
    if not curl or not shell:
        _log("curl and sh are required to install rustup from https://rustup.rs.")
        return 1
    with tempfile.TemporaryDirectory(prefix="gitmanager-rustup-") as temp:
        installer = Path(temp) / "rustup-init.sh"
        code, _, error = _run([
            curl,
            "--proto", "=https",
            "--tlsv1.2",
            "-sSf",
            "https://sh.rustup.rs",
            "-o", str(installer),
        ])
        if code != 0:
            _log("Could not download the official rustup installer: " + error)
            return code
        return _run([shell, str(installer), "-y", "--profile", "minimal"])[0]


def _ensure_rustup_toolchain() -> int:
    """Ensure rustup has an active compiler/toolchain before adding target stdlibs."""
    rustup = _find_rustup()
    if not rustup:
        _log("rustup is not visible in this terminal. Reopen it after installation, then rerun setup-build-tools.")
        return 2

    code, active, _ = _run([rustup, "show", "active-toolchain"], capture=True)
    if code != 0 or not active:
        _log("No active Rust toolchain is configured; installing the minimal stable toolchain.")
        result = _run([rustup, "toolchain", "install", "stable", "--profile", "minimal"])[0]
        if result != 0:
            return result
        result = _run([rustup, "default", "stable"])[0]
        if result != 0:
            return result
        active = "stable"

    cargo = shutil.which("cargo")
    rustc = shutil.which("rustc")
    if not cargo or not rustc:
        _log("Rustup has an active toolchain but cargo/rustc are not visible. Reopen the terminal and rerun setup-build-tools.")
        return 2

    cargo_code, cargo_version, _ = _run([cargo, "--version"], capture=True)
    rustc_code, rustc_version, _ = _run([rustc, "--version"], capture=True)
    if cargo_code != 0 or rustc_code != 0:
        _log("The active Rust toolchain is incomplete. Run rustup toolchain install stable --profile minimal and retry.")
        return 2
    _log(f"Active Rust toolchain verified ({active.strip()}): {rustc_version}; {cargo_version}.")
    return 0


def _node_major(command: str = "node") -> Optional[int]:
    executable = shutil.which(command)
    if not executable:
        return None
    code, output, _ = _run([executable, "--version"], capture=True)
    if code != 0:
        return None
    match = re.search(r"v?(\d+)", output)
    return int(match.group(1)) if match else None


def _winget_install(package_id: str, *, override: Optional[str] = None) -> int:
    winget = shutil.which("winget.exe") or shutil.which("winget")
    if not winget:
        _log("WinGet is required. Install or update Microsoft's App Installer first.")
        return 1

    code, output, _ = _run([
        winget, "list", "--id", package_id, "--exact",
        "--accept-source-agreements",
    ], capture=True)
    if code == 0 and package_id.casefold() in output.casefold():
        _log(package_id + " is already installed.")
        return 0

    command = [
        winget, "install", "--id", package_id, "--exact",
        "--source", "winget",
        "--silent",
        "--accept-package-agreements",
        "--accept-source-agreements",
        "--disable-interactivity",
    ]
    if override:
        command.extend(["--override", override])
    return _run(command)[0]


def _vswhere() -> Optional[str]:
    roots = [
        os.environ.get("ProgramFiles(x86)", ""),
        os.environ.get("ProgramFiles", ""),
    ]
    for root in roots:
        if root:
            candidate = Path(root) / "Microsoft Visual Studio" / "Installer" / "vswhere.exe"
            if candidate.is_file():
                return str(candidate)
    return shutil.which("vswhere.exe") or shutil.which("vswhere")


def _setup_visual_studio_build_tools() -> int:
    """Ensure that the Visual Studio C++ workload and Windows SDK are installed."""
    vswhere = _vswhere()
    install_path: Optional[str] = None
    if vswhere:
        code, output, _ = _run([
            vswhere, "-latest", "-products", "*",
            "-requires", *WINDOWS_VS_COMPONENTS,
            "-property", "installationPath",
        ], capture=True)
        if code == 0 and output:
            install_path = output.splitlines()[0].strip()

    if install_path:
        _log("Visual Studio C++ Build Tools workload detected at " + install_path)
        return 0

    base_path = None
    if vswhere:
        code, output, _ = _run([
            vswhere, "-latest", "-products", "*", "-property", "installationPath",
        ], capture=True)
        if code == 0 and output:
            base_path = output.splitlines()[0].strip()

    setup_exe = None
    program_files_x86 = os.environ.get("ProgramFiles(x86)", "")
    if program_files_x86:
        candidate = Path(program_files_x86) / "Microsoft Visual Studio" / "Installer" / "setup.exe"
        if candidate.is_file():
            setup_exe = str(candidate)

    override_components = [
        "--quiet",
        "--wait",
        "--norestart",
        "--includeRecommended",
    ]
    for component in WINDOWS_VS_COMPONENTS:
        override_components.extend(["--add", component])
    if platform.machine().lower() in ("arm64", "aarch64"):
        override_components.extend([
            "--add", "Microsoft.VisualStudio.Component.VC.Tools.ARM64",
        ])

    if base_path and setup_exe:
        command = [
            setup_exe, "modify", "--installPath", base_path,
            *override_components,
        ]
        result = _run(command)[0]
    else:
        override = " ".join(override_components)
        result = _winget_install("Microsoft.VisualStudio.2022.BuildTools", override=override)

    if result != 0:
        _log("Visual Studio C++ Build Tools installation failed. Install the Desktop development with C++ workload and Windows SDK, then rerun setup.")
        return result

    if vswhere:
        code, output, _ = _run([
            vswhere, "-latest", "-products", "*",
            "-requires", *WINDOWS_VS_COMPONENTS,
            "-property", "installationPath",
        ], capture=True)
        if code == 0 and output:
            _log("Verified Visual Studio C++ workload.")
            return 0

    _log("The Visual Studio installer completed, but the C++ workload could not be verified yet. Reopen this terminal and rerun setup-build-tools.")
    return 2


def _ubuntu_build_setup_script() -> str:
    """Return the idempotent Ubuntu 24.04/WSL build-prerequisite installer."""
    packages = " ".join(UBUNTU_BUILD_PACKAGES)
    return r"""
set -euo pipefail

sudo apt-get update
if apt-cache show libfuse2t64 >/dev/null 2>&1; then
  FUSE_PACKAGE=libfuse2t64
else
  FUSE_PACKAGE=libfuse2
fi
sudo apt-get install -y BUILD_PACKAGES_PLACEHOLDER "$FUSE_PACKAGE"

curl -fsSL https://deb.nodesource.com/setup_22.x -o /tmp/gitmanager-nodesource-setup.sh
sudo bash /tmp/gitmanager-nodesource-setup.sh
sudo apt-get install -y nodejs

if ! command -v rustup >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/gitmanager-rustup-init.sh
  sh /tmp/gitmanager-rustup-init.sh -y --profile minimal
fi
. "$HOME/.cargo/env"
if ! rustup show active-toolchain >/dev/null 2>&1; then
  rustup toolchain install stable --profile minimal
  rustup default stable
fi
case "$(uname -m)" in
  aarch64|arm64) RUST_TARGET=aarch64-unknown-linux-gnu ;;
  armv7l) RUST_TARGET=armv7-unknown-linux-gnueabihf ;;
  *) RUST_TARGET=x86_64-unknown-linux-gnu ;;
esac
rustup target add "$RUST_TARGET"

# Keep the user-level background worker testable in WSL without overwriting
# unrelated distro settings.
sudo python3 - <<'PY'
from pathlib import Path

path = Path("/etc/wsl.conf")
lines = path.read_text(encoding="utf-8").splitlines() if path.exists() else []
boot_index = next(
    (i for i, line in enumerate(lines) if line.strip().casefold() == "[boot]"),
    None,
)
if boot_index is None:
    if lines and lines[-1].strip():
        lines.append("")
    lines.extend(["[boot]", "systemd=true"])
else:
    next_section = next(
        (i for i in range(boot_index + 1, len(lines)) if lines[i].strip().startswith("[") and lines[i].strip().endswith("]")),
        len(lines),
    )
    setting = next(
        (i for i in range(boot_index + 1, next_section) if lines[i].split("=", 1)[0].strip().casefold() == "systemd"),
        None,
    )
    if setting is None:
        lines.insert(next_section, "systemd=true")
    else:
        lines[setting] = "systemd=true"
path.write_text("\n".join(lines).rstrip() + "\n", encoding="utf-8")
print("GITMANAGER_WSL_SYSTEMD_CONFIG=READY")
PY

flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak install --user --noninteractive --assumeyes flathub org.gnome.Platform//51 org.gnome.Sdk//51

echo "GitManager SDK setup: Node $(node --version); Rust $(rustc --version); Flatpak $(flatpak --version)"
if bwrap --unshare-user --unshare-pid --ro-bind / / --proc /proc --dev /dev /usr/bin/true >/dev/null 2>&1; then
  echo "GITMANAGER_BWRAP_CHECK=PASS"
else
  echo "GITMANAGER_BWRAP_CHECK=FAIL"
  echo "WARNING: this Linux environment cannot create the Bubblewrap namespaces required by flatpak-builder."
  echo "Use a native Linux installation or VM for the Flatpak build if the sandbox test continues to fail."
fi
""".replace("BUILD_PACKAGES_PLACEHOLDER", packages)


def _setup_wsl_linux() -> int:
    wsl = shutil.which("wsl.exe") or shutil.which("wsl")
    if not wsl:
        _log("WSL is not installed. Run PowerShell as Administrator and execute: wsl --install --distribution Ubuntu-24.04")
        return 2

    _, output, _ = _run([wsl, "--list", "--quiet"], capture=True)
    normalized = output.replace("\x00", "").replace("\r", "")
    distributions = [line.strip() for line in normalized.splitlines() if line.strip()]
    distro = next((name for name in distributions if name.casefold() == UBUNTU_WSL_DISTRIBUTION.casefold()), None)
    if not distro:
        _log("Installing Ubuntu 24.04 under WSL2 for native Linux/Flatpak builds.")
        install = _run([
            wsl, "--install", "--distribution", UBUNTU_WSL_DISTRIBUTION, "--no-launch",
        ])[0]
        if install != 0:
            _log("WSL installation needs an elevated terminal or a Windows restart. Complete the Ubuntu 24.04 WSL installation, launch Ubuntu once to create its Linux user, then rerun setup-build-tools.")
            return install
        _log("Ubuntu 24.04 is being installed. Restart Windows if requested, launch Ubuntu once from the Start menu to create your Linux username/password, then rerun setup-build-tools.")
        return 2

    probe = _run([
        wsl, "--distribution", distro, "--", "bash", "-lc",
        'test -f /etc/os-release && . /etc/os-release && test "$ID" = ubuntu',
    ])[0]
    if probe != 0:
        _log("Ubuntu is installed but its first-run account setup is incomplete. Launch Ubuntu from the Start menu, create its Linux username/password, then rerun setup-build-tools.")
        return 2

    script = _ubuntu_build_setup_script()
    result = _run([
        wsl, "--distribution", distro, "--", "bash", "-lc", script,
    ])[0]
    if result == 0:
        _log("Ubuntu/WSL build tools and GNOME 51 Flatpak runtime/SDK are installed.")
        _log("systemd is enabled in /etc/wsl.conf. After closing WSL work, run 'wsl --terminate Ubuntu-24.04' once, then reopen Ubuntu to apply it.")
    return result


def _setup_windows() -> int:
    if not (shutil.which("winget.exe") or shutil.which("winget")):
        _log("WinGet was not found. Install or update Microsoft's App Installer, then rerun setup-build-tools.")
        return 1

    failures = []
    vs_result = _setup_visual_studio_build_tools()
    if vs_result != 0:
        failures.append(("Visual Studio C++ Build Tools", vs_result))

    if _node_major() is None or _node_major() < 22:
        result = _winget_install("OpenJS.NodeJS.22")
        if result != 0:
            failures.append(("Node.js 22", result))
    else:
        _log("Node.js 22+ is already available.")

    for display_name, package_id in WINDOWS_WINGET_PACKAGES:
        result = _winget_install(package_id)
        if result != 0:
            failures.append((display_name, result))

    rust_result = _ensure_rustup_toolchain()
    rustup = _find_rustup()
    if rust_result == 0 and rustup:
        targets = list(WINDOWS_RUST_TARGETS)
        if platform.machine().lower() in ("arm64", "aarch64"):
            targets.append("aarch64-pc-windows-msvc")
        result = _run([rustup, "target", "add", *targets])[0]
        if result != 0:
            failures.append(("Windows Rust targets", result))
    else:
        failures.append(("Rustup toolchain verification", rust_result or 2))
        _log("Reopen PowerShell if Rustup was just installed, then rerun setup-build-tools.")

    _log("WebView2 is supplied by supported Windows 10/11 installations; verify it on older/custom Windows images.")

    wsl_result = _setup_wsl_linux()
    if wsl_result != 0:
        failures.append(("Ubuntu 24.04 WSL2 Linux/Flatpak environment", wsl_result))

    for name, result in failures:
        _log(f"{name} did not finish successfully (exit code {result}).")
    if failures:
        return max(result for _, result in failures)
    _log("Windows x64/x86 toolchain and Ubuntu/WSL2 Linux/Flatpak toolchain are ready.")
    _log("Windows ARM64 app builds still require a native Windows ARM64 host in this build matrix.")
    _log("macOS builds still require a real Mac with Apple's Xcode SDK.")
    return 0


def _setup_native_linux() -> int:
    if not shutil.which("apt-get"):
        _log("Automatic Linux setup currently targets Debian/Ubuntu. Install the Tauri WebKitGTK dependencies and Flatpak Builder SDK using your distribution's package manager.")
        return 2
    result = _run(["bash", "-lc", _ubuntu_build_setup_script()])[0]
    if result == 0:
        _log("Linux Tauri dependencies, Node.js 22, Rust target and GNOME 51 Flatpak runtime/SDK are ready.")
    return result


def _setup_macos() -> int:
    code, _, _ = _run(["xcode-select", "-p"], capture=True)
    if code != 0:
        _log("Installing Xcode Command Line Tools. Complete the Apple installer dialog, then rerun setup-build-tools.")
        _run(["xcode-select", "--install"])
        return 2

    brew = shutil.which("brew")
    if brew:
        result = _run([brew, "install", "node@22", "pkg-config"])[0]
        if result != 0:
            return result
        _run([brew, "link", "--overwrite", "--force", "node@22"])
    elif _node_major() is None or _node_major() < 22:
        _log("Homebrew is not installed. Install Node.js 22 from https://nodejs.org/en/download/ and rerun setup.")
        return 2

    rustup = _find_rustup()
    if not rustup:
        result = _install_rustup_unix()
        if result != 0:
            return result
    result = _ensure_rustup_toolchain()
    rustup = _find_rustup()
    if result != 0 or not rustup:
        return result or 2

    result = _run([
        rustup, "target", "add",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
    ])[0]
    if result != 0:
        return result

    _log("Xcode Command Line Tools, Node.js 22 and both macOS Rust targets are ready.")
    _log("Code signing/notarization still requires the appropriate Apple Developer credentials.")
    return 0


def setup_build_environment() -> int:
    """Prepare the current host's toolchain and, on Windows, Ubuntu under WSL2."""
    system = platform.system()
    _log("Preparing build prerequisites for " + system + " (" + platform.machine() + ").")
    if system == "Windows":
        return _setup_windows()
    if system == "Linux":
        return _setup_native_linux()
    if system == "Darwin":
        return _setup_macos()
    _log("Unsupported build host: " + system)
    return 2


def main() -> int:
    return setup_build_environment()


if __name__ == "__main__":
    raise SystemExit(main())

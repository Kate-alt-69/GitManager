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

try:
    from tools.build_targets import BuildTarget, can_build_locally, host_platform, parse_build_selection
except ModuleNotFoundError:
    from build_targets import BuildTarget, can_build_locally, host_platform, parse_build_selection


WINDOWS_VS_COMPONENTS = (
    "Microsoft.VisualStudio.Workload.VCTools",
    "Microsoft.VisualStudio.Component.Windows11SDK.26100",
)

WINDOWS_WINGET_PACKAGES = (
    ("Node.js 22", "OpenJS.NodeJS.22"),
    ("Master Packager Dev", "MasterPackager.MasterPackagerDev"),
    ("NSIS", "NSIS.NSIS"),
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
    "dpkg-dev",
    "git",
    "python3",
    "python3-venv",
    "systemd",
    "systemd-sysv",
)

FLATPAK_BUILD_PACKAGES = ("flatpak", "flatpak-builder")

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


def _find_executable(name: str) -> Optional[str]:
    found = shutil.which(name)
    if found:
        return found
    if platform.system() == "Windows":
        profile = os.environ.get("USERPROFILE")
        if profile:
            for candidate in (
                Path(profile) / ".cargo" / "bin" / (name + ".exe"),
                Path(profile) / ".cargo" / "bin" / name,
            ):
                if candidate.is_file():
                    return str(candidate)
    else:
        candidate = Path.home() / ".cargo" / "bin" / name
        if candidate.is_file():
            return str(candidate)
    return None


def _confirm_install(component: str, target: str, *, default_yes: bool) -> bool:
    """Ask before changing the machine; explicitly requested targets default to Yes."""
    answer_label = "Y/n" if default_yes else "y/N"
    while True:
        try:
            answer = input(
                f"Do you want to install {component} to build {target}? [{answer_label}] "
            ).strip().casefold()
        except (EOFError, OSError):
            answer = ""
        if not answer:
            return default_yes
        if answer in ("y", "yes"):
            return True
        if answer in ("n", "no"):
            return False
        print("Please answer yes or no.", flush=True)


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

    cargo = _find_executable("cargo")
    rustc = _find_executable("rustc")
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


def _winget_is_installed(package_id: str) -> bool:
    winget = shutil.which("winget.exe") or shutil.which("winget")
    if not winget:
        return False
    code, output, _ = _run([
        winget, "list", "--id", package_id, "--exact",
        "--accept-source-agreements",
    ], capture=True)
    return code == 0 and package_id.casefold() in output.casefold()


def _windows_package_available(package_id: str) -> bool:
    """Detect required native tools even when they were installed outside WinGet."""
    if _winget_is_installed(package_id):
        return True
    if package_id == "OpenJS.NodeJS.22":
        return (_node_major() or 0) >= 22

    roots = []
    for env_name in ("ProgramFiles(x86)", "ProgramFiles"):
        value = os.environ.get(env_name)
        if value:
            roots.append(Path(value))
    local_app_data = os.environ.get("LOCALAPPDATA")
    if local_app_data:
        roots.extend([
            Path(local_app_data) / "Programs",
            Path(local_app_data) / "Microsoft" / "WinGet" / "Links",
        ])

    candidates = []
    if package_id == "MasterPackager.MasterPackagerDev":
        for root in roots:
            candidates.extend([
                root / "Master Packager Ltd" / "Master Packager Dev" / "mpdev.exe",
                root / "Master Packager" / "Master Packager Dev" / "mpdev.exe",
                root / "Master Packager Dev" / "mpdev.exe",
                root / "mpdev.exe",
            ])
        return bool(shutil.which("mpdev") or shutil.which("mpdev.exe") or any(p.is_file() for p in candidates))

    if package_id == "NSIS.NSIS":
        for root in roots:
            candidates.extend([
                root / "NSIS" / "makensis.exe",
                root / "NSIS" / "Bin" / "makensis.exe",
                root / "makensis.exe",
            ])
        return bool(shutil.which("makensis") or shutil.which("makensis.exe") or any(p.is_file() for p in candidates))

    return False


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


def _ubuntu_environment_probe_script(include_flatpak: bool) -> str:
    """Return a non-mutating probe for required Ubuntu packages and SDKs."""
    packages = list(UBUNTU_BUILD_PACKAGES)
    if include_flatpak:
        packages.extend(FLATPAK_BUILD_PACKAGES)
    required = " ".join(packages)
    flatpak_probe = ""
    if include_flatpak:
        flatpak_probe = r"""
command -v flatpak >/dev/null 2>&1 || missing+=(flatpak)
command -v flatpak-builder >/dev/null 2>&1 || missing+=(flatpak-builder)
flatpak info --user org.gnome.Platform//51 >/dev/null 2>&1 || missing+=(org.gnome.Platform//51)
flatpak info --user org.gnome.Sdk//51 >/dev/null 2>&1 || missing+=(org.gnome.Sdk//51)
command -v bwrap >/dev/null 2>&1 || missing+=(bubblewrap)
"""
    return r"""
set -u
missing=()
for package in REQUIRED_PACKAGES; do
  dpkg-query -W -f='${Status}' "$package" 2>/dev/null | grep -q 'install ok installed' || missing+=("$package")
done
node_major=0
if command -v node >/dev/null 2>&1; then
  node_major="$(node -p 'Number(process.versions.node.split(".")[0])' 2>/dev/null || echo 0)"
fi
if [ "$node_major" -lt 22 ]; then missing+=(Node.js-22); fi
if ! command -v cargo >/dev/null 2>&1 || ! command -v rustc >/dev/null 2>&1; then
  missing+=(Rust-toolchain)
fi
FLATPAK_PROBE
if [ "${#missing[@]}" -gt 0 ]; then
  printf 'MISSING_BUILD_TOOLS=%s\n' "$(IFS=,; echo "${missing[*]}")"
  exit 1
fi
echo "BUILD_TOOLS_READY=1"
""".replace("REQUIRED_PACKAGES", required).replace("FLATPAK_PROBE", flatpak_probe)


def _ubuntu_build_setup_script(*, include_flatpak: bool, configure_wsl: bool) -> str:
    """Return an idempotent Ubuntu build-prerequisite installer."""
    packages = list(UBUNTU_BUILD_PACKAGES)
    if include_flatpak:
        packages.extend(FLATPAK_BUILD_PACKAGES)
    package_args = " ".join(packages)

    node_setup = r"""
node_major=0
if command -v node >/dev/null 2>&1; then
  node_major="$(node -p 'Number(process.versions.node.split(".")[0])' 2>/dev/null || echo 0)"
fi
if [ "$node_major" -lt 22 ]; then
  curl -fsSL https://deb.nodesource.com/setup_22.x -o /tmp/gitmanager-nodesource-setup.sh
  sudo bash /tmp/gitmanager-nodesource-setup.sh
  sudo apt-get install -y nodejs
fi
"""
    rust_setup = r"""
# Load rustup's normal user PATH only when it exists; preserve distro/system
# Rust installations and never reset a working compiler's default toolchain.
if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
if ! command -v cargo >/dev/null 2>&1 || ! command -v rustc >/dev/null 2>&1; then
  if ! command -v rustup >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/gitmanager-rustup-init.sh
    sh /tmp/gitmanager-rustup-init.sh -y --profile minimal
    . "$HOME/.cargo/env"
  fi
  if ! command -v cargo >/dev/null 2>&1 || ! command -v rustc >/dev/null 2>&1; then
    rustup toolchain install stable --profile minimal
    rustup default stable
  fi
fi
if command -v rustc >/dev/null 2>&1; then
  echo "Using existing Rust toolchain: $(rustc --version)"
fi
"""
    wsl_config = ""
    if configure_wsl:
        wsl_config = r"""
# Enable WSL systemd while preserving unrelated settings.
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
"""
    flatpak_setup = ""
    if include_flatpak:
        flatpak_setup = r"""
flatpak remote-add --user --if-not-exists flathub ${FLATPAK_RUNTIME_REPOSITORY}
flatpak install --user --noninteractive --assumeyes flathub org.gnome.Platform//51 org.gnome.Sdk//51
if ! bwrap --unshare-user --unshare-pid --ro-bind / / --proc /proc --dev /dev /usr/bin/true >/dev/null 2>&1; then
  echo "Flatpak Builder cannot create the user namespaces required by Bubblewrap in this environment." >&2
  echo "Use a native Linux installation or a VM for Flatpak packaging." >&2
  exit 3
fi
"""
    return (
        "set -euo pipefail\n"
        "sudo apt-get update\n"
        "if apt-cache show libfuse2t64 >/dev/null 2>&1; then FUSE_PACKAGE=libfuse2t64; else FUSE_PACKAGE=libfuse2; fi\n"
        f"sudo apt-get install -y {package_args} \"$FUSE_PACKAGE\"\n"
        + node_setup
        + rust_setup
        + wsl_config
        + flatpak_setup.replace("${FLATPAK_RUNTIME_REPOSITORY}", FLATPAK_RUNTIME_REPOSITORY)
        + 'echo "GitManager build prerequisites: Node $(node --version 2>/dev/null || echo missing); Rust $(rustc --version 2>/dev/null || echo missing)"\n'
    )


def _ubuntu_environment_ready(
    command_prefix: Sequence[str],
    *,
    include_flatpak: bool,
) -> bool:
    script = _ubuntu_environment_probe_script(include_flatpak)
    code, output, error = _run(
        [*command_prefix, "bash", "-lc", script],
        capture=True,
    )
    if code == 0 and "BUILD_TOOLS_READY=1" in output:
        return True
    if output:
        _log(output)
    elif error:
        _log(error)
    return False


def _setup_wsl_linux(*, include_flatpak: bool, default_yes: bool) -> int:
    wsl = shutil.which("wsl.exe") or shutil.which("wsl")
    if not wsl:
        _log("WSL was not found. It is required to build Linux from this Windows host.")
        if not _confirm_install(
            "WSL2 and Ubuntu 24.04",
            "Linux" + ("/Flatpak" if include_flatpak else ""),
            default_yes=default_yes,
        ):
            _log("Skipped WSL2 installation; the existing Windows Rust toolchain has been left unchanged.")
            return 2
        _log("Run PowerShell as Administrator if Windows requests elevation.")
        result = _run([
            "wsl", "--install", "--distribution", UBUNTU_WSL_DISTRIBUTION, "--no-launch",
        ])[0]
        if result == 0:
            _log("WSL/Ubuntu installation has started. Restart Windows if requested, launch Ubuntu once to create its Linux user, then rerun setup-build-tools.")
        return result or 2

    _, output, _ = _run([wsl, "--list", "--quiet"], capture=True)
    normalized = output.replace("\x00", "").replace("\r", "")
    distributions = [line.strip() for line in normalized.splitlines() if line.strip()]
    distro = next(
        (name for name in distributions if name.casefold() == UBUNTU_WSL_DISTRIBUTION.casefold()),
        None,
    )
    if not distro:
        if not _confirm_install(
            "Ubuntu 24.04 under WSL2",
            "Linux" + ("/Flatpak" if include_flatpak else ""),
            default_yes=default_yes,
        ):
            _log("Skipped Ubuntu installation; the existing Windows Rust toolchain has been left unchanged.")
            return 2
        result = _run([
            wsl, "--install", "--distribution", UBUNTU_WSL_DISTRIBUTION, "--no-launch",
        ])[0]
        if result == 0:
            _log("Ubuntu 24.04 is installing. Restart Windows if requested, launch Ubuntu once to create your Linux username/password, then rerun setup-build-tools.")
        return result or 2

    prefix = [wsl, "--distribution", distro, "--"]
    probe = _run([
        *prefix, "bash", "-lc",
        'test -f /etc/os-release && . /etc/os-release && test "$ID" = ubuntu',
    ])[0]
    if probe != 0:
        _log("Ubuntu is installed but its first-run account setup is incomplete. Launch Ubuntu from the Start menu, create its Linux username/password, then rerun setup-build-tools.")
        return 2

    if _ubuntu_environment_ready(prefix, include_flatpak=include_flatpak):
        _log("Linux prerequisites are already installed. Reusing the existing Rust toolchain; no compiler reinstall is needed.")
        return 0

    package_description = (
        "Tauri Linux libraries, Node.js 22, and Flatpak Builder with GNOME 51 runtime/SDK"
        if include_flatpak
        else "Tauri GTK/WebKitGTK libraries, Node.js 22, and Linux build prerequisites"
    )
    target = "Linux/Flatpak" if include_flatpak else "Linux"
    if not _confirm_install(package_description, target, default_yes=default_yes):
        _log("Installation declined. Existing Rust and SDK components were left untouched.")
        return 2

    script = _ubuntu_build_setup_script(
        include_flatpak=include_flatpak,
        configure_wsl=True,
    )
    result = _run([*prefix, "bash", "-lc", script])[0]
    if result == 0:
        _log("Ubuntu/WSL build tools are ready.")
        if include_flatpak:
            _log("GNOME 51 Flatpak runtime/SDK installed and Bubblewrap namespace probe passed.")
        _log("systemd is enabled in /etc/wsl.conf. Run 'wsl --terminate Ubuntu-24.04' after closing WSL work, then reopen Ubuntu to apply it.")
    elif result == 3:
        _log("Linux desktop builds may work, but Flatpak packaging is not supported by this WSL namespace configuration. Use native Linux or a VM for Flatpak.")
    return result


def _visual_studio_ready() -> bool:
    vswhere = _vswhere()
    if not vswhere:
        return False
    code, output, _ = _run([
        vswhere, "-latest", "-products", "*",
        "-requires", *WINDOWS_VS_COMPONENTS,
        "-property", "installationPath",
    ], capture=True)
    return code == 0 and bool(output.strip())


def _ensure_windows_rust(targets: Sequence[BuildTarget], *, default_yes: bool) -> int:
    cargo = _find_executable("cargo")
    rustc = _find_executable("rustc")
    if not cargo or not rustc:
        if not _confirm_install("Rustup and the Rust stable toolchain", "Windows", default_yes=default_yes):
            _log("Rust installation skipped. No existing cargo/rustc compiler was found.")
            return 2
        result = _winget_install("Rustlang.Rustup")
        if result != 0:
            return result
        cargo = _find_executable("cargo")
        rustc = _find_executable("rustc")
        if not cargo or not rustc:
            rustup = _find_rustup()
            if rustup:
                result = _ensure_rustup_toolchain()
                if result != 0:
                    return result
                cargo = _find_executable("cargo")
                rustc = _find_executable("rustc")
        if not cargo or not rustc:
            _log("Rustup was installed but cargo/rustc are not yet visible. Reopen PowerShell and rerun setup-build-tools.")
            return 2

    code, version, _ = _run([rustc, "--version"], capture=True)
    if code != 0:
        return 2
    _log(f"Using existing Rust compiler: {version}. No Rust reinstall will be performed.")

    rustup = _find_rustup()
    if not rustup:
        _log("rustup is not managing the existing compiler; preserving it without changing the toolchain.")
        return 0

    code, installed_output, _ = _run([rustup, "target", "list", "--installed"], capture=True)
    if code != 0:
        return 0
    installed = set(installed_output.splitlines())
    missing = sorted({target.triple for target in targets if target.triple not in installed})
    if not missing:
        _log("All requested Windows Rust target components are already installed.")
        return 0
    if not _confirm_install(
        "Rust target component(s) " + ", ".join(missing),
        "Windows",
        default_yes=default_yes,
    ):
        _log("Rust targets were not installed; the current Rust toolchain was left unchanged.")
        return 2
    return _run([rustup, "target", "add", *missing])[0]


def _setup_windows(
    targets: Sequence[BuildTarget],
    *,
    default_yes: bool,
) -> int:
    winget = shutil.which("winget.exe") or shutil.which("winget")
    failures = []
    target_description = "Windows MSI/MSIX and NSIS installers"

    if not _visual_studio_ready():
        if _confirm_install(
            "Visual Studio C++ Build Tools and the Windows SDK",
            target_description,
            default_yes=default_yes,
        ):
            result = _setup_visual_studio_build_tools()
            if result != 0:
                failures.append(("Visual Studio C++ Build Tools", result))
        else:
            failures.append(("Visual Studio C++ Build Tools (declined)", 2))
    else:
        _log("Visual Studio C++ Build Tools and the required Windows SDK are already installed.")

    if _node_major() is None or _node_major() < 22:
        if _confirm_install("Node.js 22", "the GitManager frontend for Windows", default_yes=default_yes):
            result = _winget_install("OpenJS.NodeJS.22")
            if result != 0:
                failures.append(("Node.js 22", result))
        else:
            failures.append(("Node.js 22 (declined)", 2))
    else:
        _log("Node.js 22+ is already available.")

    for display_name, package_id in WINDOWS_WINGET_PACKAGES:
        if _windows_package_available(package_id):
            _log(display_name + " is already installed and discoverable.")
            continue
        if not _confirm_install(display_name, target_description, default_yes=default_yes):
            failures.append((display_name + " (declined)", 2))
            continue
        if not winget:
            _log("WinGet is unavailable, so " + display_name + " cannot be installed automatically. Install Microsoft's App Installer or install the package manually.")
            failures.append((display_name + " (WinGet unavailable)", 1))
            continue
        result = _winget_install(package_id)
        if result != 0:
            failures.append((display_name, result))

    rust_result = _ensure_windows_rust(targets, default_yes=default_yes)
    if rust_result != 0:
        failures.append(("Windows Rust toolchain/targets", rust_result))

    _log("WebView2 is supplied by supported Windows 10/11 installations (Windows 10 version 1803+).")
    if failures:
        for name, result in failures:
            _log(f"{name} was not prepared (exit code {result}).")
        return max(result for _, result in failures)

    _log("Windows build prerequisites are ready. Existing Rust was reused.")
    return 0


def _setup_native_linux(
    *,
    targets: Sequence[BuildTarget],
    include_flatpak: bool,
    default_yes: bool,
) -> int:
    if not shutil.which("apt-get"):
        _log("Automatic Linux setup currently targets Debian/Ubuntu. Install the Tauri WebKitGTK dependencies and Flatpak Builder SDK using your distribution's package manager.")
        return 2

    if _ubuntu_environment_ready([], include_flatpak=include_flatpak):
        _log("Linux prerequisites are already installed. Reusing the existing Rust toolchain; no compiler reinstall is needed.")
        return 0

    package_description = (
        "Tauri GTK/WebKitGTK libraries, Node.js 22, Rust, and GNOME 51 Flatpak runtime/SDK"
        if include_flatpak
        else "Tauri GTK/WebKitGTK libraries, Node.js 22, and Linux build prerequisites"
    )
    target_name = "Linux/Flatpak" if include_flatpak else "Linux"
    if not _confirm_install(package_description, target_name, default_yes=default_yes):
        _log("Installation declined. Existing Rust and system packages were left untouched.")
        return 2

    result = _run([
        "bash", "-lc",
        _ubuntu_build_setup_script(include_flatpak=include_flatpak, configure_wsl=False),
    ])[0]
    if result == 0:
        _log("Native Linux build prerequisites are ready.")
    elif result == 3:
        _log("Flatpak Builder requires Bubblewrap user namespaces. Use a Linux kernel/configuration that supports unprivileged user namespaces.")
    return result


def _setup_macos(
    targets: Sequence[BuildTarget],
    *,
    default_yes: bool,
) -> int:
    code, _, _ = _run(["xcode-select", "-p"], capture=True)
    if code != 0:
        if not _confirm_install("Apple Xcode Command Line Tools", "macOS", default_yes=default_yes):
            _log("Xcode Command Line Tools were not installed.")
            return 2
        _log("Starting Apple's official Command Line Tools installer. Complete its dialog, then rerun setup-build-tools.")
        _run(["xcode-select", "--install"])
        return 2

    brew = shutil.which("brew")
    if _node_major() is None or _node_major() < 22:
        if not brew:
            _log("Node.js 22 is missing. Install it from https://nodejs.org/en/download/ and rerun setup-build-tools.")
            return 2
        if _confirm_install("Node.js 22 and pkg-config through Homebrew", "the GitManager frontend for macOS", default_yes=default_yes):
            result = _run([brew, "install", "node@22", "pkg-config"])[0]
            if result != 0:
                return result
            _run([brew, "link", "--overwrite", "--force", "node@22"])
        else:
            return 2
    elif not shutil.which("pkg-config"):
        if not brew or not _confirm_install("pkg-config through Homebrew", "Tauri on macOS", default_yes=default_yes):
            return 2
        result = _run([brew, "install", "pkg-config"])[0]
        if result != 0:
            return result

    cargo = _find_executable("cargo")
    rustc = _find_executable("rustc")
    rustup = _find_rustup()
    if not cargo or not rustc:
        if not _confirm_install("Rustup and the Rust stable toolchain", "macOS", default_yes=default_yes):
            return 2
        result = _install_rustup_unix()
        if result != 0:
            return result
        result = _ensure_rustup_toolchain()
        if result != 0:
            return result
        rustup = _find_rustup()
    else:
        _log("Using existing Rust compiler: " + _run([rustc, "--version"], capture=True)[1] + ".")

    if not rustup:
        _log("The existing Rust installation is not managed by rustup. Its native host target will be used unchanged.")
        return 0

    code, output, _ = _run([rustup, "target", "list", "--installed"], capture=True)
    if code != 0:
        return 0
    installed = set(output.splitlines())
    missing = sorted({target.triple for target in targets if target.triple not in installed})
    if missing:
        if not _confirm_install(
            "Rust target component(s) " + ", ".join(missing),
            "macOS",
            default_yes=default_yes,
        ):
            _log("The current Rust toolchain was preserved; extra architecture targets were not installed.")
            return 2
        result = _run([rustup, "target", "add", *missing])[0]
        if result != 0:
            return result

    _log("macOS build prerequisites are ready. Use the Mac's native Xcode SDK for packaging.")
    _log("Code signing/notarization still requires the appropriate Apple Developer credentials.")
    return 0


def setup_build_environment(
    target_selection: Optional[str] = None,
    store_target: Optional[str] = None,
) -> int:
    """Prepare only requested target SDKs; reuse installed Rust/toolchains."""
    system = platform.system()
    host = host_platform()
    explicit_request = target_selection is not None or store_target is not None
    all_platform_request = target_selection == "all" or store_target == "all"

    if store_target == "flatpak":
        selected_platforms = {"linux"}
    elif store_target == "msix":
        selected_platforms = {"windows"}
    elif store_target == "all":
        selected_platforms = {"linux", "windows"}
    elif target_selection:
        _, parsed = parse_build_selection(target_selection)
        selected_platforms = {target.platform for target in parsed}
    else:
        selected_platforms = {host}

    if target_selection:
        _, targets = parse_build_selection(target_selection)
    elif store_target == "flatpak":
        _, targets = parse_build_selection("linux")
    elif store_target == "msix":
        _, targets = parse_build_selection("windows")
    elif store_target == "all":
        _, targets = parse_build_selection("all")
    else:
        _, targets = parse_build_selection(host)

    local_targets = []
    for target in targets:
        allowed, reason = can_build_locally(target)
        if allowed:
            local_targets.append(target)
        elif host == "windows" and target.platform == "linux":
            windows_machine = platform.machine().lower()
            wsl_arch = {
                "amd64": "x64", "x86_64": "x64",
                "arm64": "arm64", "aarch64": "arm64",
            }.get(windows_machine, windows_machine)
            if target.architecture == wsl_arch:
                local_targets.append(target)
            elif target.platform in selected_platforms:
                _log(
                    f"Linux {target.architecture} is not available through this WSL host ({wsl_arch}); "
                    "use a matching native Linux machine for that architecture."
                )
        elif target.platform in selected_platforms:
            _log("Target architecture is not available on this machine: " + reason)

    _log("Requested build platforms: " + ", ".join(sorted(selected_platforms)))
    _log("Preparing prerequisites on " + system + " (" + platform.machine() + ").")

    outcomes = []
    if "windows" in selected_platforms:
        if host == "windows":
            windows_targets = [target for target in local_targets if target.platform == "windows"]
            if not windows_targets:
                _log("No selected Windows architecture can be built on this host. Use native Windows ARM64 for ARM64 packages or Windows x64 for x64/x86.")
                outcomes.append(2)
            else:
                outcomes.append(_setup_windows(windows_targets, default_yes=True))
        elif all_platform_request:
            _log("Windows SDK/MSVC/MPDEV setup skipped: run setup-build-tools on a native Windows machine.")
        else:
            _log("Windows SDK/MSVC/MPDEV installs require a native Windows host. Run setup-build-tools on Windows.")
            outcomes.append(2)

    if "linux" in selected_platforms:
        include_flatpak = store_target in ("flatpak", "all")
        linux_targets = [target for target in local_targets if target.platform == "linux"]
        if not linux_targets:
            if all_platform_request and host not in ("windows", "linux"):
                _log("Linux/Flatpak setup skipped: use a native Linux machine or prepare Ubuntu WSL2 from Windows.")
            else:
                _log("No selected Linux architecture matches this host/WSL architecture. Use a matching native Linux machine for that target.")
                outcomes.append(2)
        elif host == "windows":
            outcomes.append(_setup_wsl_linux(include_flatpak=include_flatpak, default_yes=explicit_request))
        elif host == "linux":
            outcomes.append(_setup_native_linux(
                targets=linux_targets,
                include_flatpak=include_flatpak,
                default_yes=True,
            ))
        elif all_platform_request:
            _log("Linux/Flatpak setup skipped: use a native Linux machine or prepare Ubuntu WSL2 from Windows.")
        else:
            _log("Linux/Flatpak packaging needs a native Linux host or an Ubuntu WSL2 environment on Windows.")
            outcomes.append(2)

    if "macos" in selected_platforms:
        mac_targets = [target for target in targets if target.platform == "macos"]
        if host == "macos":
            outcomes.append(_setup_macos(mac_targets, default_yes=True))
        elif all_platform_request:
            _log("macOS setup skipped: Apple's SDK requires a real Mac with Xcode Command Line Tools.")
        else:
            _log("Apple's macOS SDK cannot be installed on this host. Build macOS packages on a real Mac with Xcode Command Line Tools.")
            outcomes.append(2)

    if outcomes and any(result != 0 for result in outcomes):
        return max(result for result in outcomes)
    _log("All requested host-supported build prerequisites are ready. Existing Rust toolchains were preserved.")
    return 0


def main() -> int:
    return setup_build_environment()


if __name__ == "__main__":
    raise SystemExit(main())

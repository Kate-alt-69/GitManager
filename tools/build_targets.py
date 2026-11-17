"""Target selection helpers for GitManager's cross-platform desktop builds.

The desktop support matrix follows the targets supported by the Tauri/Rust
application, not every architecture on which the Git command-line tool runs.
Platform-wide selections are delegated to build-matrix.yml so each target is
built on an appropriate GitHub Actions runner.
"""
from __future__ import annotations

from dataclasses import dataclass
import platform
import re
from typing import Iterable


@dataclass(frozen=True)
class BuildTarget:
    platform: str
    architecture: str
    triple: str
    windows_version: str | None = None

    @property
    def selector(self) -> str:
        if self.platform == "windows":
            version = self.windows_version or "10"
            return f"windows({version},{self.architecture})"
        return f"{self.platform}({self.architecture})"

    @property
    def output_name(self) -> str:
        return f"{self.platform}-{self.architecture}"


TARGETS = {
    ("windows", "x64"): "x86_64-pc-windows-msvc",
    ("windows", "x86"): "i686-pc-windows-msvc",
    ("windows", "arm64"): "aarch64-pc-windows-msvc",
    ("linux", "x64"): "x86_64-unknown-linux-gnu",
    ("linux", "arm64"): "aarch64-unknown-linux-gnu",
    ("linux", "armv7"): "armv7-unknown-linux-gnueabihf",
    ("macos", "x64"): "x86_64-apple-darwin",
    ("macos", "arm64"): "aarch64-apple-darwin",
}

SUPPORTED_ARCHITECTURES = {
    "windows": ("x64", "x86", "arm64"),
    "linux": ("x64", "arm64", "armv7"),
    "macos": ("x64", "arm64"),
}


def parse_build_selection(value: str) -> tuple[str, list[BuildTarget]]:
    """Return (kind, targets), where kind is exact, group, or all."""
    raw = (value or "all").strip().lower()
    if raw == "all":
        return "all", all_targets()
    if raw in SUPPORTED_ARCHITECTURES:
        return "group", [target for target in all_targets() if target.platform == raw]

    match = re.fullmatch(r"(windows|linux|macos)\(([^()]*)\)", raw)
    if not match:
        raise ValueError(
            "Invalid --build value. Use all, windows, linux, macos, "
            "or a target such as windows(10,x64), windows(11,x86), "
            "linux(arm64), or macos(x64)."
        )

    family, raw_parts = match.groups()
    parts = [part.strip() for part in raw_parts.split(",") if part.strip()]
    windows_version = None
    if family == "windows" and len(parts) == 2 and parts[0] in ("10", "11"):
        windows_version, architecture = parts
    elif len(parts) == 1:
        architecture = parts[0]
    else:
        raise ValueError(
            "Windows selectors may include an OS version and architecture, "
            "for example windows(10,x64) or windows(11,arm64)."
        )

    if architecture not in SUPPORTED_ARCHITECTURES[family]:
        choices = ", ".join(SUPPORTED_ARCHITECTURES[family])
        raise ValueError(f"Unsupported {family} architecture '{architecture}'. Supported: {choices}.")

    target = BuildTarget(
        platform=family,
        architecture=architecture,
        triple=TARGETS[(family, architecture)],
        windows_version=windows_version,
    )
    return "exact", [target]


def all_targets() -> list[BuildTarget]:
    targets: list[BuildTarget] = []
    for family, architectures in SUPPORTED_ARCHITECTURES.items():
        for architecture in architectures:
            targets.append(BuildTarget(family, architecture, TARGETS[(family, architecture)]))
    return targets


def host_platform() -> str:
    system = platform.system().lower()
    return "macos" if system == "darwin" else system


def can_build_locally(target: BuildTarget) -> tuple[bool, str]:
    host = host_platform()
    if host != target.platform:
        return False, (
            f"{target.selector} requires a {target.platform} build runner; "
            f"this machine is {host}. Use --build={target.platform} to dispatch the platform group."
        )

    machine = platform.machine().lower()
    aliases = {
        "amd64": "x64", "x86_64": "x64",
        "i386": "x86", "i686": "x86", "x86": "x86",
        "aarch64": "arm64", "arm64": "arm64", "armv7l": "armv7",
    }
    host_arch = aliases.get(machine, machine)
    if target.platform == "linux" and target.architecture != host_arch:
        return False, (
            f"Cross-compiling Linux {target.architecture} from {host_arch} needs a matching "
            "native linker and WebKitGTK development sysroot. Use the Linux ARM64 workflow runner."
        )
    if target.platform == "macos" and target.architecture not in ("x64", "arm64"):
        return False, f"Unsupported macOS host architecture: {host_arch}."
    if target.platform == "windows" and target.architecture == "arm64" and host_arch != "arm64":
        return False, (
            "Windows ARM64 requires the Visual Studio ARM64 C++ toolchain. "
            "Use the Windows ARM64 workflow runner or install that toolchain first."
        )
    if target.platform == "windows" and host_arch == "arm64" and target.architecture != "arm64":
        return False, (
            "The Windows ARM64 runner is configured for native ARM64 builds. "
            "Use the Windows x64 runner for x64 and x86 application targets."
        )
    return True, ""


def describe_targets(targets: Iterable[BuildTarget]) -> str:
    return ", ".join(target.selector for target in targets)

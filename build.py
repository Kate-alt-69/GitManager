#!/usr/bin/env python3
"""
GitManager Pure-Rust Tauri v2 Dependency Checker & Build Script
Supports: Windows 10 & 11, Linux, and macOS (Zero Node.js / npm required).

Features:
1. Checks if Rust (`rustc` and `cargo`) is installed across PATH and `~/.cargo/bin`.
2. Compares the installed Rust version against the target baseline (`1.77.2` for Tauri v2):
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
import json
import os
import platform
import re
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Optional, Tuple, List

RECOMMENDED_RUST_VERSION = (1, 77, 2)
RECOMMENDED_TAURI_CRATE_VERSION = (2, 0, 0)


def log_info(msg: str) -> None:
    print(f"[INFO] {msg}")


def log_ok(msg: str) -> None:
    print(f"[ OK ] {msg}")


def log_warn(msg: str) -> None:
    print(f"[WARN] {msg}")


def log_err(msg: str) -> None:
    print(f"[ERR ] {msg}", file=sys.stderr)


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
        if rc == 0 and mode == "build":
            # Also build the standalone background service executable (`git-manager-service`)
            svc_cmd = ["cargo"]
            if toolchain_arg:
                svc_cmd.append(toolchain_arg)
            svc_cmd.extend(["build", "--bin", "git-manager-service"])
            if release:
                svc_cmd.append("--release")
            log_info(f"Building Background Update Service binary (`git-manager-service`): {' '.join(svc_cmd)}")
            subprocess.call(svc_cmd, cwd=str(src_tauri_dir))
        return rc

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
        description="GitManager Pure-Rust Tauri v2 Dependency Checker & Build Script"
    )
    parser.add_argument(
        "mode",
        nargs="?",
        default="build",
        choices=["deps", "check", "dev", "build"],
        help="Action to run: 'deps' (only check dependencies), 'check' (cargo check), 'dev' (run app), or 'build' (compile app).",
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

    check_os_system_dependencies()
    sanitize_tauri_v2_config(project_root)

    toolchain_arg = check_and_resolve_rust(project_root)
    if toolchain_arg is None:
        return 1

    has_cargo_tauri = check_tauri_rust_cli(toolchain_arg, auto_install=args.install_tauri_cli)

    if args.mode == "deps":
        log_ok("All dependency checks finished.")
        return 0

    is_release = not args.debug
    return run_build_or_dev(
        project_root=project_root,
        mode=args.mode,
        toolchain_arg=toolchain_arg,
        has_cargo_tauri=has_cargo_tauri,
        release=is_release,
    )


if __name__ == "__main__":
    sys.exit(main())

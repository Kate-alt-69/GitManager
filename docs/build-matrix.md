# GitManager local build targets

GitManager is a native Tauri desktop application. A full desktop installer must be compiled on a machine running the target operating system with its native SDK and WebView dependencies. The build script therefore compiles locally; it does not dispatch build requests to GitHub Actions or download remote build artifacts.

## Supported targets

| Platform | Architecture | Rust target | Local build requirements |
| --- | --- | --- | --- |
| Windows | x64 | `x86_64-pc-windows-msvc` | Windows x64, MSVC x64 tools |
| Windows | x86 | `i686-pc-windows-msvc` | Windows x64 with x86 MSVC tools |
| Windows | ARM64 | `aarch64-pc-windows-msvc` | Windows ARM64 or an installed MSVC ARM64 cross-toolchain |
| Linux | x64 | `x86_64-unknown-linux-gnu` | Native x64 Linux, GTK/WebKitGTK development libraries |
| Linux | ARM64 | `aarch64-unknown-linux-gnu` | Native ARM64 Linux and GTK/WebKitGTK development libraries |
| Linux | ARMv7 | `armv7-unknown-linux-gnueabihf` | Native ARMv7 Linux and matching WebKitGTK development libraries |
| macOS | Intel x64 | `x86_64-apple-darwin` | macOS with Xcode command-line tools and SDK |
| macOS | Apple Silicon ARM64 | `aarch64-apple-darwin` | macOS with Xcode command-line tools and SDK |

A Windows computer cannot create a macOS app bundle, and a Linux computer cannot create a Windows installer. The `all` selection means all architectures that the current host can build; other operating systems are reported as skipped. The script never tries GitHub Actions as a fallback.

Windows 10 and Windows 11 are compatibility labels, not different CPU targets. For example, `windows(10,x64)` and `windows(11,x64)` compile the same x64 executable. ARM64 targets are labelled Windows 11.

Linux ARMv7 is limited to DEB/RPM packaging in this build stream; AppImage is not produced for that target. Cross-architecture Linux builds need a native linker and matching WebKitGTK sysroot, so the script does not pretend a generic x64 Linux host can package ARM builds.

## Toolchain setup

Run `py build.py setup-build-tools` to prepare only the current host. Use `py build.py setup-build-tools --build=all` to prepare every target environment this machine can support: Windows also prepares matching Linux x64 through Ubuntu/WSL2, while macOS is skipped because Apple requires a real Mac. Missing prerequisites prompt before installation; explicitly requested targets default to Yes, already-installed tools are reused, and the current Rust toolchain is preserved. Use `--store=flatpak` to additionally install Flatpak Builder and the GNOME 51 runtime/SDK. See [Build environment setup](build-environment-setup.md) for prerequisites, WSL first-run steps and platform limits.

## Interactive selection

Run the build script without a target:

```powershell
py build.py
```

It detects the current operating system and lists the architectures available on that machine. Press Enter to select all locally supported architectures, or enter a number to select one. A non-interactive invocation also defaults to all locally supported architectures.

On Windows x64, the menu offers x64 and x86 builds. ARM64 appears as unavailable because this script currently requires a native Windows ARM64 host; it does not cross-build Windows ARM64 from x64.

## Explicit build commands

```powershell
# All locally supported architectures for the current OS
py build.py --build=all

# All architectures this Windows host can build
py build.py --build=windows

# One exact Windows target
py build.py --build='windows(10,x64)' --type=installer
py build.py --build='windows(11,x86)' --type=app
py build.py --build='windows(11,arm64)' --type=installer

# Linux and macOS targets (run these on the matching OS)
py build.py --build='linux(x64)' --type=installer
py build.py --build='linux(arm64)' --type=installer
py build.py --build='linux(armv7)' --type=installer
py build.py --build='macos(x64)' --type=installer
py build.py --build='macos(arm64)' --type=installer
```

`--type=installer` is the default. Use `--type=app` to compile and publish only the application and its sidecars without creating installers. Release builds are the default; add `--debug` for debug binaries.

An explicit target for another operating system returns an actionable error. It does not call `gh`, trigger a workflow, or download a build artifact.

## Windows installer formats

Windows installer builds use Master Packager Dev as the required packager:

- MPDEV builds both MSI and MSIX.
- Tauri builds NSIS `GitManager-Setup.exe` as the fallback.
- Tauri's WiX MSI target is disabled.
- The old `--master-packager` switch is accepted for compatibility, but MPDEV runs automatically for Windows installer builds.

See [Windows packaging](windows-packaging.md).

## Store packages

Store/package-manager streams also run locally:

```powershell
# Run on Linux to build a Flatpak bundle from the native DEB payload
py build.py --store=flatpak

# Run on Windows to build the Partner Center-aware Store MSIX
py build.py --store=msix
```

Flatpak requires `flatpak`, `flatpak-builder` and `dpkg-deb`; its packaging script installs the GNOME 51 runtime and SDK for the current user. The Microsoft Store path requires Windows and MPDEV. `--store=all` is intentionally rejected because Flatpak and Microsoft Store MSIX require different native operating systems; run the two commands on their respective machines. Homebrew/Linuxbrew remains deferred.

## Output layout

Build products and Rust intermediates are kept out of the repository-root `target/` folder:

```text
build/
  .rcache/                 # Cargo cache and intermediate files
  dist/
    windows-x64/
      app/                 # GitManager and sidecars
      installer/           # MPDEV MSI/MSIX and NSIS fallback
      build-manifest.json
    windows-x86/
      app/
      installer/
      build-manifest.json
    windows-arm64/
      app/
      installer/
      build-manifest.json
    linux-x64/
      app/
      installer/           # DEB, RPM, AppImage (where supported)
      build-manifest.json
    linux-arm64/
      app/
      installer/
      build-manifest.json
    linux-armv7/
      app/
      installer/           # DEB/RPM
      build-manifest.json
    macos-x64/
      app/
      installer/           # DMG
      build-manifest.json
    macos-arm64/
      app/
      installer/
      build-manifest.json
    store/
      flatpak/
      msix/
```

The generated `build/` directory is ignored by Git. It is safe to delete when no build is running; doing so removes Cargo intermediates and previously generated local installers.

## Automated checks

GitHub Actions can still run tests, syntax checks and the Linux runtime smoke test. Those checks are separate from local packaging: running `py build.py` never dispatches or relies on GitHub Actions to compile your app.

# Build environment setup

Run this once on each machine that will compile a GitManager target:

    py build.py setup-build-tools

The command checks what is already installed and prepares only the current host's supported toolchain. It does not dispatch or download builds from GitHub Actions.

## Windows 10/11

The Windows setup prepares Visual Studio 2022 Build Tools with the C++ workload and Windows SDK, Rustup and the Windows x64/x86 Rust targets, Node.js 22, Master Packager Dev and NSIS.

It also provisions Ubuntu 24.04 under WSL2 for Linux builds. On first setup, Windows may need an elevated terminal and/or a restart. Launch Ubuntu once from the Start menu, create its Linux username/password, and rerun the setup command.

The WSL Ubuntu setup installs Tauri's GTK/WebKitGTK 4.1 dependencies, Node.js 22, Rustup, Flatpak, Flatpak Builder, Debian/RPM packaging tools, and the matching GNOME 51 runtime and SDK. It runs a Bubblewrap namespace smoke test because Flatpak Builder requires sandbox namespaces. If that test fails under WSL, use a native Linux installation or VM for Flatpak packaging rather than weakening Flatpak's sandbox.

To build Linux from WSL, work from a Linux-side checkout for better filesystem performance:

    mkdir -p ~/src
    git clone --branch feature/cross-platform-build-matrix --single-branch https://github.com/Kate-alt-69/GitManager.git ~/src/GitManager
    cd ~/src/GitManager
    python3 build.py --build='linux(x64)' --type=installer
    python3 build.py --store=flatpak

For later builds, run git pull --ff-only in that WSL checkout before building. WSL can build Linux x64 on an x64 Windows PC. Linux ARM64/ARMv7 still need matching native Linux hosts or a separately configured cross-linker/sysroot.

WSL is a build environment, not a replacement for a native Linux desktop when validating Flatpak notifications, systemd user services, sandbox permissions or GUI behavior. WSL instances can stop when no process keeps them alive.

## Linux (native Debian/Ubuntu)

The setup command installs the Tauri v2 WebKitGTK/GTK development packages, build utilities, Node.js 22, Rustup, Flatpak Builder, Debian/RPM packaging tools, and GNOME 51 runtime/SDK for Flatpak.

Other Linux distributions are not modified automatically yet. Install the Tauri prerequisites for your distribution, then add Flatpak, Flatpak Builder, dpkg-deb and RPM/AppImage tools as required.

## macOS

Run the setup command on the Mac. It checks for Xcode Command Line Tools, installs Node.js 22 and pkg-config through Homebrew when available, and adds both x86_64-apple-darwin and aarch64-apple-darwin Rust targets.

If Command Line Tools are missing, macOS opens Apple's installer dialog; complete that installation and run setup again. The command does not install the full Xcode application. Xcode itself is needed for some Apple workflows beyond this desktop build, and code signing/notarization still requires Apple Developer credentials.

A Windows or Linux machine cannot install Apple's macOS SDK or produce a native macOS application bundle. Build macOS packages on a real Mac.

## Build matrix boundaries

| Output | Build host | Notes |
| --- | --- | --- |
| Windows x64/x86 MSI + MSIX + NSIS | Windows x64 | Requires MSVC, MPDEV and NSIS |
| Windows ARM64 | Native Windows ARM64 | Current build selector does not cross-build ARM64 Windows from x64 |
| Linux x64 DEB/RPM/AppImage | Native Linux x64 or x64 WSL2 | Requires GTK/WebKitGTK sysroot |
| Flatpak x64 | Native Linux x64; WSL2 only if Bubblewrap smoke test passes | GNOME 51 runtime/SDK |
| Linux ARM64/ARMv7 | Matching native Linux architecture | Matching linker and WebKitGTK sysroot required |
| macOS Intel/Apple Silicon | Real Mac with Xcode Command Line Tools | Both Rust targets are installed by setup |

This command installs toolchains and SDK prerequisites; it does not make every operating system cross-compilable from one host. To publish all formats from one Windows PC, build Windows natively and build Linux in WSL2 (or a native Linux VM). Use a Mac for the macOS artifact.

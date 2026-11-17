# Build environment setup

Run this on the machine that will compile the selected GitManager target:

    py build.py setup-build-tools

The setup is target-aware and asks before installing missing SDKs or packages. It does not dispatch builds to GitHub Actions and does not replace a working Rust compiler.

## Install prompts

When a prerequisite is missing, the prompt names the tool and purpose, for example:

    Do you want to install Ubuntu 24.04 + GTK/WebKitGTK to build Linux? [Y/n]

An explicitly requested target (`--build=linux(x64)` or `--store=flatpak`) defaults to **Yes**. Optional platforms that were not selected are not installed. If the required tools are already present, setup skips the prompt and reuses the existing `cargo`/`rustc` toolchain. It only installs Rustup when no usable Rust compiler exists, and it does not reset an existing active toolchain.

Use these commands to select what should be prepared:

    py build.py setup-build-tools
    py build.py setup-build-tools --build='linux(x64)'
    py build.py setup-build-tools --store=flatpak
    py build.py setup-build-tools --build=all

On Windows, `--build=all` prepares the supported Windows targets and the matching Linux/WSL2 environment, then skips macOS with a note because Apple's SDK requires a real Mac. On Linux or macOS, it prepares that host's native targets and skips the other operating systems. It does not attempt to cross-install foreign SDKs.

## Windows 10/11

For Windows targets, setup checks for Visual Studio 2022 C++ Build Tools and the Windows SDK, Node.js 22, Master Packager Dev and NSIS. It installs only missing components after the confirmation prompt. WebView2 is already included with supported Windows 10 (version 1803+) and later installations.

Windows does not install Linux/Flatpak tools unless Linux or Flatpak was selected. To prepare Linux x64 on a Windows x64 machine, run:

    py build.py setup-build-tools --build='linux(x64)'

That provisions Ubuntu 24.04 under WSL2 if needed, then asks before installing Tauri's GTK/WebKitGTK 4.1 dependencies, Node.js 22 and any missing Rust build prerequisites. On first WSL installation, Windows may require elevation and a restart. Ubuntu must complete a one-time Linux account setup before GitManager can install Linux build packages.

**First launch — create your Ubuntu username and password**

1. Open **Ubuntu 24.04 LTS** from the Windows Start menu. You can also start it from PowerShell with `wsl --distribution Ubuntu-24.04`.
2. When Ubuntu displays `Enter new UNIX username:`, type a Linux username and press Enter. This is a Linux account; it does not have to match your Windows username.
3. At `New password:`, type a password and press Enter. **The terminal shows nothing while you type**—no letters, dots or asterisks. This is normal Linux password behaviour.
4. At `Retype new password:`, type the same password again and press Enter.
5. Wait until Ubuntu displays its normal shell prompt. Type `exit` and press Enter to return to PowerShell.
6. Rerun the same setup command. For the all-platform Windows workflow, use:

   ```powershell
   py build.py setup-build-tools --build=all
   ```

   For Linux x64 only, use `py build.py setup-build-tools --build='linux(x64)'`.

GitManager will then enter Ubuntu, check its prerequisites, ask before installing missing packages, and preserve any existing Rust toolchain. The username/password are used by Ubuntu inside WSL; they are not your Windows sign-in credentials and GitManager does not read or store them.

To prepare Flatpak as well, run:

    py build.py setup-build-tools --store=flatpak

That additionally installs Flatpak, Flatpak Builder, the GNOME 51 runtime and SDK. It checks Bubblewrap user namespaces; if WSL cannot create the required namespaces, keep using WSL for Linux DEB/RPM builds and use native Linux or a Linux VM for Flatpak.

Build Linux from a Linux-side checkout for better filesystem performance:

    mkdir -p ~/src
    git clone --branch feature/cross-platform-build-matrix --single-branch https://github.com/Kate-alt-69/GitManager.git ~/src/GitManager
    cd ~/src/GitManager
    python3 build.py --build='linux(x64)' --type=installer
    python3 build.py --store=flatpak

WSL is a build environment, not a replacement for a native Linux desktop when validating Flatpak notifications, systemd user services, sandbox permissions or GUI behavior.

## Linux (native Debian/Ubuntu)

Run `py build.py setup-build-tools` to check the current machine's native dependencies. For the Flatpak runtime and SDK, explicitly request `--store=flatpak`. The setup installs GTK/WebKitGTK, Node.js 22 and Debian packaging dependencies only when missing and after confirmation. It installs GNOME 51 runtime/SDK only for a Flatpak request.

Other Linux distributions are not modified automatically yet. Install the Tauri prerequisites for your distribution, then add Flatpak/Flatpak Builder, dpkg-deb and RPM/AppImage tools as required.

## macOS

Run `py build.py setup-build-tools` on a real Mac. It checks Xcode Command Line Tools, Node.js 22, pkg-config and the requested Rust target components. If Xcode Command Line Tools are missing, setup asks before opening Apple's installer dialog. Complete the installation and rerun setup.

A Windows or Linux machine cannot install Apple's macOS SDK or produce a native macOS application bundle. Build macOS packages on a real Mac. Code signing/notarization still requires Apple Developer credentials.

## Build matrix boundaries

| Output | Build host | Notes |
| --- | --- | --- |
| Windows x64/x86 MSI + MSIX + NSIS | Windows x64 | Requires MSVC, Windows SDK, MPDEV and NSIS |
| Windows ARM64 | Native Windows ARM64 | Current build selector does not cross-build ARM64 Windows from x64 |
| Linux x64 DEB/RPM/AppImage | Native Linux x64 or x64 WSL2 | Requires GTK/WebKitGTK sysroot |
| Flatpak x64 | Native Linux x64; WSL2 only if Bubblewrap smoke test passes | GNOME 51 runtime/SDK |
| Linux ARM64/ARMv7 | Matching native Linux architecture | Matching linker and WebKitGTK sysroot required |
| macOS Intel/Apple Silicon | Real Mac with Xcode Command Line Tools | Both Rust targets can be installed with rustup |

This command prepares the selected host environment; it does not make all operating systems cross-compilable from one host. From Windows, build Windows natively and Linux in WSL2 (Flatpak only if Bubblewrap works). Use a Mac for the macOS artifact.

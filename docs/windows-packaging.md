# Windows packaging

GitManager uses **Master Packager Dev (MPDEV) as the required Windows MSI/MSIX packager**. Tauri creates the application bundle and the NSIS fallback; MPDEV produces both Windows package formats from the same staged application payload.

| Output | Builder | Purpose |
| --- | --- | --- |
| `GitManager-Setup.exe` | Tauri + NSIS | Fallback interactive installer |
| `GitManager_<version>_<arch>.msi` | Master Packager Dev | Required Windows Installer package |
| `GitManager_<version>_<arch>.msix` | Master Packager Dev | Required Windows app package |

Tauri's WiX MSI target is disabled. WiX is not required or installed by the Windows build script.

## Requirements

- Rust toolchain and the Tauri CLI
- Visual Studio C++ build tools for the selected architecture
- Master Packager Dev (required)
- NSIS (fallback installer)

The setup command installs MPDEV and NSIS using exact WinGet package IDs:

```powershell
py build.py setup-packaging-tools
```

MPDEV must be available as `mpdev.exe` on PATH or in its standard installation directory. The build fails clearly when MPDEV is missing; it never silently substitutes WiX or skips MSI/MSIX creation.

## Build locally

Run the interactive build selector:

```powershell
py build.py
```

Press Enter to build every architecture the current Windows machine can build. On a Windows x64 host, the script attempts x64 and x86 locally; Windows ARM64 is listed only when the matching Visual Studio toolchain is available. You can choose an individual architecture:

```powershell
py build.py --build='windows(10,x64)' --type=installer
py build.py --build='windows(11,x86)' --type=installer
py build.py --build='windows(11,arm64)' --type=installer
```

MPDEV runs automatically for every Windows `--type=installer` build. The old `--master-packager` switch remains accepted for command-line compatibility, but is no longer needed to enable MPDEV.

```powershell
# Build only the application payload, without installers
py build.py --build='windows(10,x64)' --type=app

# Build a Microsoft Store identity MSIX locally
py build.py --store=msix
```

The normal Windows build creates MPDEV MSI and MSIX packages plus the NSIS fallback. It does not use GitHub Actions to compile or package your local build.

## Output layout

All build products remain under the repository's ignored `build/` directory:

```text
build/
  .rcache/
    <rust-target>/release/        # Cargo outputs and intermediate files
    windows-package/source/       # Generated MPDEV staging payload
    windows-package/mpdev-package.json
    windows-installers/           # MPDEV MSI and MSIX outputs
  dist/
    windows-x64/
      app/                        # Main app and sidecar binaries
      installer/
        GitManager-Setup.exe      # NSIS fallback
        GitManager_1.0.0.0_x64.msi
        GitManager_1.0.0.0_x64.msix
```

The MSI and MSIX versions are derived from `src-tauri/tauri.conf.json` and padded to four numeric components. GitManager's CLI selector stays lowercase (`arm64`), while the generated MPDEV manifest uses the documented `Arm64` platform enum for Windows ARM64. The generated desktop MSIX uses MPDEV's development/test signing certificate for local installation testing. Do not treat that certificate as a trusted public release identity.

## Install locations and runtime files

The MSI is authored as a per-user package with `ALLUSERS=2` and `MSIINSTALLPERUSER=1`. Its MPDEV manifest uses the supported Program Files directory root; Windows Installer redirects that folder to the current user's per-user Programs location:

```text
%LOCALAPPDATA%\Programs\Kastrick's Software\GitManager
```

This keeps the MSI install per-user without hard-coding the build machine's user profile into the package.

The package includes the main executable, `gitm`, first-run setup helper, crash supervisor, background service, Developer Cleaner and the private architecture-matched CPython embeddable runtime.

User settings and data remain separate:

```text
%LOCALAPPDATA%\kastrick's_software\config\GitManager
%LOCALAPPDATA%\kastrick's_software\data\GitManager
```

MSIX is installed and managed inside the Windows package container. It does not use the MSI's Local AppData install path.

## Microsoft Store identity

For a Microsoft Store submission, use the separate local command `py build.py --store=msix`. Set `MSIX_PUBLISHER` and `MSIX_IDENTITY_NAME` to the exact values from Partner Center before building. That path generates an MSIX with the Store identity and omits the development certificate when both values are configured. Microsoft signs Store packages after certification.

A locally generated development/test-signed MSIX is for validation only, not direct public distribution.

## Fallback and verification

NSIS remains the fallback when an MSI/MSIX package cannot be installed or tested. Its post-install hook verifies that the bundled private Python runtime starts successfully and aborts an incomplete install.

Before considering an installer production-ready, test on a clean Windows 10/11 machine:

1. Install and launch the NSIS fallback, MSI and MSIX separately.
2. Confirm the crash supervisor, background service, CLI and Developer Cleaner work.
3. Confirm the MSI installs into the per-user Local AppData directory.
4. Test upgrade over an older version, repair, uninstall and reinstall.
5. Confirm clean uninstall preserves the sibling settings/data directories.
6. Import/trust the development MSIX certificate only for local testing; use the real Partner Center identity for Store submission.

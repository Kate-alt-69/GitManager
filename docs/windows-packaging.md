# Windows packaging

GitManager uses **Master Packager Dev (MPDEV) as the required Windows MSI/MSIX packager**. Tauri creates the application bundle and the NSIS fallback; MPDEV produces both Windows package formats from the same staged application payload.

| Output | Builder | Purpose |
| --- | --- | --- |
| `GitManager-Setup.exe` | Tauri + NSIS | Fallback interactive installer |
| `GitManager_<version>_<arch>.msi` | Master Packager Dev | Required Windows Installer package |
| `GitManager_<version>_<arch>.msix` | Master Packager Dev | Required Windows app package |

Tauri's WiX MSI target is disabled. WiX is not required or installed by the Windows build script. The NSIS fallback uses a version-matched Tauri installer template so its directory field defaults to `%LOCALAPPDATA%\kastrick's_software\GitManager`; users can still choose another folder.

See [Background worker packaging](background-service-packaging.md) for why GitManager ships the per-user worker without registering it as an MPDEV machine-wide Windows Service, and how the Store/MSIX path differs.

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

### MPDEV Free edition and internet access

MPDEV Free is suitable for qualifying personal non-commercial use and open-source projects, and does not require license activation for those use cases. It still needs to stay online for license validation: **Offline Mode is an Enterprise feature**, not a requirement for ordinary local MSI/MSIX builds.

GitManager does not enable MPDEV Offline Mode or add an offline-build flag to the command. When MPDEV reports `Offline mode is only supported with "Enterprise" license`, check that `mpdev.exe` can reach the internet. Review Windows Firewall, VPN, proxy, DNS filtering and endpoint-security rules, then return MPDEV to online mode and retry. An Enterprise license is only needed when you actually require MPDEV's offline operation.

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

The MSI and MSIX versions are derived from `src-tauri/tauri.conf.json` and padded to four numeric components. The custom NSIS template is based on Tauri CLI 2.11.4's upstream template; update the vendored template when upgrading the CLI, preserving the Kastrick default-directory change. GitManager's CLI selector stays lowercase (`arm64`), while the generated MPDEV manifest uses the documented `Arm64` platform enum for Windows ARM64. The generated desktop MSIX uses MPDEV's development/test signing certificate for local installation testing. Do not treat that certificate as a trusted public release identity.

## Windows background worker

Both the desktop MSI/MSIX payload and the separate Store MSIX payload include `resources\\gitmanager.service.exe`. GitManager starts this executable on demand during desktop startup and waits for its authenticated readiness handshake. The Settings page can also register it as an **opt-in per-user sign-in process** through Windows Task Scheduler, allowing repository indexing and remote checks to begin before the desktop UI opens.

This is deliberately not declared in MPDEV's `services[]` section. MPDEV's service feature creates a Windows Service Control Manager service. GitManager's worker is not currently an SCM service binary, and running it as `LocalSystem` or `LocalService` would move it out of the interactive user's profile and repository permissions. MSIX also does not support per-user Windows services; packaged services require restricted capabilities that Microsoft says are rarely approved for Store submissions. Do not add `packagedServices` or `localSystemServices` just to register this worker.

The supported direction is to keep the required service in the package and launch it as the signed-in user. The current MSI/MSIX sign-in registration path still requires real-machine validation, especially for MSIX package identity and Store certification; packaging the executable alone does not prove that the scheduled launch succeeds.

## Install locations and runtime files

The MPDEV MSI displays a branded install dialog with the GitManager description, Kastrick website/release links, an orange accent, and an optional `Launch GitManager` action after installation. The built-in layout and interaction model are provided by MPDEV; the project file customizes its supported branding fields rather than drawing arbitrary new dialogs.

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

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

### MSIX timestamp server

The development MSIX uses MPDEV's test certificate and the Microsoft timestamp authority by default:

`http://timestamp.acs.microsoft.com`

MPDEV validates that the configured timestamp server is reachable and responds successfully before signing. If your network blocks that endpoint, select another reachable RFC 3161 timestamp service with the `GITMANAGER_MPDEV_TIMESTAMP_SERVER` environment variable:

```powershell
$env:GITMANAGER_MPDEV_TIMESTAMP_SERVER = "http://timestamp.acs.microsoft.com"
py build.py
```

The default is used whenever the variable is unset. The build rejects non-HTTP(S) values rather than passing an invalid URL to MPDEV. Warnings about unsigned development executables or missing PE file-version metadata are separate from a timestamp-server error; the timestamp validation failure is the blocking issue in the reported log.

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

### Build order

Installer builds use two explicit passes so packaging never interrupts compilation:

1. **Compile pass:** for every selected architecture, build all workspace library and binary targets, build the Tauri desktop executable without bundling, and stage the complete app payload.
2. **Installer pass:** only after every selected target has finished its compile pass, run Tauri's bundle-only command for each successful target. On Windows, MPDEV then creates that target's MSI and MSIX; Tauri supplies the NSIS fallback.

If one architecture fails compilation, it is skipped during packaging while other successfully compiled targets can still finish. No installer is generated while another selected architecture is still compiling.

Release automation can set `GITMANAGER_DEFER_MPDEV_PACKAGING=1`. In that mode the script creates the Tauri bundles and MPDEV manifest, then leaves MSI/MSIX generation to the release workflow so it can sign the staged app and NSIS payload first. Ordinary local builds do not set this variable and create MSI/MSIX immediately in the final installer pass.

Store-package builds follow the same order. Flatpak builds compile every selected target before generating any DEB/Flatpak output; Store MSIX builds compile all selected Windows app payloads before MPDEV starts creating MSIX files.

On multi-architecture Windows builds, the shared private CPython staging runtime is reselected and verified for each target immediately before its Tauri bundle is created. This prevents an x86 build pass from leaving an x86 interpreter in the later x64 NSIS/MSI/MSIX output (or vice versa).

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

## First-run setup companion

Both Windows installers launch the packaged `gitmanager-setup.exe` companion after installation: NSIS runs its root-level sidecar, while the MPDEV MSI prompts the user to run `resources\gitmanager-setup.exe`. The companion launches the installed GUI with both `--setup` and `GITMANAGER_SETUP_MODE=1`, so the first-run wizard opens directly instead of re-triggering the launcher. The Tauri sidecar name must match the configured `binaries/gitmanager-setup` path exactly.

The normal application bootstrap starts `crash_helper` before the UI. The helper starts and supervises `gitmanager.service` outside the UI's process-containment job, so the first-run wizard is reached with the required data service already starting. The wizard offers a **checked-by-default** option to register a per-user sign-in task (Windows Task Scheduler, systemd user service, or macOS LaunchAgent); the user can uncheck it. The Settings page can also register the task later. When setup registers the Windows task while the service is already running, it does not launch a second copy.

## Windows background worker

Both the desktop MSI/MSIX payload and the separate Store MSIX payload include `resources\\gitmanager.service.exe`. GitManager starts this executable on demand during desktop startup and waits for its authenticated readiness handshake. The Settings page can also register it as an **opt-in per-user sign-in process** through Windows Task Scheduler, allowing repository indexing and remote checks to begin before the desktop UI opens.

This is deliberately not declared in MPDEV's `services[]` section. MPDEV's service feature creates a Windows Service Control Manager service. GitManager's worker is not currently an SCM service binary, and running it as `LocalSystem` or `LocalService` would move it out of the interactive user's profile and repository permissions. MSIX also does not support per-user Windows services; packaged services require restricted capabilities that Microsoft says are rarely approved for Store submissions. Do not add `packagedServices` or `localSystemServices` just to register this worker.

The supported direction is to keep the required service in the package and launch it as the signed-in user. The current MSI/MSIX sign-in registration path still requires real-machine validation, especially for MSIX package identity and Store certification; packaging the executable alone does not prove that the scheduled launch succeeds.

## Install locations and runtime files

Windows MSI and NSIS intentionally use separate install directories. The MSI installs to:

```text
%LOCALAPPDATA%\Programs\Kastrick's Software\GitManager
```

The NSIS fallback installs to:

```text
%LOCALAPPDATA%\kastrick's_software\GitManager
```

Do not point both installer formats at the same directory. They maintain separate Add/Remove Programs registrations and separate uninstallers; sharing the directory lets one format overwrite or remove the other format's `uninstall.exe`. Older builds used the NSIS directory for both formats, so upgrading from those builds may leave a second legacy registration. The new MSI path prevents future file/uninstaller collisions; remove a legacy entry only with the uninstaller belonging to that installation. Do not delete the shared config/data directories.


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

## Per-user service shutdown during install and removal

The NSIS hook and MPDEV MSI both prefer the installed `crash_helper.exe --stop-service` command before replacing or removing application files. The helper authenticates against the per-user service endpoint, requests a short suspension, waits briefly for active indexing and remote checks to become idle, then stops only the process ID returned by that authenticated endpoint. It clears the temporary suspension record afterward.

NSIS retains a current-user-scoped process-name fallback for older installations whose helper does not implement --stop-service. The MSI custom action is sequenced at StartOfExecution and is conditional on an existing installation; if it cannot stop an older service, the MSI's running-process detection remains available as a manual fallback.

MPDEV custom actions and detectRunningProcesses are MSI-only. A raw MSIX install therefore cannot run GitManager's graceful-stop helper before deployment. Microsoft documents ERROR_PACKAGES_IN_USE (0x80073D02) when an MSIX update or removal would modify files still mapped by a running process. For a local desktop MSIX update, close GitManager first, then run the following from PowerShell in the directory containing the new package:

\`\`\`powershell
$packagePath = (Resolve-Path .\GitManager_1.0.0.0_x64.msix).Path
$installed = Get-AppxPackage -Name GitManager -ErrorAction SilentlyContinue |
    Sort-Object Version -Descending |
    Select-Object -First 1

if ($installed) {
    $helper = Join-Path $installed.InstallLocation "resources\crash_helper.exe"
    if (Test-Path $helper) {
        & $helper --stop-service
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "The graceful service stop did not complete; Windows will close remaining processes associated with this package."
        }
    }
}

Add-AppxPackage -Path $packagePath -ForceTargetApplicationShutdown
\`\`\`

The helper gets the first chance to quiesce repository work and stop the service. ForceTargetApplicationShutdown is the bounded OS fallback for remaining processes associated with the package; it can forcibly close the app, so do not use it while unsaved UI work is open. Change the package filename and Name filter to match the identity of the package being updated. Store/MSIX identity updates still need validation through the Store's own deployment flow.

## Fallback and verification

NSIS remains the fallback when an MSI/MSIX package cannot be installed or tested. Its post-install hook verifies that the bundled private Python runtime starts successfully and aborts an incomplete install.

Before considering an installer production-ready, test on a clean Windows 10/11 machine:

1. Install and launch the NSIS fallback, MSI and MSIX separately.
2. Confirm the crash supervisor, background service, CLI and Developer Cleaner work.
3. Confirm the MSI installs into the per-user Local AppData directory.
4. Test upgrade over an older version, repair, uninstall and reinstall.
5. Confirm clean uninstall preserves the sibling settings/data directories.
6. Import/trust the development MSIX certificate only for local testing; use the real Partner Center identity for Store submission.

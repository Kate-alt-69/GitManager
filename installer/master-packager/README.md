# GitManager Windows packaging (Master Packager Dev)

**Master Packager Dev (MPDEV) is the required Windows MSI/MSIX packager.** Tauri creates the application and sidecar binaries; Tauri's NSIS package remains the fallback setup executable.

## Outputs

- NSIS fallback: `GitManager-Setup.exe`
- MPDEV MSI: `GitManager_<version>_<architecture>.msi`
- MPDEV MSIX: `GitManager_<version>_<architecture>.msix`

Tauri's WiX MSI target is disabled. The desktop MPDEV manifest at `package.json` creates **both** MSI and MSIX from the same staged application payload. The separate Microsoft Store identity MSIX uses `installer/store/msix/package.json` and the `py build.py --store=msix` command.

## Install the build tool

Install the required MPDEV package and the NSIS fallback:

```powershell
winget install --id MasterPackager.MasterPackagerDev --exact
winget install --id NSIS.NSIS --exact
```

Verify MPDEV is available:

```powershell
mpdev version
```

MPDEV is invoked automatically for every Windows `--type=installer` build. The deprecated `--master-packager` option is still accepted for compatibility but is no longer needed to enable it. If MPDEV is missing, the Windows installer build stops with an actionable error rather than falling back to WiX or silently skipping MSI/MSIX.

## Local build

```powershell
# Interactive local architecture selection; Enter builds all locally supported architectures
py build.py

# Exact Windows targets
py build.py --build='windows(10,x64)' --type=installer
py build.py --build='windows(11,x86)' --type=installer

# Application binaries without installer packaging
py build.py --build='windows(10,x64)' --type=app
```

MPDEV consumes the generated architecture-specific manifest at `build/.rcache/windows-package/mpdev-package.json`. It packages the staged Tauri executable, background service, crash supervisor, `gitm` CLI, first-run helper, Developer Cleaner and private CPython runtime. Cargo outputs and installer files stay under the ignored `build/` directory.

## Install and signing behavior

The MSI manifest uses the current user's Local AppData install directory:

```text
%LOCALAPPDATA%\kastrick's_software\GitManager
```

User configuration and data stay separate:

- `%LOCALAPPDATA%\kastrick's_software\config\GitManager\`
- `%LOCALAPPDATA%\kastrick's_software\data\GitManager\`

MSIX is installed and managed inside the Windows package container. It must not be described as using the MSI's Local AppData path.

The desktop MSIX currently uses MPDEV's generated development/test certificate for local installation validation. A test certificate is not a trusted public release identity. For Microsoft Store submission, use the separate Store stream with the exact Partner Center publisher and package identity. Microsoft signs the package after certification.

## Comparison and fallback

NSIS remains available if an MSI/MSIX installation path needs fallback or diagnosis. Before selecting MPDEV as the only release installer, test install, upgrade, repair and uninstall on a clean Windows 10/11 machine and confirm that user configuration/data directories are preserved.

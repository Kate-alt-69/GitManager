# GitManager Windows packaging (Master Packager Dev)

Master Packager Dev (MPDEV) creates an additional Windows **MSI** from GitManager's staged Tauri application payload. It is an additional packaging path, not a replacement for Tauri's native NSIS setup executable or WiX MSI.

## Outputs

- Tauri NSIS: `GitManager-Setup.exe`
- Tauri WiX: `*.msi`
- MPDEV comparison MSI: `GitManager_<version>_x64.msi`

The separate Microsoft Store MSIX is built only by `py build.py --store=msix` through `.github/workflows/store-build.yml`. It uses `installer/store/msix/package.json` and is deliberately not emitted by this desktop installer workflow.

The MPDEV package definition is `package.json`. The build overrides its four-part MSI version from `src-tauri/tauri.conf.json` so the installer version stays aligned with the desktop application. It packages the Tauri app bundle, including the desktop executable, crash supervisor, background service, CLI/setup sidecars, cleaner, and bundled CPython runtime.

## Install the build tool

```powershell
winget install --id MasterPackager.MasterPackagerDev --exact
```

MPDEV is a standalone command-line packager:

```powershell
mpdev version
mpdev build installer/master-packager/package.json --working-dir .
```

The repository workflow installs MPDEV through WinGet and verifies that its MSI exists. MPDEV is free for non-commercial and open-source projects; commercial use requires the appropriate license.

## Install location

The MSI targets the current user's Local AppData location:

```text
%LOCALAPPDATA%\\kastrick's_software\\GitManager\\
```

User configuration and data remain separate:

- `%LOCALAPPDATA%\\kastrick's_software\\config\\GitManager\\`
- `%LOCALAPPDATA%\\kastrick's_software\\data\\GitManager\\`

## Store packaging

MSIX is intentionally isolated in the Store stream. Store package identity, publisher identity, four-part versioning, signing and Partner Center validation are managed by `installer/store/msix/package.json` and `docs/store-packaging.md`. Do not distribute a development/test-signed MSIX as a trusted production package.

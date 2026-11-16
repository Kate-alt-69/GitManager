# GitManager Windows packaging (Master Packager Dev)

Windows installers are authored and built with **Master Packager Dev (MPDEV)**. Tauri creates the raw Windows application bundle; it does not generate NSIS or WiX/MSI installers.

## Outputs
- `GitManager_1.0.0.0_x64.msi`
- `GitManager_1.0.0.0_x64.msix`

The package definition is `package.json`. It packages the Tauri app bundle, including the desktop executable, crash supervisor, background service, CLI/setup sidecars, cleaner, and bundled CPython runtime.

## Install the build tool
```powershell
winget install --id MasterPackager.MasterPackagerDev --exact
```

MPDEV is a standalone command-line packager:
```powershell
mpdev version
mpdev build installer/master-packager/package.json --working-dir .
```

The repository workflow installs MPDEV through WinGet and verifies that both outputs exist. MPDEV is free for non-commercial and open-source projects; commercial use requires the appropriate license.

## Install location
The MSI targets the current user's Local AppData location:
```text
%LOCALAPPDATA%\kastrick's_software\GitManager\
```

User configuration and data remain separate:
- `%LOCALAPPDATA%\kastrick's_software\config\GitManager\`
- `%LOCALAPPDATA%\kastrick's_software\data\GitManager\`

MSIX is installed and managed by Windows in its package container. Its install location is controlled by Windows; it must not be represented as the MSI's Local AppData path.

## Signing
MSIX requires a trusted signature before Windows will install it. Package generation is verified separately from end-user trust. Production release signing must use the project's configured Windows code-signing identity; do not distribute a test-signed MSIX as a trusted production package.

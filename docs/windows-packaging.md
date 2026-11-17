# Windows packaging and installer comparison

GitManager keeps three Windows installer paths available:

| Output | Builder | Status |
| --- | --- | --- |
| `GitManager-Setup.exe` | Tauri + NSIS | Interactive setup |
| Tauri `*.msi` | Tauri + WiX | Standard MSI fallback |
| `GitManager-MasterPackager.msi` | Master Packager Dev | Experimental local comparison |

Master Packager Dev is additive. It does not replace WiX or NSIS.

## Prepare a Windows build machine

Run this once from the repository root in PowerShell:

```powershell
py build.py setup-packaging-tools
```

This uses exact WinGet package IDs:

- `WiXToolset.WiXToolset`
- `NSIS.NSIS`
- `MasterPackager.MasterPackagerDev`

You can combine the install and build steps:

```powershell
py build.py --build='windows(10,x64)' --type=installer --install-packaging-tools
```

To also generate the experimental MPDEV MSI:

```powershell
py build.py --build='windows(10,x64)' --type=installer --master-packager
```

Or do everything in one command:

```powershell
py build.py --build='windows(10,x64)' --type=installer --install-packaging-tools --master-packager
```

The installer-tool setup is explicit because WinGet may install machine-wide developer tools and may request elevation. A normal `py build.py --build='windows(10,x64)' --type=installer` does not silently install software. Tauri can also download/cache the bundler tools it needs.

Master Packager Dev is free without activation for open-source and non-commercial projects. Commercial use follows its own licensing terms; no license key is stored in this repository.

## Build outputs

A normal Windows `py build.py --build='windows(10,x64)' --type=installer` creates both Tauri-native outputs:

- NSIS setup executable under `build/.rcache/x86_64-pc-windows-msvc/release/bundle/nsis/`
- WiX MSI under `build/.rcache/x86_64-pc-windows-msvc/release/bundle/msi/`

When `--master-packager` is supplied, the build script stages the same release executables and supporting files into a generated directory, then calls `mpdev build` using `src-tauri/packaging/master-packager-dev.json`. Its output is placed under `build/.rcache/x86_64-pc-windows-msvc/release/bundle/master-packager-dev/`.

The MPDEV test payload includes:

- `GitManager.exe`
- `crash_helper.exe`
- `git-manager-service.exe`
- `gitm.exe`
- `GitManager-Setup.exe`
- `resources/python/` (the private CPython embeddable runtime)
- `resources/tools/gitmanager_cleaner.py`

### Important: MPDEV output is local-build-only for now

Master Packager Dev expands environment variables such as `%LOCALAPPDATA%` while building its project. The current experimental manifest therefore resolves the install path using the account that built the MSI. This is suitable for comparing the install flow on that same Windows account, but **do not redistribute this experimental MSI**: a build on a CI runner or another account would bake in that builder's profile path. The normal Tauri NSIS and WiX installers remain the portable release artifacts.

The MPDEV MSI uses its own stable upgrade code. Do not change it between test builds of the same product; changing it causes Windows Installer to treat the package as a different product.

## Expected NSIS install and resource layout

The NSIS hook forces the installation directory to:

```text
%LOCALAPPDATA%\kastrick's_software\GitManager
```

Shared settings and data remain in sibling directories:

```text
%LOCALAPPDATA%\kastrick's_software\config\GitManager
%LOCALAPPDATA%\kastrick's_software\data\GitManager
```

Tauri maps the Python runtime directory recursively to `resources/python/` and the cleaner to `resources/tools/gitmanager_cleaner.py`. Before bundling, `build.py` verifies that `python.exe`, `python314.zip`, and `python314._pth` exist. The NSIS post-install hook verifies that `resources/python/python.exe` starts successfully and aborts rather than finishing an incomplete installation.

## Comparison checklist

On a clean Windows 10/11 x64 test account, compare the NSIS and MPDEV installers:

1. Confirm the app installs into `kastrick's_software\\GitManager`.
2. Confirm the app starts and `crash_helper.exe` appears in the GitManager process tree.
3. Confirm the Developer Cleaner starts with the bundled Python runtime.
4. Confirm Start Menu shortcuts and Apps & Features entries are correct.
5. Test install over an older version, repair, uninstall and reinstall.
6. Confirm uninstall preserves the sibling user settings/data directories.

The WiX MSI remains the standard MSI fallback while the MPDEV install, upgrade and uninstall behavior is evaluated.

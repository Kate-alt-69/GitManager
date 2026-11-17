# Local store distribution builds

GitManager keeps store-distribution packages separate from the normal desktop installers, but it builds them on the user's machine rather than dispatching GitHub Actions jobs.

## Commands

Run each command on its native operating system:

```powershell
# On Linux: build a Flatpak bundle from the local Linux DEB
py build.py --store=flatpak

# On Windows: build the Partner Center-aware Store MSIX with MPDEV
py build.py --store=msix
```

`--store=all` is deliberately rejected on a single machine because Flatpak and Microsoft Store MSIX require different native operating systems. Run the Flatpak command on Linux and the MSIX command on Windows. Artifacts are written under `build/dist/store/`.

## Flatpak

The Flatpak path first builds GitManager's native Linux DEB package locally, then extracts its application payload and builds a Flatpak bundle from it. This preserves Tauri's resource and sidecar layout rather than rebuilding another copy of the app in the Flatpak manifest.

- Application ID: `com.gitmanager.desktop`
- Runtime: `org.gnome.Platform//51`
- Local architectures: the current Linux host architecture with a matching WebKitGTK development stack
- ARMv7: DEB/RPM only; Flatpak runtime support is not included
- Output: `build/dist/store/flatpak/GitManager-<arch>.flatpak`

Required local tools include `dpkg-deb`, `flatpak` and `flatpak-builder`, plus the GNOME 51 platform and SDK. The Flatpak builder uses the generated local DEB payload.

The manifest grants Wayland/X11, graphics, network, SSH-agent, home-directory and mounted-media access. Home access is currently required because GitManager scans and performs Git operations on user-selected local repositories.

The Settings → General page exposes an explicit **Install gitm command** action. It writes only a GitManager-marked wrapper at `~/.local/bin/gitm`, refuses to overwrite an unrelated file, and never edits `.bashrc`, `.zshrc`, or the host PATH. The user must already have `~/.local/bin` on PATH. The `~/.local/bin:create` permission documents the intended write location; while the broad home grant remains enabled for repository access, that narrower path grant is redundant. The action is opt-in and does not run during installation.

The current `--filesystem=home`, `/media`, `/run/media` and `/mnt` grants are intentionally broad because the first Flatpak iteration needs read/write access to arbitrary user-selected Git working trees and mounted removable drives. Before Flathub submission, replace these grants with portal-backed, user-selected directory access and persisted grants where feasible, then run Flathub lint/review.

Flatpak documentation:
- https://docs.flatpak.org/en/latest/sandbox-permissions.html
- https://docs.flatpak.org/en/latest/manifests.html
- https://v2.tauri.app/distribute/flatpak/

A locally generated bundle is for testing. A Flathub submission still needs Flathub-specific source manifests (Cargo and npm source generators), linting and review; building a bundle does not publish it to Flathub.

## Microsoft Store MSIX

Normal Windows desktop installer builds already use MPDEV to create both MSI and MSIX. The separate `--store=msix` stream uses `installer/store/msix/package.json` to produce an MSIX with the publisher identity reserved in Partner Center.

Set these environment variables in the same PowerShell session before building:

```powershell
$env:MSIX_PUBLISHER = "CN=Your exact Partner Center publisher"
$env:MSIX_IDENTITY_NAME = "YourPartnerCenterIdentityName"
py build.py --store=msix
```

Both values must match Partner Center exactly. When both are set, the generated Store package omits MPDEV's development certificate so Microsoft can sign it after certification. Without both values, the script warns and leaves a development/test-signed package for local validation only.

- Local architectures: Windows architectures the current host can compile and package
- Output: `build/dist/store/msix/`

A generated Store MSIX is not automatically published. Submission and certification still happen through Partner Center.

Official Microsoft guidance:
- https://learn.microsoft.com/en-us/windows/apps/publish/get-started
- https://learn.microsoft.com/en-us/windows/msix/package/signing-package-overview

## Future package managers

Homebrew on Linux (Linuxbrew) is deliberately deferred. It can be added as a separate package-manager stream after Flatpak installation, update, sandbox permissions and CLI integration have been tested.

# Store distribution builds

GitManager keeps store packages separate from its normal desktop installers.

## Commands

Run these from the repository root:

\`\`\`powershell
# Build both store package streams
py build.py --store=all

# Flatpak bundles for Linux x64 and ARM64
py build.py --store=flatpak

# Microsoft Store MSIX for Windows x64, x86 and ARM64
py build.py --store=msix
\`\`\`

The command dispatches \`.github/workflows/store-build.yml\` through GitHub CLI and downloads the completed artifacts under \`build/dist/store/\`. Authenticate first with \`gh auth login\`.

## Flatpak

The Flatpak stream first creates GitManager's normal Linux DEB package, then extracts its application payload and builds a Flatpak bundle from it. This preserves Tauri's packaged resource and sidecar layout instead of trying to rebuild a second copy of the app inside the Flatpak manifest.

- Application ID: \`com.gitmanager.desktop\`
- Runtime: \`org.gnome.Platform//51\`
- First targets: Linux x64 and Linux ARM64 (the ARMv7 target remains DEB/RPM only)
- Output: \`build/dist/store/flatpak/GitManager-x86_64.flatpak\`

The manifest grants Wayland/X11, graphics, network, SSH-agent, home-directory and optional \`~/.local/bin\` creation access. Home access is currently required because GitManager scans and performs Git operations on user-selected local repositories. The Settings → General page now exposes an explicit **Install gitm command** action. It writes only a GitManager-marked wrapper at `~/.local/bin/gitm`, refuses to overwrite an unrelated file, and never edits `.bashrc`, `.zshrc`, or the host PATH. The user must already have `~/.local/bin` on PATH. The `~/.local/bin:create` permission documents the intended narrow write location for this opt-in action; while the broad home grant remains enabled for repository access, that path grant is redundant. It does not run during installation. Flatpak installation itself does not run a host post-install script or silently alter shell configuration.

The current `--filesystem=home`, `/media`, `/run/media` and `/mnt` grants are intentionally broad because the first Flatpak iteration needs read/write access to arbitrary user-selected Git working trees and mounted removable drives. Before a Flathub submission, replace these broad grants with portal-backed, user-selected directory access and persisted grants where feasible, then re-run Flathub lint/review.

Flatpak's sandbox and portal behavior is documented at:
- https://docs.flatpak.org/en/latest/sandbox-permissions.html
- https://docs.flatpak.org/en/latest/manifests.html
- https://v2.tauri.app/distribute/flatpak/

The current workflow produces a testable Flatpak bundle. A Flathub submission still needs the Flathub-specific source manifests (Cargo and npm source generators), linting, and review; a CI bundle is not itself a Flathub publication.

## Microsoft Store MSIX

The MSIX stream builds Windows x64, x86 and ARM64 app-only payloads on matching hosted runners, then invokes Master Packager Dev using the isolated definition at \`installer/store/msix/package.json\`. It does not produce the ordinary NSIS/WiX installers.

The package currently uses the project's development/test signing mode. Microsoft Store submissions are re-signed by Microsoft after certification, but the package identity and publisher must match the identity reserved in Partner Center. Set the repository Actions variables `MSIX_PUBLISHER` and `MSIX_IDENTITY_NAME` to the exact publisher and identity name reserved in Partner Center before treating a generated artifact as a submission-ready package. The build derives the four-part MSIX version from `src-tauri/tauri.conf.json`. A test-signed MSIX is for local validation, not direct public distribution.

- First target: Windows x64
- Output: \`build/dist/store/msix/\`

Official Microsoft guidance:
- https://learn.microsoft.com/en-us/windows/apps/publish/get-started
- https://learn.microsoft.com/en-us/windows/msix/package/signing-package-overview

## Future package managers

Homebrew on Linux (Linuxbrew) is deliberately deferred. Once Flatpak install, update, sandbox permissions and CLI integration are tested, Linuxbrew can be added as a separate package-manager stream rather than being mixed into the Flatpak bundle.

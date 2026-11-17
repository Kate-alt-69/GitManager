# Store distribution builds

GitManager keeps store packages separate from its normal desktop installers.

## Commands

Run these from the repository root:

\`\`\`powershell
# Build both store package streams
py build.py --store=all

# Flatpak bundle only (Linux x64 for the first implementation)
py build.py --store=flatpak

# Microsoft Store MSIX only (Windows x64)
py build.py --store=msix
\`\`\`

The command dispatches \`.github/workflows/store-build.yml\` through GitHub CLI and downloads the completed artifacts under \`build/dist/store/\`. Authenticate first with \`gh auth login\`.

## Flatpak

The Flatpak stream first creates GitManager's normal Linux DEB package, then extracts its application payload and builds a Flatpak bundle from it. This preserves Tauri's packaged resource and sidecar layout instead of trying to rebuild a second copy of the app inside the Flatpak manifest.

- Application ID: \`com.gitmanager.desktop\`
- Runtime: \`org.gnome.Platform//48\`
- First targets: Linux x64 and Linux ARM64 (the ARMv7 target remains DEB/RPM only)
- Output: \`build/dist/store/flatpak/GitManager-x86_64.flatpak\`

The manifest grants Wayland/X11, graphics, network, SSH-agent, home-directory and optional \`~/.local/bin\` creation access. Home access is currently required because GitManager scans and performs Git operations on user-selected local repositories. The \`~/.local/bin:create\` permission is reserved for a future explicit in-app action that installs a \`gitm\` wrapper; installation must never edit shell startup files or the host PATH automatically.

Flatpak's sandbox and portal behavior is documented at:
- https://docs.flatpak.org/en/latest/sandbox-permissions.html
- https://docs.flatpak.org/en/latest/manifests.html
- https://v2.tauri.app/distribute/flatpak/

The current workflow produces a testable Flatpak bundle. A Flathub submission still needs the Flathub-specific source manifests (Cargo and npm source generators), linting, and review; a CI bundle is not itself a Flathub publication.

## Microsoft Store MSIX

The MSIX stream builds the Windows app-only payload and invokes Master Packager Dev using the isolated definition at \`installer/store/msix/package.json\`. It does not produce the ordinary NSIS/WiX installers.

The package currently uses the project's development/test signing mode. Microsoft Store submissions are re-signed by Microsoft after certification, but the package identity and publisher must match the identity reserved in Partner Center. Set the repository Actions variables `MSIX_PUBLISHER` and `MSIX_IDENTITY_NAME` to the exact publisher and identity name reserved in Partner Center before treating a generated artifact as a submission-ready package. The build derives the four-part MSIX version from `src-tauri/tauri.conf.json`. A test-signed MSIX is for local validation, not direct public distribution.

- First target: Windows x64
- Output: \`build/dist/store/msix/\`

Official Microsoft guidance:
- https://learn.microsoft.com/en-us/windows/apps/publish/get-started
- https://learn.microsoft.com/en-us/windows/msix/package/signing-package-overview

## Future package managers

Homebrew on Linux (Linuxbrew) is deliberately not part of this implementation. It can be added as a separate package-manager distribution after Flatpak's install, update, and CLI integration behavior is tested.

# GitManager RBE node deployment

## Download the bootstrap ZIP

The current bootstrap package is published separately from the GitManager desktop installers:

- [Download GitManager-Cloud-Node-Setup.zip](https://github.com/Kate-alt-69/GitManager/releases/download/cloud-node-setup-latest/GitManager-Cloud-Node-Setup.zip)
- [Download the SHA-256 checksum](https://github.com/Kate-alt-69/GitManager/releases/download/cloud-node-setup-latest/GitManager-Cloud-Node-Setup.zip.sha256)

For pull-request builds, download the gitmanager-cloud-node-deployment artifact from the GitManager native-installer workflow.

The ZIP is a small bootstrap package, not a multi-architecture binary archive. It contains:

- setup.py and manifest.json
- settings.json and setting.node.cn.json starter configurations
- an api/ folder with the RBE health and example .route files
- GitManager/RBE license and notice files

It deliberately does not contain backend, cloud_node, Container or Service binaries. This avoids shipping five copies of the RBE runtime to every NAS.

## Build the bootstrap ZIP

From the GitManager repository:

    py build.py prepare-cloud-node

The command uses Python 3.9+ and does not need Rust, Cargo or network access. The normal py build.py desktop build also creates the bootstrap ZIP. The release workflow builds and validates the ZIP independently.

## Install on the NAS / target machine

Extract the ZIP on the target computer. Python 3.9+ and network access are required on that machine because setup.py downloads the matching RBE release archive.

Windows:

    py setup.py

Linux or macOS:

    python3 setup.py

The installer detects the OS and CPU architecture, fetches the latest stable RBE release metadata, selects only that machine's archive, verifies the GitHub Release asset digest and matching SHA256SUMS entry, then safely extracts the complete runtime. The --verify-only option downloads and verifies the matching runtime but does not install it.

The official runtime archive contains the complete platform set:

| Installed item | Purpose |
| --- | --- |
| backend(.exe) | RBE HTTP/API backend |
| cloud_node(.exe) | RBE Cloud Node storage and peer/provider client |
| dep/container(.exe) | RBE Container runtime |
| dep/service(.exe) | RBE Service runtime |
| build-info.json | Release/build provenance |

The installer places these files beside one another in the install directory, keeping dep/ under the backend directory. It copies the bundled api/ folder beside backend so RBE's file-based route discovery can find the .route files. It also creates/preserves settings.json, setting.node.cn.json, the private-key file, and node-registration.json.

The installer never deletes existing data or overwrites existing settings/API routes. On upgrades, the downloaded runtime files are replaced only after the selected RBE archive has passed both checksum checks.

## Backend status and API routes

The included settings.json binds the backend API to 0.0.0.0:8080 by default. After starting the backend, these endpoints should be available:

- http://127.0.0.1:8080/health — native backend health endpoint
- http://127.0.0.1:8080/api/health — bundled api/health.route

Use the backend's actual configured port if settings.json was already present and preserved. The installer does not open firewall ports or create a TLS certificate. Restrict LAN access with the host firewall and configure HTTPS before exposing an endpoint beyond a trusted local network.

Initialize the RBE administrator account before the first normal backend run:

Windows:

    .\backend.exe --init-admin
    .\run-rbe-node.ps1

Linux/macOS:

    ./backend --init-admin
    ./run-rbe-node.sh

The generated start script sets RBE_CLOUD_NODE_PRIVATE_KEY to a protected file: reference and points RBE_CN_SETTINGS at the installed setting.node.cn.json. The private key itself is never written into JSON or copied into the ZIP.

## Cloud Node configuration and GitManager namespace

setting.node.cn.json is the separate RBE Cloud Node configuration. Its default storage root is under the install directory's data/cloud-node folder. The generated node-registration.json contains the node ID and public key; it does not contain the private key.

Cloud Node's content-addressed store can hold independent logical data prefixes. GitManager can use prefixes such as:

    gitm.archives/
    gitm.metadata/
    gitm.recovery/

Those are logical namespaces for GitManager-owned archive/metadata objects, not directories that the installer automatically syncs. The GitManager background worker must explicitly write those objects through the Cloud Node storage API or a configured peer/provider sync.

## Current peer-server limitation

The current public RBE binary release has a full backend API server and a Cloud Node storage/client binary. Its authenticated peer receiver is currently wired into the backend's temporary maintenance/boot-recovery listener; it is not yet a permanent LAN NAS listener with an interactive pairing endpoint. The installer therefore provides the full backend, /api route setup, status endpoints and CN configuration, but does not claim that a persistent GitManager-to-NAS peer connection is already enabled.

RBE's current peer URL validation also requires HTTPS for non-loopback hosts. A plain http://192.168.x.x address is not accepted by its peer configuration. A permanent LAN server/pairing flow and TLS behavior need to be implemented and tested before the GitManager UI should mark a NAS as connected.

## Supported RBE release targets

The setup script selects one matching runtime archive from the official Kastrick8/RBE latest stable release:

| Host | Release target | Compatibility |
| --- | --- | --- |
| Windows x64 | windows-x64 | MSVC |
| Linux x64 | linux-x64 | glibc 2.35+ |
| Linux ARM64 | linux-arm64 | glibc 2.39+ |
| macOS Intel | macos-intel | x86_64 |
| macOS Apple Silicon | macos-arm64 | arm64 |

Windows ARM64, Linux musl and Linux 32-bit are not included in the current RBE release. macOS RBE binaries are not Apple-notarized; this installer does not bypass Gatekeeper or remove quarantine attributes.

## Provenance and integrity

setup.py resolves the latest stable release from the official Kastrick8/RBE GitHub Releases API. It checks the release asset's size and SHA-256 digest, verifies the same archive hash in the matching SHA256SUMS-build.*.txt, rejects unsafe archive paths and file types, and checks the required binaries/build-info before installation. rbe-release.json records the selected release and archive provenance.

GitManager and the setup tool are distributed under Apache License 2.0; see LICENSE-GITMANAGER.txt. The separately downloaded RBE runtime is Apache License 2.0; see LICENSE-RBE.txt and NOTICE-RBE.txt. Public RBE documentation:

- https://github.com/Kastrick8/RBE/blob/main/doc/cloud-node.md
- https://github.com/Kastrick8/RBE/blob/main/doc/binary-updates.md
- https://github.com/Kastrick8/RBE/tree/main/api

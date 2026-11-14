# GitManager Cloud Node deployment

This ZIP is a separate, cross-platform deployment package for **RBE Cloud Node**. It is not embedded in `git-manager.exe` and it does not install the RBE backend, Container, or Service binaries.

The package contains the standalone `cloud_node` executable extracted from the official RBE runtime release for five targets:

| Host | Included target | Compatibility |
| --- | --- | --- |
| Windows x64 | `windows-x64` | MSVC |
| Linux x64 | `linux-x64` | glibc 2.35+ (RBE baseline: Ubuntu 22.04) |
| Linux ARM64 | `linux-arm64` | glibc 2.39+ (RBE baseline: Ubuntu 24.04 ARM) |
| macOS Intel | `macos-intel` | x86_64 |
| macOS Apple Silicon | `macos-arm64` | arm64 |

Windows ARM64, Linux musl, and Linux 32-bit are not included in the current RBE release. macOS RBE binaries are not Apple-notarized; this setup does not bypass Gatekeeper or remove quarantine attributes.

## Install

Extract this ZIP on the machine that will host the node. Python 3.9+ is required; the script uses only the Python standard library and the included binaries. No Rust, Cargo, Node.js, npm, or network download is required on the destination machine.

Windows:

```powershell
py setup.py
```

Linux or macOS:

```sh
python3 setup.py
```

The setup script detects the local OS and CPU, verifies the selected executable against `manifest.json`, installs it into a per-user application-data directory, asks for the RBE backend connection details and storage location, creates a node identity, and offers to register a per-user background service.

- Windows: Task Scheduler task at user logon.
- Linux: `systemd --user` unit, when available.
- macOS: per-user LaunchAgent.

No machine-wide PATH changes or administrator/root privileges are required. The Cloud Node private key is stored separately in a restricted secret file; it is never written into `setting.cn.json`, the peer-registration snippet, or this ZIP.

## Connect the node to your RBE backend

Cloud Node uses the existing authenticated RBE peer protocol. The node initiates an outbound HTTPS connection to the RBE backend's Cloud Node recovery endpoint; it does not open a general-purpose NAS file-sharing listener.

During setup, provide:

1. The RBE backend base URL.
2. The backend Cloud Node ID.
3. The backend's 64-character Ed25519 public key.

The backend public key must correspond to the private key configured on the RBE backend as `RBE_CLOUD_NODE_PRIVATE_KEY`. On the RBE host, the public key can be obtained with the RBE `cloud_node public-key` command while that private-key environment variable is configured. **Never copy the backend private key to this node.**

Setup generates:

- `setting.cn.json`: local node ID, storage root, and the authenticated upstream backend.
- `cloud-node-private-key.secret`: this node's private Ed25519 key, stored separately.
- `rbe-peer-registration.json`: this node's public identity for the RBE backend administrator.

Before authentication can succeed, add the generated `rbe-peer-registration.json` object to the RBE backend's `setting.cn.json` under `replication.targets`, preserving any existing targets. The backend must be configured with its own private key and must allow Cloud Node recovery. The `url` member is required by the current RBE settings schema; the current inbound recovery authenticator trusts the peer's `nodeId` and `publicKey` and does not dial that URL.

The Cloud Node service retries with bounded backoff while the backend is unavailable or before the new peer has been registered. You can therefore configure the RBE backend after installing the node, then restart the service.

## Files and data

The default install directory is user-local. The selected `storageRoot` is a separate data directory; it is not the application install directory. Existing storage is never deleted by this installer. To upgrade, rerun setup and choose to reuse the existing node identity/settings.

Useful checks:

```text
cloud_node --help
cloud_node --config=<path-to-setting.cn.json> evaluate
cloud_node --config=<path-to-setting.cn.json> verify
```

On Linux, the user service can be inspected with `systemctl --user status cloud-node.service` and `journalctl --user -u cloud-node.service`. On macOS, logs are written under the installation directory's `logs` folder. On Windows, the scheduled-task launcher appends output to `logs/cloud-node.log`.

## Provenance and integrity

The binaries are downloaded by GitManager's build step from the official `Kastrick8/RBE` GitHub Release. The build verifies each complete upstream archive against both GitHub's Release API SHA-256 digest and the release's `SHA256SUMS` file, then extracts only the root-level `cloud_node` executable. `manifest.json` records each executable's SHA-256 and the exact RBE release/build provenance. Setup verifies the selected binary again before installing it.

RBE is distributed under Apache License 2.0; see `LICENSE-RBE.txt`. The RBE source and release documentation are available at:

- https://github.com/Kastrick8/RBE
- https://github.com/Kastrick8/RBE/blob/main/doc/cloud-node.md
- https://github.com/Kastrick8/RBE/blob/main/doc/binary-updates.md

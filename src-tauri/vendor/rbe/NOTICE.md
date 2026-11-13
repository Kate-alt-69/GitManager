# Vendored RBE Cloud Node — provenance and license status

Source project: [Kastrick8/RBE](https://github.com/Kastrick8/RBE)

Vendored components:

- `engine/crates/cloud-node`
- `atomic-io`
- `storage-sync-journal`

The source files were copied into GitManager on 2026-11-13 and are maintained
as a local copy under this directory. GitManager does not modify or push to the
RBE repository as part of this vendoring work.

GitManager's own root `LICENSE` is Apache License 2.0. However, at the time
this copy was prepared, the public RBE default branch had no root or nested
`LICENSE`, `NOTICE`, or `COPYING` file, the Cloud Node Cargo manifest had
no `license` field, and GitHub reported no detected repository license.
Therefore this notice records provenance but does **not** claim that the
upstream RBE source has independently been confirmed as Apache-2.0 licensed.
The source owner's explicit Apache-2.0 grant for the RBE components should be
confirmed before redistribution of these vendored portions outside the
owner-controlled repositories.

GitManager-local modifications made after copying include:

- Excluding common credential files and directories from Cloud Node local
  directory scans by default.
- Adding direct logical-file read, verify, restore, and prune operations to
  CloudNodeStore for repository-level content-addressed snapshots.
- Making the Cloud Node crate manifests standalone Cargo packages rather than
  inheriting settings from the upstream RBE workspace.

The RBE source copy is intentionally vendored rather than referenced through a
floating Git dependency. Upstream updates must be reviewed and copied
deliberately.

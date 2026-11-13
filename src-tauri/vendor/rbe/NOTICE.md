# Vendored RBE Cloud Node — third-party notices

The following source components are copied from [Kastrick8/RBE](https://github.com/Kastrick8/RBE):

- `engine/crates/cloud-node`
- `atomic-io`
- `storage-sync-journal`

**Copyright and origin:** RBE project by Kastrick (upstream repository: `Kastrick8/RBE`).

**License:** Apache License, Version 2.0. A complete copy of the upstream license is
provided in [LICENSE](./LICENSE). The license is also available at
https://www.apache.org/licenses/LICENSE-2.0.

The vendored source is maintained locally under GitManager. GitManager does not
modify, push to, or depend on a live branch of the RBE repository.

## GitManager modifications

GitManager modifies `cloud-node/src/local_sync.rs` to exclude common environment
secrets, credentials and private keys by default for its archive integration.
The change is marked in the affected source file.

The bounded logical-file read, SHA-256 verification, staged restore and
logical-object removal APIs are already provided by the upstream
`cloud-node/src/store.rs`; GitManager consumes those APIs without duplicating
them. The remainder of the vendored source is retained from the upstream copy.
Upstream updates must be reviewed and copied deliberately.

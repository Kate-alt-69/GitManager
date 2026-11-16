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

GitManager also applies Windows flush-compatibility patches in
`cloud-node/src/store.rs`, `client.rs`, `recovery.rs`, and `durable.rs`.
Copied files are opened with write access before `sync_all()`; Windows requires
a write-capable handle for this flush and the original read-only handles
returned `ERROR_ACCESS_DENIED`.

The bounded logical-file read, SHA-256 verification, staged restore and
logical-object removal APIs are already provided by upstream
`cloud-node/src/store.rs`; GitManager consumes those APIs without duplicating
them. The remainder of the vendored source is retained from the upstream copy.
Upstream updates must be reviewed and copied deliberately.


## GitManager Cloud Node feature isolation

The vendored Cloud Node manifest now keeps default features empty. GitManager's
archive engine enables only `provider-sync`; the authenticated peer transport
(`peer-client` / `peer-auth`), inbound server (`server`), registry ingestion,
local sync and storage-journal ingestion remain separate opt-in features.
The existing `client` feature remains as a compatibility umbrella for the full
peer-client and provider-sync surfaces.

Shared durable outbound-cache helpers were moved to
`cloud-node/src/outbound_cache.rs` so provider sync does not compile the
peer-transport module. The interrupted registry replacement marker check was
moved to `cloud-node/src/registry_guard.rs` and remains active even when the
optional registry-ingest implementation is not compiled. These changes are
local GitManager vendor adaptations; the upstream RBE repository is untouched.

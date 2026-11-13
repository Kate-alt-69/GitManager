# Repository archives and recovery

## Two archive types

GitManager maintains distinct archive types:

- **Local Archive** preserves the current working copy and local edits, including
  committed, uncommitted, and untracked project files.
- **Remote Archive** preserves Git metadata as well as repository files. A
  recovered Remote Archive can be opened as a Git repository and pushed to a
  Git host again after credentials/remotes are reviewed.

Both types exclude secret material by default. This includes `.env` and
`.env.*` files except example/template files, common credential directories,
private-key files, and common credential JSON files. Excluded paths must be
reported to the user and configurable explicitly; a cloud copy must not silently
collect environment secrets.

## RBE Cloud Node storage

GitManager vendors the RBE `cloud-node`, `atomic-io`, and
`storage-sync-journal` crates under `src-tauri/vendor/rbe`. This is a pinned
source copy, not a floating Git dependency. The existing content-addressed
storage, durable writes, provider authentication, resumable transfer, and
provider-history implementation are the base for GitManager's repository
archive layer.

Cloud Node currently supports Amazon S3, Supabase Storage, Azure Blob Storage,
Google Cloud Storage, and generic HTTPS object storage. Google Cloud Storage is
not Google Drive: a Google Drive Desktop-synced folder can be used as a local
filesystem destination, while direct Google Drive API support needs a separate
provider adapter.

The upstream `backupVersions: 5` setting is per-object history, not five
consistent repository-wide restore points. GitManager must maintain its own
transactional repository snapshot index and apply a five-point retention rule
to the complete archive. Provider-side history/object pruning must also be
implemented before claiming that remote object storage itself is capped at five
complete copies.

## Capture triggers

- **On commit** captures a repository state after its Git HEAD changes.
- **On change** waits for a quiet/debounce interval before capturing, so an
  editor save sequence does not create a restore point per low-level write.
- Manual capture remains available from GitManager and `gitm`.

A capture is staged and verified before its index entry is published. A failed
or interrupted capture must not evict the last verified restore point.

## Recovery behavior

Restore previews the archive type, repository identity, timestamp, branch/HEAD,
file count, size, and excluded paths. Restoring to an existing non-empty
directory requires explicit confirmation. The current destination is preserved
before replacement by default. Remote Archive restore recovers Git data, not
authentication secrets; credentials must be set up again.

Five restore points are a retention policy, not a full disaster-recovery
guarantee. A local archive destination should be on a different physical device;
a cloud provider should be independent of the source machine and disk.

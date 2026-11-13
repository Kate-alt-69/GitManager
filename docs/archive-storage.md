# Repository archives and recovery

## Two archive types

GitManager maintains distinct archive types:

- **Local Archive** preserves the current working copy and local edits, including
  committed, uncommitted, and untracked project files, but excludes Git metadata.
- **Remote Archive** preserves working files and self-contained Git metadata,
  including commits, refs and sanitized remote definitions. It can be opened as
  a Git repository and pushed to a Git host again after credentials are reviewed.

Both types exclude secret material by default. This includes `.env` and
`.env.*` files except example/template files, common credential directories,
private-key files, common credential JSON files, and Git credential-bearing
configuration/hooks. The excluded paths are recorded in each restore-point
summary. Remote Archive stores safe remote names and sanitized fetch/push URLs
separately so it can recreate remotes without copying `.git/config` or its
credential helpers. It also omits Git reflogs, rerere caches, linked-worktree
metadata and nested Git module stores.

Before building a Remote Archive pack, GitManager scans the trees of every
commit reachable from repository refs. If a reachable historical tree contains
an excluded secret path (for example `.env.production` or `.ssh/id_ed25519`),
capture is refused before any archive data is written. Remove the path from all
reachable history (rewrite the affected commits), then retry. The scan is cached
against the repository's ref fingerprint and is repeated when refs change.

Remote Archive never copies the source `.git/objects` database wholesale. It
builds a fresh pack containing only commit, tree, blob and annotated-tag objects
reachable from repository refs (plus a detached HEAD when present). Staged-only
blobs, dangling/unreachable objects, reflogs, the Git index, hooks and transient
merge/rebase state are not included. Working-tree file contents are still
captured separately, including uncommitted and untracked edits; the staging
state itself is not preserved, so staged changes appear as ordinary worktree
changes after restore and can be staged again.

This is a path-based protection, not a secret-content scanner: credentials pasted
into ordinary source files cannot be reliably detected. Repositories with shallow
history are rejected because a partial object graph cannot provide a complete
Remote Archive.

## RBE Cloud Node storage

GitManager vendors the RBE `cloud-node`, `atomic-io`, and
`storage-sync-journal` crates under `src-tauri/vendor/rbe`. This is a pinned
source copy, not a floating Git dependency. The existing content-addressed
storage, durable writes, provider authentication, resumable transfer, and
provider-history implementation are the base for GitManager's repository
archive layer. The complete upstream Apache-2.0 license and a source notice
are included beside the vendored crates.

Cloud Node currently supports Amazon S3, Supabase Storage, Azure Blob Storage,
Google Cloud Storage, and generic HTTPS object storage. Google Cloud Storage is
not Google Drive: a Google Drive Desktop-synced folder can be used as a local
filesystem destination, while direct Google Drive API support needs a separate
provider adapter.

The desktop **Scan Setup → Repository archives → Cloud object storage** section
accepts one provider JSON object. Use **Configure provider** to validate and save
it, **Check status** to inspect the target and required credential environment
variable names, **Sync now** to perform an explicit synchronization/retry, and
**Disable** to stop future synchronization. Disabling removes only the local
provider configuration; it does not delete objects already stored by the
provider. Repository captures attempt cloud synchronization automatically
after their local restore point has been committed. If the provider is offline,
the local archive remains saved and can be synchronized later.

Provider credentials must be set in the current user's operating-system
environment. The JSON stores environment-variable names only; never paste raw
tokens, passwords, private keys or service-account JSON into provider
configuration. The desktop status panel reports the names GitManager expects
for the selected provider/authentication mode.

The upstream `backupVersions: 5` setting is per-object history, not five
consistent repository-wide restore points. GitManager maintains a separate
per-repository/per-kind manifest index and retains the five newest complete
restore points. Repository identity uses the sanitized primary Git remote URL
where available, so cloud restore points follow clones across checkout paths
and machines. Content-addressed blobs are shared between retained snapshots
and stale logical objects are removed only after the replacement index is
published. Provider-side versioning or soft-delete policies may retain older
physical object versions even after GitManager removes them from its active
logical view.

## Capture triggers

- **After Git commits** watches the repository Git metadata and checks that
  HEAD actually changed. It watches HEAD/refs, not the potentially huge object
  database.
- **After file changes settle** watches discovered repositories and waits for
  the configured quiet period before capturing. Git metadata-only events are
  ignored so a commit does not cause duplicate change-trigger snapshots.
- **Manual only** disables background capture. Manual capture remains available
  in the desktop repository panel and in `gitm`.

The watcher is limited to discovered Git worktrees and 256 repositories per
process. It refuses to start for a repository whose archive directory is nested
inside that repository. Archive capture/restore operations for one repository
are serialized so Local and Remote writes cannot race the shared Cloud Node
index.

A capture is staged and verified before its index entry is published. A failed
or interrupted capture must not evict the last verified restore point.

## Portable repository identity and disaster recovery

For repositories with a configured Git remote, GitManager derives the archive
identity from the sanitized `origin` URL (or the first configured remote when
there is no `origin`). The identity does not depend on the checkout directory,
so a clone opened on another disk or machine can find the same cloud restore
points. Repositories without any remote remain local-path scoped.

To recover a Remote Archive after both the working folder and Git host are
unavailable, create an empty folder, run `git init`, and add the archived
remote URL as `origin` (adding the URL to local Git config does not require the
host to be online). Then use `gitm backups` to find the restore point and
`gitm restore . --kind remote --id <snapshot-id> --yes`. The Remote Archive
recreates the stored refs and sanitized remotes from its local/cloud archive.
You can change or remove the remote later if the old host no longer exists.

## Recovery behavior

The restore panel shows archive type, snapshot time, branch/HEAD, file count,
size and excluded paths. Restoring to an existing non-empty directory requires
explicit confirmation. The current destination is preserved before replacement
by default. Remote Archive restores the repository's configured remotes from
sanitized URLs; authentication secrets and Git credential helpers must be set
up again. Remote Archive currently refuses linked worktrees and repositories
that depend on external Git object alternates, because those are not
self-contained recovery sources.

Five restore points are a retention policy, not a full disaster-recovery
guarantee. A local archive destination should be on a different physical device;
a cloud provider should be independent of the source machine and disk.

## `gitm` native CLI

GitManager ships a separate Rust CLI so archive operations do not require Node.js
or the desktop WebView runtime:

```text
gitm
gitm status .
gitm log . --limit 25
gitm branches .
gitm remotes .
gitm backup . --kind local
gitm backup . --kind remote
gitm backups . --kind local
gitm restore . --id <snapshot-id> --kind local --to ../recovered --yes
```

The inspection commands use Git's native repository data. Remote URLs are
redacted before they are printed; use Git itself to inspect or change
credential-related settings.

The no-argument command opens a keyboard-driven TUI. It supports arrow keys,
WASD, left/right navigation on wider terminals, Enter, Escape/Q, and optional
SGR mouse selection. Its menu switches to two columns on wide terminals and
clips text by terminal display width. ASCII and no-color modes support older or
dumb terminals (`GITM_ASCII=1`, `TERM=dumb`, `NO_COLOR=1`, or `GITM_COLOR=0`). Use `--archive-root PATH` to select another local folder,
including a Google Drive, OneDrive, or Dropbox Desktop-synced folder.

Cloud object storage can be configured with `gitm cloud configure --file
provider.json`, checked with `gitm cloud status`, synchronized with
`gitm cloud sync`, and disabled with `gitm cloud disable`. The vendored RBE
Cloud Node provider layer supports Amazon S3, Supabase Storage, Azure Blob
Storage, Google Cloud Storage, and a generic HTTPS object-storage endpoint.
Configuration stores environment-variable names only; put the actual secret
values in the operating system environment before syncing.

Google Cloud Storage is not Google Drive. Selecting a Google Drive Desktop
folder stores a local archive in that folder and lets Google's own sync client
replicate it; direct Google Drive API support is not implied.

## Cloud provider configuration example

This example configures a generic HTTPS endpoint with a bearer token supplied
through an environment variable:

```json
{
  "kind": "http",
  "namespace": "gitmanager",
  "bucket": "repository-archives",
  "endpoint": "https://storage.example.com",
  "auth": {
    "mode": "bearer",
    "bearerTokenEnv": "GITMANAGER_ARCHIVE_TOKEN"
  }
}
```

Set `GITMANAGER_ARCHIVE_TOKEN` in the current user's environment. The token
value is never written into GitManager's cloud settings. See the RBE Cloud Node
provider configuration for provider-specific fields and authentication modes.

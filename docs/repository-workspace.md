# Repository Workspace

The repository workspace is the native Git operations layer behind GitManager's
**Code & Changes** view. It is deliberately separate from the presentation
layer so another frontend can reuse the same safe file-browser and Git commands.

## Location and integration

- Native implementation: \`src-tauri/src/repository_workspace.rs\`
- Tauri command wrappers and registration: \`src-tauri/src/lib.rs\`
- Shared TypeScript contracts: \`frontend/lib/types.ts\`
- Active desktop UI: \`frontend/components/repository-workspace.tsx\`
- Parent integration: \`frontend/components/gitmanager-app.tsx\`

The packaged desktop app uses the Next.js static export configured by
\`src-tauri/tauri.conf.json\`. The legacy \`ui/\` directory is not the active
repository-workspace surface.

## Command API

All commands accept camelCase arguments at the Tauri boundary.

| Command | Purpose |
| --- | --- |
| \`list_repository_files\` | List one directory from the local working tree or a fetched remote-tracking branch |
| \`read_repository_file\` | Read a bounded UTF-8 text preview from local disk or a remote Git tree |
| \`get_repository_changes\` | Return staged, unstaged, deleted, renamed, conflicted and untracked paths |
| \`read_repository_file_diff\` | Read a bounded staged or unstaged diff for one path |
| \`stage_repository_paths\` | Stage explicitly selected paths |
| \`unstage_repository_paths\` | Reset selected index paths to HEAD while preserving working-tree contents |
| \`preview_repository_commit\` | Capture the staged tree ID and current status for a commit review |
| \`create_repository_commit\` | Create a commit only if the staged tree still matches the reviewed tree ID |
| \`delete_repository_path\` | Delete one local file, link or directory after path and nested-repository checks |
| \`fetch_repository_remote\` | Fetch a configured remote's tracking refs |
| \`preview_repository_pull\` | Fetch the configured upstream and return branch, ahead/behind, changes and a status token |
| \`prepare_and_pull_repository_update\` | Revalidate the preview, prepare local changes, then fast-forward or rebase the reviewed commit |
| \`list_local_repository_branches\` / \`switch_repository_branch\` | List local branches and switch without overwriting a dirty working tree |

The command argument and result interfaces are defined in \`frontend/lib/types.ts\`
and should be kept in sync with the native serializable structs.

## Safety boundaries

### File browsing

- Paths must be repository-relative; absolute paths and parent traversal are rejected.
- Git metadata named \`.git\` is hidden and rejected at every depth, including remote trees.
- Local symbolic links are not followed. A preview must resolve to a regular file inside the repository.
- Text previews are limited to 1 MiB; diff previews are limited to 512 KiB.
- A directory listing is limited to 2,000 visible entries. Open a subfolder to browse a smaller section.
- The file browser does not execute, render or parse project files.

### Stage, unstage and commit

- Stage and unstage accept explicit paths only; Git pathspecs are treated literally.
- Unstage resets only selected index entries to HEAD. It does not overwrite local file contents; a staged addition becomes an ordinary untracked file.
- Before the first commit exists, Unstage removes only the selected index entries with `git rm --cached`, leaving the files on disk.
- Commit review records the staged tree ID. If the index changes before confirmation, commit creation is rejected.
- Commit messages are required and limited to 200 characters.

### Delete

- Deletion is restricted to paths inside the selected repository and does not follow symbolic-link directories.
- A directory containing nested Git metadata is rejected. Failure to inspect the directory is treated as unsafe and also rejects deletion.
- Deleting a tracked path leaves a normal Git deletion in Changes, which can be restored before it is committed.
- Deleting untracked data removes it from disk; Git cannot restore it unless another backup exists.

### Pull / Sync

- Opening the Pull review fetches the configured upstream branch and updates remote-tracking metadata. It does not change local files, the index or the checked-out branch.
- Confirmation repeats the preview and compares the branch, local commit, remote commit and working-tree status token. A stale review is rejected before staging, stashing, committing or pulling.
- Only fast-forward candidates with incoming commits and no local-only commits are accepted.
- A clean working tree uses a fast-forward-only merge.
- **Stash all** saves staged, unstaged and untracked local changes before the fast-forward.
- **Commit selected paths** commits only the checked paths, stashes unchecked changes, then rebases the new local commit onto the fetched upstream.
- The UI explicitly reports that a stash was created or a commit/rebase was performed. A failed pull never silently applies a stash.

## Verification

The Rust module includes focused tests for path traversal, Git-metadata exclusion,
status-token changes, and unstage behavior for both tracked edits and staged
new files. The GitManager Cleaner CI workflow runs the Tauri backend test suite;
the Archive Engine & gitm CLI CI and Native Installers workflows also compile
the shared Tauri backend across their platform matrices.

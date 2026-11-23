"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "../lib/tauri";
import type {
	GitRepository,
	PullExecutionResult,
	PullPreview,
	RepositoryCommitPreview,
	RepositoryFileEntry,
	WorktreeChange,
} from "../lib/types";

type WorkspaceSource = "local" | "remote";
type PullMode = "" | "none" | "stash" | "commit";
type WorkspaceFocus = "all" | "conflicts";

function messageOf(error: unknown): string {
	return error instanceof Error ? error.message : String(error);
}

function formatBytes(value: number): string {
	const size = Math.max(0, Number(value) || 0);
	if (size < 1024) return size + " B";
	if (size < 1024 * 1024) return (size / 1024).toFixed(1) + " KiB";
	if (size < 1024 * 1024 * 1024) return (size / (1024 * 1024)).toFixed(1) + " MiB";
	return (size / (1024 * 1024 * 1024)).toFixed(1) + " GiB";
}

function statusLabel(change: WorktreeChange): string {
	if (change.staged && change.worktreeStatus !== "Clean") {
		return change.indexStatus + " + " + change.worktreeStatus;
	}
	return change.staged ? "Staged " + change.indexStatus : change.worktreeStatus !== "Clean"
		? change.worktreeStatus
		: change.indexStatus;
}

export function RepositoryWorkspace({
	repo,
	onRepositoryUpdated,
	onOpenCleaner,
	focus = "all",
}: {
	repo: GitRepository;
	onRepositoryUpdated: (repo: GitRepository) => void;
	onOpenCleaner: () => void;
	focus?: WorkspaceFocus;
}) {
	const remoteBranches = useMemo(
		() => (repo.branches ?? []).filter((branch) => branch.is_remote),
		[repo.branches],
	);
	const initialRemoteBranch = useMemo(() => {
		const current = (repo.branches ?? []).find(
			(branch) => !branch.is_remote && branch.name === repo.current_branch,
		);
		const upstream = current?.upstream ?? "";
		return remoteBranches.some((branch) => branch.name === upstream)
			? upstream
			: remoteBranches.find((branch) => branch.name.startsWith("origin/"))?.name
				?? remoteBranches[0]?.name
				?? "";
	}, [repo.branches, repo.current_branch, remoteBranches]);

	const [source, setSource] = useState<WorkspaceSource>("local");
	const [remoteBranch, setRemoteBranch] = useState(initialRemoteBranch);
	const [currentPath, setCurrentPath] = useState("");
	const [entries, setEntries] = useState<RepositoryFileEntry[]>([]);
	const [changes, setChanges] = useState<WorktreeChange[]>([]);
	const [selectedPaths, setSelectedPaths] = useState<string[]>([]);
	const [selectedFile, setSelectedFile] = useState<RepositoryFileEntry | null>(null);
	const [previewTitle, setPreviewTitle] = useState("");
	const [previewText, setPreviewText] = useState("");
	const [previewKind, setPreviewKind] = useState<"file" | "diff">("file");
	const [editorDraft, setEditorDraft] = useState("");
	const [editorDirty, setEditorDirty] = useState(false);
	const [savingResolution, setSavingResolution] = useState(false);
	const [loadingFiles, setLoadingFiles] = useState(false);
	const [loadingChanges, setLoadingChanges] = useState(false);
	const [busy, setBusy] = useState(false);
	const [fetching, setFetching] = useState(false);
	const [error, setError] = useState("");
	const [notice, setNotice] = useState("");

	const [commitOpen, setCommitOpen] = useState(false);
	const [commitMessage, setCommitMessage] = useState("");
	const [commitPreview, setCommitPreview] = useState<RepositoryCommitPreview | null>(null);

	const [pullPreview, setPullPreview] = useState<PullPreview | null>(null);
	const [pullMode, setPullMode] = useState<PullMode>("");
	const [pullSelectedPaths, setPullSelectedPaths] = useState<string[]>([]);
	const [pullCommitMessage, setPullCommitMessage] = useState("");
	const [pullBusy, setPullBusy] = useState(false);
	const filesRequestId = useRef(0);
	const previewRequestId = useRef(0);

	const refreshChanges = useCallback(async () => {
		setLoadingChanges(true);
		try {
			const result = await invoke<WorktreeChange[]>("get_repository_changes", {
				repoPath: repo.path,
			});
			const next = result ?? [];
			setChanges(next);
			setSelectedPaths((current) => current.filter((path) => next.some((item) => item.path === path)));
		} catch (cause) {
			setError(messageOf(cause));
		} finally {
			setLoadingChanges(false);
		}
	}, [repo.path]);

	const refreshFiles = useCallback(async () => {
		const requestId = ++filesRequestId.current;
		if (source === "remote" && !remoteBranch) {
			setEntries([]);
			setLoadingFiles(false);
			setError("Fetch the remote first, then choose a remote branch to browse.");
			return;
		}
		setLoadingFiles(true);
		setError("");
		try {
			const result = await invoke<RepositoryFileEntry[]>("list_repository_files", {
				repoPath: repo.path,
				relativePath: currentPath || null,
				source,
				branch: source === "remote" ? remoteBranch : null,
			});
			if (requestId === filesRequestId.current) setEntries(result ?? []);
		} catch (cause) {
			if (requestId === filesRequestId.current) {
				setEntries([]);
				setError(messageOf(cause));
			}
		} finally {
			if (requestId === filesRequestId.current) setLoadingFiles(false);
		}
	}, [repo.path, source, remoteBranch, currentPath]);

	const refreshRepositoryDetails = useCallback(async () => {
		try {
			const updated = await invoke<GitRepository>("inspect_repository", { repoPath: repo.path });
			onRepositoryUpdated(updated);
		} catch (cause) {
			setError(messageOf(cause));
		}
	}, [repo.path, onRepositoryUpdated]);

	useEffect(() => {
		void refreshChanges();
	}, [refreshChanges]);

	useEffect(() => {
		void refreshFiles();
	}, [refreshFiles]);

	useEffect(() => {
		setRemoteBranch((current) => remoteBranches.some((branch) => branch.name === current)
			? current
			: initialRemoteBranch);
	}, [initialRemoteBranch, remoteBranches]);

	function toggleSelectedPath(path: string, checked: boolean) {
		setSelectedPaths((current) => checked
			? [...new Set([...current, path])]
			: current.filter((item) => item !== path));
	}

	function openDirectory(path: string) {
		previewRequestId.current += 1;
		setCurrentPath(path);
		setSelectedFile(null);
		setPreviewTitle("");
		setPreviewText("");
		setEditorDraft("");
		setEditorDirty(false);
	}

	function openParentDirectory() {
		const parts = currentPath.split("/").filter(Boolean);
		parts.pop();
		openDirectory(parts.join("/"));
	}

	async function previewFile(entry: RepositoryFileEntry) {
		if (entry.isDir) {
			openDirectory(entry.path);
			return;
		}
		if (entry.isSymlink) {
			setError("Symbolic links are not followed in the repository file preview.");
			return;
		}
		const requestId = ++previewRequestId.current;
		setSelectedFile(entry);
		setPreviewTitle(entry.path);
		setPreviewText("Loading file…");
		setEditorDraft("");
		setEditorDirty(false);
		setPreviewKind("file");
		setError("");
		try {
			const text = await invoke<string>("read_repository_file", {
				repoPath: repo.path,
				relativePath: entry.path,
				source,
				branch: source === "remote" ? remoteBranch : null,
			});
			if (requestId === previewRequestId.current) {
				setPreviewText(text);
				setEditorDraft(text);
				setEditorDirty(false);
			}
		} catch (cause) {
			if (requestId === previewRequestId.current) {
				setPreviewText("");
				setError(messageOf(cause));
			}
		}
	}

	async function saveConflictResolution() {
		if (focus !== "conflicts" || source !== "local" || !selectedFile || previewKind !== "file") return;
		setSavingResolution(true);
		setError("");
		setNotice("Saving resolved file…");
		try {
			const result = await invoke<string>("save_conflict_resolution", {
				repoPath: repo.path,
				relativePath: selectedFile.path,
				content: editorDraft,
			});
			setPreviewText(editorDraft);
			setEditorDirty(false);
			setNotice(result);
			await refreshChanges();
			await refreshRepositoryDetails();
		} catch (cause) {
			setError(messageOf(cause));
			setNotice("");
		} finally {
			setSavingResolution(false);
		}
	}

	async function previewDiff(change: WorktreeChange) {
		const requestId = ++previewRequestId.current;
		setPreviewTitle(change.path + (change.staged ? " · staged diff" : " · unstaged diff"));
		setPreviewText("Loading diff…");
		setEditorDraft("");
		setEditorDirty(false);
		setPreviewKind("diff");
		setSelectedFile(null);
		setError("");
		try {
			const text = await invoke<string>("read_repository_file_diff", {
				repoPath: repo.path,
				relativePath: change.path,
				staged: change.staged,
			});
			if (requestId === previewRequestId.current) setPreviewText(text);
		} catch (cause) {
			if (requestId === previewRequestId.current) {
				setPreviewText("");
				setError(messageOf(cause));
			}
		}
	}

	async function runPathOperation(operation: "stage_repository_paths" | "unstage_repository_paths") {
		const paths = selectedPaths.filter((path) => {
			const change = changes.find((item) => item.path === path);
			return change && (operation === "stage_repository_paths" ? !change.staged || change.worktreeStatus !== "Clean" : change.staged);
		});
		if (!paths.length) {
			setNotice(operation === "stage_repository_paths" ? "Select unstaged files to stage." : "Select staged files to unstage.");
			return;
		}
		setBusy(true);
		setError("");
		setNotice(operation === "stage_repository_paths" ? "Staging selected files…" : "Unstaging selected files…");
		try {
			const updated = await invoke<WorktreeChange[]>(operation, { repoPath: repo.path, paths });
			setChanges(updated ?? []);
			setSelectedPaths([]);
			setNotice(operation === "stage_repository_paths" ? "Selected files staged." : "Selected files unstaged. Working-tree edits were preserved.");
			await refreshRepositoryDetails();
		} catch (cause) {
			setError(messageOf(cause));
			setNotice("");
		} finally {
			setBusy(false);
		}
	}

	async function openCommitDialog() {
		setBusy(true);
		setError("");
		try {
			const preview = await invoke<RepositoryCommitPreview>("preview_repository_commit", {
				repoPath: repo.path,
			});
			if (!preview.changes.some((change) => change.staged)) {
				setNotice("There are no staged changes to commit. Stage files first.");
				return;
			}
			setCommitPreview(preview);
			setCommitMessage("");
			setCommitOpen(true);
		} catch (cause) {
			setError(messageOf(cause));
		} finally {
			setBusy(false);
		}
	}

	async function createCommit() {
		if (!commitPreview || !commitMessage.trim()) return;
		setBusy(true);
		setError("");
		try {
			const result = await invoke<[string, WorktreeChange[]]>("create_repository_commit", {
				repoPath: repo.path,
				message: commitMessage.trim(),
				expectedIndexTree: commitPreview.indexTree,
			});
			const [oid, updatedChanges] = result;
			setChanges(updatedChanges ?? []);
			setSelectedPaths([]);
			setCommitOpen(false);
			setCommitPreview(null);
			setCommitMessage("");
			setNotice("Created commit " + oid.slice(0, 8) + ".");
			await refreshRepositoryDetails();
		} catch (cause) {
			setError(messageOf(cause));
		} finally {
			setBusy(false);
		}
	}

	async function deleteEntry(entry: RepositoryFileEntry) {
		if (source !== "local") return;
		const accepted = window.confirm(
			"Delete " + entry.path + " from this local repository?\n\nThis removes the file or folder from disk. Tracked deletions remain visible in Changes and can be restored before committing; untracked data is not recoverable through Git.",
		);
		if (!accepted) return;
		setBusy(true);
		setError("");
		setNotice("Deleting " + entry.name + "…");
		try {
			const result = await invoke<string>("delete_repository_path", {
				repoPath: repo.path,
				relativePath: entry.path,
			});
			setNotice(result || "Deleted " + entry.path + ".");
			if (selectedFile?.path === entry.path) {
				setSelectedFile(null);
				setPreviewTitle("");
				setPreviewText("");
			}
			await Promise.all([refreshChanges(), refreshFiles(), refreshRepositoryDetails()]);
		} catch (cause) {
			setError(messageOf(cause));
			setNotice("");
		} finally {
			setBusy(false);
		}
	}

	async function fetchRemote() {
		const remote = (repo.remotes ?? []).find((item) => item.name === "origin") ?? repo.remotes?.[0];
		if (!remote) {
			setError("This repository has no configured remote.");
			return;
		}
		setFetching(true);
		setError("");
		setNotice("Fetching " + remote.name + "…");
		try {
			const result = await invoke<string>("fetch_repository_remote", {
				repoPath: repo.path,
				remoteName: remote.name,
			});
			const updated = await invoke<GitRepository>("inspect_repository", { repoPath: repo.path });
			onRepositoryUpdated(updated);
			setNotice(result || "Remote references updated.");
			await refreshFiles();
		} catch (cause) {
			setError(messageOf(cause));
			setNotice("");
		} finally {
			setFetching(false);
		}
	}

	async function openPullPreview() {
		setPullBusy(true);
		setError("");
		setNotice("");
		try {
			const preview = await invoke<PullPreview>("preview_repository_pull", {
				repoPath: repo.path,
			});
			setPullPreview(preview);
			setPullMode(preview.changes.length ? "" : "none");
			setPullSelectedPaths(preview.changes.map((change) => change.path));
			setPullCommitMessage("");
		} catch (cause) {
			setError(messageOf(cause));
		} finally {
			setPullBusy(false);
		}
	}

	function togglePullPath(path: string, checked: boolean) {
		setPullSelectedPaths((current) => checked
			? [...new Set([...current, path])]
			: current.filter((item) => item !== path));
	}

	const hasPullConflicts = Boolean(pullPreview?.changes.some(
		(change) => change.indexStatus === "Conflict" || change.worktreeStatus === "Conflict",
	));
	const canConfirmPull = Boolean(
		pullPreview
		&& pullPreview.canPull
		&& !hasPullConflicts
		&& !pullBusy
		&& (
			pullPreview.changes.length === 0
				? pullMode === "none"
				: pullMode === "stash"
					|| (pullMode === "commit" && pullSelectedPaths.length > 0 && pullCommitMessage.trim().length > 0)
		),
	);

	async function confirmPull() {
		if (!pullPreview || !canConfirmPull) return;
		setPullBusy(true);
		setError("");
		setNotice("Preparing local changes and pulling…");
		try {
			const result = await invoke<PullExecutionResult>("prepare_and_pull_repository_update", {
				repoPath: repo.path,
				expectedBranch: pullPreview.branch,
				expectedLocalOid: pullPreview.localOid,
				expectedRemoteOid: pullPreview.remoteOid,
				expectedStatusToken: pullPreview.statusToken,
				mode: pullMode,
				stagePaths: pullMode === "commit" ? pullSelectedPaths : [],
				commitMessage: pullMode === "commit" ? pullCommitMessage.trim() : "",
			});
			setPullPreview(null);
			setPullMode("");
			setNotice(
				result.message
				+ (result.commitCreated ? " Local changes were committed and rebased." : "")
				+ (result.stashCreated ? " Unselected/local changes were saved in Git stash." : ""),
			);
			await Promise.all([refreshChanges(), refreshFiles(), refreshRepositoryDetails()]);
		} catch (cause) {
			setError(messageOf(cause));
			setNotice("");
		} finally {
			setPullBusy(false);
		}
	}

	const selectedChangeSet = useMemo(
		() => new Set(selectedPaths),
		[selectedPaths],
	);
	const selectedStagedCount = changes.filter((change) => selectedChangeSet.has(change.path) && change.staged).length;
	const selectedUnstagedCount = changes.filter((change) => selectedChangeSet.has(change.path) && (!change.staged || change.worktreeStatus !== "Clean")).length;
	const stagedCount = changes.filter((change) => change.staged).length;
	const unstagedCount = changes.filter((change) => !change.staged || change.worktreeStatus !== "Clean").length;
	const visibleChanges = focus === "conflicts"
		? changes.filter((change) => change.indexStatus === "Conflict" || change.worktreeStatus === "Conflict")
		: changes;
	const pathParts = currentPath.split("/").filter(Boolean);

	return (
		<section className="gm-workspace-panel">
			<div className="gm-workspace-heading">
				<div>
					<div className="gm-eyebrow">WORKING COPY</div>
					<h2>{focus === "conflicts" ? "Conflict Resolver" : "Code &amp; Changes"}</h2>
					<p>{focus === "conflicts"
						? "Inspect unresolved paths, edit them in your local editor, then stage the resolved files."
						: "Browse files, review changes, stage selected paths and commit locally."}</p>
				</div>
				<div className="gm-workspace-summary">
					<span><strong>{stagedCount}</strong> staged</span>
					<span><strong>{unstagedCount}</strong> unstaged</span>
					{focus === "conflicts" && <span><strong>{visibleChanges.length}</strong> conflicts</span>}
				</div>
			</div>

			<div className="gm-workspace-toolbar">
				<div className="gm-workspace-source" role="group" aria-label="File source">
					<button type="button" className={source === "local" ? "active" : ""} aria-pressed={source === "local"} onClick={() => { previewRequestId.current += 1; setSource("local"); setCurrentPath(""); setSelectedFile(null); setPreviewTitle(""); setPreviewText(""); }}>Local</button>
					<button type="button" className={source === "remote" ? "active" : ""} aria-pressed={source === "remote"} onClick={() => { previewRequestId.current += 1; setSource("remote"); setCurrentPath(""); setSelectedFile(null); setPreviewTitle(""); setPreviewText(""); }}>Remote</button>
				</div>
				{source === "remote" && (
					<select className="gm-workspace-select" value={remoteBranch} onChange={(event) => { previewRequestId.current += 1; setRemoteBranch(event.target.value); setCurrentPath(""); setSelectedFile(null); setPreviewTitle(""); setPreviewText(""); }} aria-label="Remote branch" disabled={!remoteBranches.length}>
						{remoteBranches.length === 0 && <option value="">No fetched remote branches</option>}
						{remoteBranches.map((branch) => <option key={branch.name} value={branch.name}>{branch.name}</option>)}
					</select>
				)}
				<div className="gm-workspace-toolbar-spacer" />
				<button type="button" className="gm-button gm-button-secondary" onClick={() => void fetchRemote()} disabled={!repo.has_remote || fetching || busy}>
					{fetching ? "Fetching…" : "Fetch remote"}
				</button>
				<button type="button" className="gm-button gm-button-primary" onClick={() => void openPullPreview()} disabled={!repo.has_remote || pullBusy || busy}>
					{pullBusy && !pullPreview ? "Checking…" : "Pull / Sync"}
				</button>
				<button type="button" className="gm-button gm-button-secondary" onClick={onOpenCleaner}>Developer Cleaner</button>
				<button type="button" className="gm-button gm-button-ghost" onClick={() => { void refreshChanges(); void refreshFiles(); }} disabled={busy || loadingFiles || loadingChanges}>Refresh</button>
			</div>

			{error && <div className="gm-workspace-error" role="alert">{error}<button type="button" onClick={() => setError("")}>Dismiss</button></div>}
			{notice && <div className="gm-workspace-notice" role="status">{notice}</div>}

			<div className="gm-workspace-layout gm-workbench-layout">
				<section className="gm-workspace-card gm-workspace-changes" aria-label="Source control">
					<div className="gm-workspace-card-heading">
						<div><h3>{focus === "conflicts" ? "Unresolved Files" : "Source Control"}</h3><span>{visibleChanges.length === 0 ? "No matching paths" : `${visibleChanges.length} changed paths`}</span></div>
						<span className={visibleChanges.length === 0 ? "gm-workspace-card-hint gm-workspace-clean" : "gm-workspace-card-hint"}>{visibleChanges.length === 0 ? "Clean" : focus === "conflicts" ? "Conflicts" : "Changes"}</span>
					</div>
					{loadingChanges ? (
						<div className="gm-workspace-empty">Reading Git status…</div>
					) : visibleChanges.length === 0 ? (
						<div className="gm-workspace-empty gm-workspace-source-empty">
							<strong>{focus === "conflicts" ? "No unresolved conflicts" : "Working tree clean"}</strong>
							<span>{focus === "conflicts"
								? "GitManager has not detected any unmerged paths in this working tree."
								: "Changed files will appear here. Select a file to inspect its diff, then stage and commit it."}</span>
						</div>
					) : (
						<div className="gm-workspace-change-list">
							{visibleChanges.map((change) => (
								<div className="gm-workspace-change" key={change.path}>
									<label className="gm-workspace-check">
										<input type="checkbox" checked={selectedChangeSet.has(change.path)} onChange={(event) => toggleSelectedPath(change.path, event.target.checked)} />
										<span />
									</label>
									<button type="button" className="gm-workspace-change-path" onClick={() => void previewDiff(change)} title="Preview this file's diff">
										<strong>{change.path}</strong>
										<small>{statusLabel(change)}</small>
									</button>
									<span className={change.staged ? "gm-workspace-state staged" : "gm-workspace-state"}>{change.staged ? "Staged" : "Unstaged"}</span>
									<button type="button" className="gm-workspace-mini" onClick={() => void previewDiff(change)}>Diff</button>
								</div>
							))}
						</div>
					)}
					<div className="gm-workspace-actions">
						<button type="button" className="gm-button gm-button-secondary" onClick={() => void runPathOperation("stage_repository_paths")} disabled={busy || selectedUnstagedCount === 0}>Stage</button>
						<button type="button" className="gm-button gm-button-secondary" onClick={() => void runPathOperation("unstage_repository_paths")} disabled={busy || selectedStagedCount === 0}>Unstage</button>
						{focus !== "conflicts" && <button type="button" className="gm-button gm-button-primary" onClick={() => void openCommitDialog()} disabled={busy || stagedCount === 0}>Commit</button>}
					</div>
				</section>

				<section className="gm-workspace-card gm-workspace-files" aria-label="Repository explorer">
					<div className="gm-workspace-card-heading">
						<div><h3>Explorer</h3><span>{source === "local" ? "Local working copy" : (remoteBranch || "Remote branch")}</span></div>
						<span className="gm-workspace-card-hint">{entries.length} items</span>
					</div>
					<div className="gm-workspace-breadcrumbs">
						<button type="button" onClick={() => openDirectory("")}>/</button>
						{pathParts.map((part, index) => (
							<span key={part + index}>
								<span className="gm-workspace-crumb-separator">/</span>
								<button type="button" onClick={() => openDirectory(pathParts.slice(0, index + 1).join("/"))}>{part}</button>
							</span>
						))}
						{currentPath && <button type="button" className="gm-workspace-up" onClick={openParentDirectory}>↑ Up</button>}
					</div>
					{loadingFiles ? (
						<div className="gm-workspace-empty">Loading {source} files…</div>
					) : entries.length === 0 ? (
						<div className="gm-workspace-empty">This folder is empty, or could not be listed.</div>
					) : (
						<div className="gm-workspace-file-list">
							{entries.map((entry) => (
								<div className={selectedFile?.path === entry.path ? "gm-workspace-file selected" : "gm-workspace-file"} key={entry.path}>
									<button type="button" className="gm-workspace-file-open" onClick={() => void previewFile(entry)} disabled={entry.isSymlink}>
										<span className={entry.isDir ? "gm-workspace-file-icon folder" : "gm-workspace-file-icon"}>{entry.isDir ? "▰" : entry.isSymlink ? "↗" : "▤"}</span>
										<strong>{entry.name}</strong>
										<small>{entry.isDir ? "Folder" : entry.isSymlink ? "Symbolic link" : formatBytes(entry.sizeBytes)}</small>
									</button>
									{source === "local" && (
										<button type="button" className="gm-workspace-delete" onClick={() => void deleteEntry(entry)} disabled={busy} title={"Delete " + entry.path}>Delete</button>
									)}
								</div>
							))}
						</div>
					)}
				</section>

				<aside className="gm-workspace-card gm-workspace-preview" aria-label="File editor and preview">
					<div className="gm-workspace-card-heading">
						<div><h3>{previewKind === "diff" ? "Diff Editor" : "Editor"}</h3><span>{previewTitle || "No file selected"}</span></div>
						<button type="button" className="gm-workspace-mini" onClick={() => { previewRequestId.current += 1; setPreviewTitle(""); setPreviewText(""); setEditorDraft(""); setEditorDirty(false); setSelectedFile(null); }}>Clear</button>
					</div>
					{focus === "conflicts" && source === "local" && selectedFile && previewKind === "file" ? (
						<div className="gm-workspace-conflict-editor">
							<div className="gm-workspace-conflict-editor-note">Edit the merged contents below. Saving writes only this currently conflicted file; use Stage in Source Control to mark it resolved.</div>
							<textarea
								className="gm-workspace-preview-text gm-workspace-conflict-textarea"
								value={editorDraft}
								onChange={(event) => { setEditorDraft(event.target.value); setEditorDirty(true); }}
								disabled={previewText === "Loading file…" || savingResolution}
								spellCheck={false}
								aria-label="Resolved conflict file contents"
								maxLength={1024 * 1024}
							/>
							<div className="gm-workspace-conflict-editor-actions">
								<span>{editorDraft.length.toLocaleString()} characters{editorDirty ? " · unsaved" : ""}</span>
								<button type="button" className="gm-button gm-button-primary" onClick={() => void saveConflictResolution()} disabled={!editorDirty || savingResolution || loadingFiles || previewText === "Loading file…"}>
									{savingResolution ? "Saving…" : "Save resolved file"}
								</button>
							</div>
						</div>
					) : previewText ? (
						<pre className="gm-workspace-preview-text">{previewText}</pre>
					) : (
						<div className="gm-workspace-empty gm-workspace-editor-empty">
							<div>
								<strong>Select a file to open it</strong>
								<span>Choose a file in Explorer to read its UTF-8 contents, or select a changed path in Source Control to inspect its Git diff.</span>
							</div>
						</div>
					)}
					{selectedFile && <div className="gm-workspace-preview-meta">{formatBytes(selectedFile.sizeBytes)} · {source === "local" ? "Local disk" : "Remote branch"}</div>}
				</aside>
			</div>

			{commitOpen && (
				<div className="gm-workspace-modal-backdrop" role="presentation">
					<section className="gm-workspace-modal" role="dialog" aria-modal="true" aria-labelledby="gm-workspace-commit-title">
						<div className="gm-workspace-modal-heading">
							<div><div className="gm-eyebrow">LOCAL GIT OPERATION</div><h3 id="gm-workspace-commit-title">Commit staged changes</h3></div>
							<button type="button" onClick={() => setCommitOpen(false)} disabled={busy}>×</button>
						</div>
						<p>{commitPreview?.changes.filter((change) => change.staged).length ?? 0} staged paths will be included. Unstaged changes remain in your working tree.</p>
						<div className="gm-workspace-commit-paths" aria-label="Staged paths included in this commit">
							{commitPreview?.changes.filter((change) => change.staged).slice(0, 30).map((change) => (
								<div key={change.path}><strong>{change.path}</strong><small>{change.indexStatus}</small></div>
							))}
							{(commitPreview?.changes.filter((change) => change.staged).length ?? 0) > 30 && (
								<p>And {(commitPreview?.changes.filter((change) => change.staged).length ?? 0) - 30} more staged paths…</p>
							)}
						</div>
						<label className="gm-workspace-message">
							<span>Commit message</span>
							<textarea rows={3} maxLength={200} value={commitMessage} onChange={(event) => setCommitMessage(event.target.value)} placeholder="Describe this change…" />
							<small>{commitMessage.length}/200</small>
						</label>
						<div className="gm-workspace-modal-actions">
							<button type="button" className="gm-button gm-button-secondary" onClick={() => setCommitOpen(false)} disabled={busy}>Cancel</button>
							<button type="button" className="gm-button gm-button-primary" onClick={() => void createCommit()} disabled={busy || !commitMessage.trim()}>{busy ? "Committing…" : "Create commit"}</button>
						</div>
					</section>
				</div>
			)}

			{pullPreview && (
				<div className="gm-workspace-modal-backdrop" role="presentation">
					<section className="gm-workspace-modal gm-workspace-pull-modal" role="dialog" aria-modal="true" aria-labelledby="gm-workspace-pull-title">
						<div className="gm-workspace-modal-heading">
							<div><div className="gm-eyebrow">SYNC REVIEW</div><h3 id="gm-workspace-pull-title">Pull {pullPreview.remoteName}/{pullPreview.remoteBranch}</h3></div>
							<button type="button" onClick={() => { if (!pullBusy) setPullPreview(null); }} disabled={pullBusy}>×</button>
						</div>
						<p className="gm-workspace-pull-note">This preview fetches remote references. It does not change your local files or checked-out branch. Local preparation only starts after you confirm below.</p>
						<div className="gm-workspace-pull-stats">
							<div><span>Current branch</span><strong>{pullPreview.branch}</strong></div>
							<div><span>Incoming commits</span><strong>{pullPreview.commitsBehind}</strong></div>
							<div><span>Local-only commits</span><strong>{pullPreview.commitsAhead}</strong></div>
							<div><span>Local changes</span><strong>{pullPreview.changes.length}</strong></div>
						</div>
						{hasPullConflicts && (
							<div className="gm-workspace-pull-blocked" role="alert">
								Resolve existing merge conflicts before preparing this Pull. No local changes will be staged or stashed.
							</div>
						)}
						{!pullPreview.canPull && (
							<div className="gm-workspace-pull-blocked" role="alert">
								{pullPreview.commitsAhead > 0
									? "This branch has local-only commits. Push or reconcile them before using this fast-forward Pull."
									: pullPreview.commitsBehind === 0
										? "The branch is already up to date. There is nothing to Pull."
										: "This branch is not a fast-forward-only Pull candidate. Review its history before continuing."}
							</div>
						)}
						{pullPreview.changes.length > 0 && (
							<div className="gm-workspace-pull-preparation">
								<h4>Prepare local changes</h4>
								<p>Choose whether to save all changes in Git stash, or commit selected paths and keep the unchecked paths stashed.</p>
								<div className="gm-workspace-pull-options">
									<label className={pullMode === "stash" ? "selected" : ""}>
										<input type="radio" name="workspace-pull-mode" checked={pullMode === "stash"} onChange={() => setPullMode("stash")} />
										<span><strong>Stash all</strong><small>Keep staged, unstaged and untracked changes in a Git stash before Pull.</small></span>
									</label>
									<label className={pullMode === "commit" ? "selected" : ""}>
										<input type="radio" name="workspace-pull-mode" checked={pullMode === "commit"} onChange={() => setPullMode("commit")} />
										<span><strong>Commit selected paths</strong><small>Commit checked paths; save every unchecked path in Git stash.</small></span>
									</label>
								</div>
								{pullMode === "commit" && (
									<>
										<div className="gm-workspace-pull-paths">
											{pullPreview.changes.map((change) => (
												<label key={change.path}>
													<input type="checkbox" checked={pullSelectedPaths.includes(change.path)} onChange={(event) => togglePullPath(change.path, event.target.checked)} />
													<span><strong>{change.path}</strong><small>{statusLabel(change)}</small></span>
												</label>
											))}
										</div>
										<label className="gm-workspace-message">
											<span>Commit message</span>
											<textarea rows={2} maxLength={200} value={pullCommitMessage} onChange={(event) => setPullCommitMessage(event.target.value)} placeholder="Describe the local changes…" />
										</label>
									</>
								)}
							</div>
						)}
						{pullPreview.changes.length === 0 && <p className="gm-workspace-pull-note">The working tree is clean. Pull will only perform the verified fast-forward update.</p>}
						<div className="gm-workspace-modal-actions">
							<button type="button" className="gm-button gm-button-secondary" onClick={() => setPullPreview(null)} disabled={pullBusy}>Cancel</button>
							<button type="button" className="gm-button gm-button-primary" onClick={() => void confirmPull()} disabled={!canConfirmPull}>{pullBusy ? "Applying…" : "Confirm Pull"}</button>
						</div>
					</section>
				</div>
			)}
		</section>
	);
}

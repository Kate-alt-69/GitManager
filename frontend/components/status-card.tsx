"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import { EngineDrawer } from "@/engine";
import { invoke } from "../lib/tauri";
import type {
	CommitHistoryPage,
	CommitNode,
	GitRepository,
	PullPreview,
	WorktreeChange,
} from "../lib/types";
import { Button } from "./shared-ui";

function relativeTime(timestamp: number): string {
	if (!timestamp) return "—";
	const delta = Math.max(0, Math.floor(Date.now() / 1000) - timestamp);
	if (delta < 60) return "just now";
	const mins = Math.floor(delta / 60);
	if (mins < 60) return `${mins}m ago`;
	const hours = Math.floor(mins / 60);
	if (hours < 24) return `${hours}h ago`;
	return `${Math.floor(hours / 24)}d ago`;
}

/**
 * Interactive Classic Cozy StatusCard — GUI sibling of `gitm status -i`.
 * Shows live Behind/Ahead counts, Top 5 incoming/outgoing commits, and cleanly
 * grouped Staged / Modified / Untracked files with an inline EngineDrawer
 * clean file & diff viewer on click.
 */
export function StatusCard({
	repo,
	onOpenPull,
}: {
	repo: GitRepository;
	onOpenPull: () => void;
}) {
	const [pullInfo, setPullInfo] = useState<PullPreview | null>(null);
	const [changes, setChanges] = useState<WorktreeChange[]>([]);
	const [aheadCommits, setAheadCommits] = useState<CommitNode[]>([]);
	const [behindCommits, setBehindCommits] = useState<CommitNode[]>([]);
	const [loading, setLoading] = useState(false);
	const [expanded, setExpanded] = useState(true);

	// Clean File / Diff Viewer Drawer state
	const [viewerOpen, setViewerOpen] = useState(false);
	const [viewerFile, setViewerFile] = useState<string>("");
	const [viewerMode, setViewerMode] = useState<"file" | "diff">("file");
	const [viewerContent, setViewerContent] = useState<string>("");
	const [viewerLoading, setViewerLoading] = useState(false);

	const refreshCard = useCallback(async () => {
		setLoading(true);
		try {
			const [preview, worktreeChanges] = await Promise.all([
				invoke<PullPreview>("preview_repository_pull", { repoPath: repo.path }).catch(() => null),
				invoke<WorktreeChange[]>("get_repository_changes", { repoPath: repo.path }).catch(() => []),
			]);
			setPullInfo(preview);
			setChanges(worktreeChanges ?? []);

			// Load Top 5 local ahead commits and Top 5 remote behind commits when diverged.
			if (preview && preview.commitsAhead > 0) {
				const localPage = await invoke<CommitHistoryPage>("load_repository_commit_history", {
					repoPath: repo.path,
					offset: 0,
					limit: Math.min(5, preview.commitsAhead),
					branchFilter: preview.branch || null,
					searchQuery: null,
				}).catch(() => null);
				setAheadCommits(localPage?.commits ?? []);
			} else {
				setAheadCommits([]);
			}

			if (preview && preview.commitsBehind > 0 && preview.remoteBranch) {
				const remotePage = await invoke<CommitHistoryPage>("load_repository_commit_history", {
					repoPath: repo.path,
					offset: 0,
					limit: Math.min(5, preview.commitsBehind),
					branchFilter: preview.remoteBranch,
					searchQuery: null,
				}).catch(() => null);
				setBehindCommits(remotePage?.commits ?? []);
			} else {
				setBehindCommits([]);
			}
		} finally {
			setLoading(false);
		}
	}, [repo.path]);

	useEffect(() => {
		void refreshCard();
	}, [refreshCard]);

	const groupedFiles = useMemo(() => {
		const staged: WorktreeChange[] = [];
		const modified: WorktreeChange[] = [];
		const untracked: WorktreeChange[] = [];
		for (const item of changes) {
			if (item.staged) staged.push(item);
			const wt = (item.worktreeStatus || "").toLowerCase();
			if (wt.includes("untracked") || wt === "new") {
				untracked.push(item);
			} else if (wt && wt !== "clean") {
				modified.push(item);
			}
		}
		return { staged, modified, untracked };
	}, [changes]);

	const openFileViewer = useCallback(async (relativePath: string, mode: "file" | "diff", staged = false) => {
		setViewerFile(relativePath);
		setViewerMode(mode);
		setViewerOpen(true);
		setViewerLoading(true);
		setViewerContent("");
		try {
			if (mode === "diff") {
				const diffText = await invoke<string>("read_repository_file_diff", {
					repoPath: repo.path,
					relativePath,
					staged,
				});
				setViewerContent(diffText || "No diff lines available for this file.");
			} else {
				const filePayload = await invoke<{ content?: string } | string>("read_repository_file", {
					repoPath: repo.path,
					relativePath,
					source: "local",
					branch: null,
				});
				setViewerContent(
					typeof filePayload === "string"
						? filePayload
						: (filePayload?.content ?? "Binary or empty file."),
				);
			}
		} catch (cause) {
			setViewerContent(cause instanceof Error ? cause.message : String(cause));
		} finally {
			setViewerLoading(false);
		}
	}, [repo.path]);

	const behindCount = pullInfo?.commitsBehind ?? 0;
	const aheadCount = pullInfo?.commitsAhead ?? 0;
	const totalFiles = groupedFiles.staged.length + groupedFiles.modified.length + groupedFiles.untracked.length;
	const upstreamLabel = pullInfo?.remoteBranch || "no upstream";
	const viewerLines = useMemo(() => viewerContent.split(/\r?\n/), [viewerContent]);

	return (
		<section className="gm-status-card" aria-label="Repository Sync and Files StatusCard">
			<div className="gm-sc-header">
				<div className="gm-sc-title">
					<span className={totalFiles > 0 ? "gm-led dirty" : "gm-led clean"} />
					<strong>StatusCard</strong>
					<code className="gm-branch-tag">{repo.current_branch || "HEAD"} ↔ {upstreamLabel}</code>
					{behindCount === 0 && aheadCount === 0 ? (
						<span className="gm-sc-pill synced">✓ Up to date</span>
					) : (
						<>
							<span className={behindCount > 0 ? "gm-sc-pill behind" : "gm-sc-pill"}>↓ {behindCount} behind</span>
							<span className={aheadCount > 0 ? "gm-sc-pill ahead" : "gm-sc-pill"}>↑ {aheadCount} ahead</span>
						</>
					)}
					<span className="gm-sc-pill">{groupedFiles.staged.length} staged · {groupedFiles.modified.length} modified · {groupedFiles.untracked.length} untracked</span>
				</div>

				<div className="gm-sc-actions">
					{behindCount > 0 && (
						<Button variant="primary" onClick={onOpenPull}>Pull ({behindCount})</Button>
					)}
					<Button onClick={() => void refreshCard()} disabled={loading}>
						{loading ? "Checking…" : "↻ Refresh"}
					</Button>
					<button
						type="button"
						className="gm-mini-btn"
						onClick={() => setExpanded((value) => !value)}
					>
						{expanded ? "Hide" : "Expand"}
					</button>
				</div>
			</div>

			{expanded && (
				<div className="gm-sc-body">
					{(behindCount > 0 || aheadCount > 0) && (
						<div className="gm-sc-sync-grid">
							<div className="gm-sc-col">
								<div className="gm-sc-col-head">
									<span>BEHIND (Top 5 of {behindCount})</span>
								</div>
								{behindCommits.length === 0 ? (
									<div className="gm-sc-empty">No incoming commits behind upstream.</div>
								) : (
									<ul className="gm-sc-list">
										{behindCommits.slice(0, 5).map((commit) => (
											<li key={commit.oid} className="gm-sc-commit-row">
												<code className="gm-sc-sha behind">↓ {commit.short_oid}</code>
												<span className="gm-sc-msg" title={commit.summary}>{commit.summary}</span>
												<time>{relativeTime(commit.timestamp)}</time>
											</li>
										))}
									</ul>
								)}
								{behindCount > 5 && (
									<div className="gm-sc-more">+ {behindCount - 5} more commits behind</div>
								)}
							</div>

							<div className="gm-sc-col">
								<div className="gm-sc-col-head">
									<span>AHEAD (Top 5 of {aheadCount})</span>
								</div>
								{aheadCommits.length === 0 ? (
									<div className="gm-sc-empty">No unpushed local commits.</div>
								) : (
									<ul className="gm-sc-list">
										{aheadCommits.slice(0, 5).map((commit) => (
											<li key={commit.oid} className="gm-sc-commit-row">
												<code className="gm-sc-sha ahead">↑ {commit.short_oid}</code>
												<span className="gm-sc-msg" title={commit.summary}>{commit.summary}</span>
												<time>{relativeTime(commit.timestamp)}</time>
											</li>
										))}
									</ul>
								)}
								{aheadCount > 5 && (
									<div className="gm-sc-more">+ {aheadCount - 5} more commits ahead</div>
								)}
							</div>
						</div>
					)}

					<div className="gm-sc-files-strip">
						{totalFiles === 0 ? (
							<div className="gm-sc-empty">✓ Working tree clean — no uncommitted, staged, or untracked files.</div>
						) : (
							<div className="gm-sc-files-grid">
								{(
									[
										{ label: "Staged", cls: "staged", badge: "+", items: groupedFiles.staged, isStaged: true },
										{ label: "Modified", cls: "modified", badge: "~", items: groupedFiles.modified, isStaged: false },
										{ label: "Untracked", cls: "untracked", badge: "?", items: groupedFiles.untracked, isStaged: false },
									] as const
								).map((group) => (
									<div key={group.label} className="gm-sc-file-group">
										<div className="gm-sc-col-head">
											<span>{group.label} ({group.items.length})</span>
										</div>
										{group.items.length === 0 ? (
											<div className="gm-sc-empty">None</div>
										) : (
											<ul className="gm-sc-file-list">
												{group.items.slice(0, 8).map((file) => (
													<li key={`${group.label}-${file.path}`} className="gm-sc-file-row">
														<button
															type="button"
															className="gm-sc-file-btn"
															onClick={() => void openFileViewer(file.path, "file", group.isStaged)}
															title={`View ${file.path}`}
														>
															<code className={`gm-sc-badge ${group.cls}`}>{group.badge}</code>
															<span>{file.path}</span>
														</button>
														{group.label !== "Untracked" && (
															<button
																type="button"
																className="gm-mini-btn"
																onClick={() => void openFileViewer(file.path, "diff", group.isStaged)}
															>
																Diff
															</button>
														)}
													</li>
												))}
											</ul>
										)}
									</div>
								))}
							</div>
						)}
					</div>
				</div>
			)}

			<EngineDrawer
				open={viewerOpen}
				onOpenChange={setViewerOpen}
				side="right"
				size="min(720px, 90vw)"
				title={viewerFile || "File Viewer"}
				description={viewerMode === "diff" ? "Unified Diff" : "Clean File Preview"}
				duration={180}
			>
				<div className="gm-clean-viewer">
					<div className="gm-clean-viewer-bar">
						<div className="gm-segmented">
							<button
								type="button"
								className={viewerMode === "file" ? "active" : ""}
								onClick={() => void openFileViewer(viewerFile, "file", false)}
							>
								File
							</button>
							<button
								type="button"
								className={viewerMode === "diff" ? "active" : ""}
								onClick={() => void openFileViewer(viewerFile, "diff", false)}
							>
								Diff
							</button>
						</div>
						<span className="gm-mono-pill">{viewerLines.length} lines</span>
					</div>

					{viewerLoading ? (
						<div className="gm-sc-empty">Loading…</div>
					) : (
						<pre className="gm-clean-viewer-code">
							{viewerLines.map((line, index) => {
								const lineCls = viewerMode === "diff"
									? line.startsWith("+")
										? "gm-cv-line add"
										: line.startsWith("-")
											? "gm-cv-line del"
											: "gm-cv-line"
									: "gm-cv-line";
								return (
									<div key={index} className={lineCls}>
										<span className="gm-cv-num">{index + 1}</span>
										<span className="gm-cv-text">{line || " "}</span>
									</div>
								);
							})}
						</pre>
					)}
				</div>
			</EngineDrawer>
		</section>
	);
}
